//! WebSocket 一次性票据
//!
//! 浏览器 WebSocket 无法携带 `Authorization` 头，因此采用「先 Bearer 换票据、
//! 再带票据升级 WS」：票据是 `uuid v4`（122 bit 随机）、**单次消费**、
//! 绑定 `session_id`、默认 60 秒过期，绝不把 JWT 放进 URL（避免日志 / 历史泄露）。
//!
//! 票据只存在于进程内，与 §10.1 的单实例 / sticky 约束一致。

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use dashmap::DashMap;

/// 默认票据有效期（秒）
const DEFAULT_TICKET_TTL_SECS: u64 = 60;

/// 票据条目
struct Ticket {
    user_id: i64,
    session_id: i64,
    expires_at: Instant,
}

/// 进程内票据存储
pub struct TicketStore {
    inner: DashMap<String, Ticket>,
}

impl TicketStore {
    /// 新建空存储
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: DashMap::new(),
        }
    }

    /// 票据有效期（秒，`SSH_TICKET_TTL_SECS` 可覆盖）
    #[must_use]
    pub fn ttl_secs() -> u64 {
        std::env::var("SSH_TICKET_TTL_SECS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_TICKET_TTL_SECS)
    }

    /// 签发票据，返回 `(票据, 有效期秒数)`
    ///
    /// 顺带清理已过期条目，避免长期运行后 DashMap 里堆积垃圾。
    pub fn issue(&self, user_id: i64, session_id: i64) -> (String, u64) {
        let ttl = Self::ttl_secs();
        let ticket = self.issue_with_ttl(user_id, session_id, Duration::from_secs(ttl));
        (ticket, ttl)
    }

    /// 单次消费：返回 `user_id`；票据不存在 / 已过期 / session 不匹配一律 `None`
    #[must_use]
    pub fn consume(&self, ticket: &str, session_id: i64) -> Option<i64> {
        let (_, entry) = self.inner.remove(ticket)?;
        if entry.expires_at <= Instant::now() || entry.session_id != session_id {
            return None;
        }
        Some(entry.user_id)
    }

    /// 当前有效票据数量（自检 / 单测用）
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// 是否没有票据
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// 指定 TTL 签发（便于单测过期分支），返回票据串
    fn issue_with_ttl(&self, user_id: i64, session_id: i64, ttl: Duration) -> String {
        let now = Instant::now();
        self.inner.retain(|_, entry| entry.expires_at > now);
        let ticket = uuid::Uuid::new_v4().to_string();
        self.inner.insert(
            ticket.clone(),
            Ticket {
                user_id,
                session_id,
                expires_at: now + ttl,
            },
        );
        ticket
    }
}

impl Default for TicketStore {
    fn default() -> Self {
        Self::new()
    }
}

/// 进程级票据存储（api 层所有 SSH 路由共用一个实例）
pub fn store() -> &'static TicketStore {
    static STORE: OnceLock<TicketStore> = OnceLock::new();
    STORE.get_or_init(TicketStore::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_is_single_use() {
        let store = TicketStore::new();
        let (ticket, ttl) = store.issue(7, 100);
        assert!(ttl > 0);
        assert_eq!(store.consume(&ticket, 100), Some(7));
        // 第二次消费必须失败（单次消费）
        assert_eq!(store.consume(&ticket, 100), None);
        assert!(store.is_empty());
    }

    #[test]
    fn ticket_is_bound_to_session() {
        let store = TicketStore::new();
        let (ticket, _) = store.issue(7, 100);
        // session 不匹配：消费失败，且票据已被移除（不可重放）
        assert_eq!(store.consume(&ticket, 101), None);
        assert_eq!(store.consume(&ticket, 100), None);
    }

    #[test]
    fn expired_ticket_is_rejected() {
        let store = TicketStore::new();
        let ticket = store.issue_with_ttl(7, 100, Duration::from_secs(0));
        assert_eq!(store.consume(&ticket, 100), None);
    }

    #[test]
    fn unknown_ticket_is_rejected() {
        let store = TicketStore::new();
        assert_eq!(store.consume("not-a-ticket", 100), None);
    }

    #[test]
    fn expired_tickets_are_pruned_on_issue() {
        let store = TicketStore::new();
        let _ = store.issue_with_ttl(7, 100, Duration::from_secs(0));
        // 签发新票据时顺带清理过期项
        let _ = store.issue(8, 101);
        assert_eq!(store.len(), 1);
    }
}
