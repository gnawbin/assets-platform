//! 浏览器会话（OAuth 2.1 方案 B）的数据访问层
//!
//! 设计文档 §7.3：`/oauth/authorize` 的标准流程依赖**浏览器会话 Cookie**，
//! 而本项目此前只有 Bearer JWT。方案 B 引入本表 + 本模块，只做三件事：
//!
//! 1. 登录成功时签发会话令牌（`ss_<43>`，只存 `SHA-256`），写入 httpOnly Cookie；
//! 2. `GET /oauth/authorize` 用 Cookie 识别用户，渲染同意页；`POST` 校验 CSRF 后 302；
//! 3. 登出 / 改密 / 封禁时撤销会话。
//!
//! 边界（§16 T-1 / T-8）：本模块**不含**任何 HTTP 语义（Cookie、状态码、重定向
//! 都在 `crates/api`），也不改动既有 service 函数签名，因此 Tauri 桌面通道无感。

use chrono::{DateTime, Duration, Utc};
use tracing::{info, warn};

use utils::oauth::{generate_csrf_token, generate_session_token, hash_secret};
use utils::snowflake::next_id;

/// 会话有效期（秒）：12 小时（可用 `SESSION_TTL_SECS` 覆盖，0 或非法值回落默认）
pub const SESSION_TTL_SECS: i64 = 12 * 3600;

/// `last_seen_at` 的最小写入间隔（秒）：避免每个请求都产生一次 UPDATE
pub const LAST_SEEN_THROTTLE_SECS: i64 = 300;

/// 已过期会话的保留期（天）：便于审计追溯，过期后由清理任务物理删除
pub const EXPIRED_RETENTION_DAYS: i64 = 7;

/// 浏览器会话（不含任何令牌明文）
#[derive(Debug, Clone)]
pub struct UserSession {
    /// `sys_user_session.id`
    pub id: i64,
    /// 会话所属用户
    pub user_id: i64,
    /// 会话绑定的租户（`/oauth/authorize` 签发授权码时沿用）
    pub tenant_id: Option<i64>,
    /// CSRF 令牌的 SHA-256（双提交 Cookie 校验用）
    pub csrf_hash: String,
    pub expires_at: DateTime<Utc>,
}

/// 新建会话的产物：`token` / `csrf_token` 明文**只返回这一次**
#[derive(Debug, Clone)]
pub struct CreatedSession {
    /// 会话令牌明文（写入 httpOnly Cookie）
    pub token: String,
    /// CSRF 令牌明文（写入非 httpOnly Cookie + 同意页隐藏域）
    pub csrf_token: String,
    pub expires_at: DateTime<Utc>,
}

/// 读取会话有效期配置
pub fn session_ttl_secs() -> i64 {
    std::env::var("SESSION_TTL_SECS")
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(SESSION_TTL_SECS)
}

/// 截断 User-Agent（按字符而非字节，避免中文截断成非法 UTF-8）
pub fn truncate_user_agent(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    Some(raw.chars().take(512).collect())
}

/// 创建会话（登录成功时调用）
///
/// `ip` 传字符串形式（如 `127.0.0.1`），由 SQL 侧 `$n::inet` 转换，
/// 避免为 sqlx 引入 `ipnetwork` 特性。
pub async fn create_session(
    user_id: i64,
    tenant_id: Option<i64>,
    ip: Option<&str>,
    user_agent: Option<&str>,
) -> Result<CreatedSession, String> {
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;

    let token = generate_session_token();
    let csrf_token = generate_csrf_token();
    let expires_at = Utc::now() + Duration::seconds(session_ttl_secs());
    let user_agent = truncate_user_agent(user_agent);

    sqlx::query(
        "INSERT INTO public.sys_user_session \
         (id, session_hash, user_id, tenant_id, csrf_hash, ip, user_agent, expires_at, last_seen_at) \
         VALUES ($1, $2, $3, $4, $5, $6::inet, $7, $8, now())",
    )
    .bind(next_id() as i64)
    .bind(&token.hash)
    .bind(user_id)
    .bind(tenant_id)
    .bind(hash_secret(&csrf_token))
    .bind(ip)
    .bind(&user_agent)
    .bind(expires_at)
    .execute(&pool)
    .await
    .map_err(|e| format!("创建浏览器会话失败: {}", e))?;

    info!(
        "浏览器会话已创建: user_id={}, tenant_id={:?}, expires_at={}",
        user_id, tenant_id, expires_at
    );

    Ok(CreatedSession {
        token: token.plaintext,
        csrf_token,
        expires_at,
    })
}

/// 校验会话令牌（未撤销且未过期），命中后顺带节流更新 `last_seen_at`
pub async fn find_session(raw_token: &str) -> Result<Option<UserSession>, String> {
    let raw_token = raw_token.trim();
    if raw_token.is_empty() {
        return Ok(None);
    }
    // 会话是安全凭据：一律读主库，避免只读副本延迟导致「刚登录却查不到会话」
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;

    let row: Option<(i64, i64, Option<i64>, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, user_id, tenant_id, csrf_hash, expires_at FROM public.sys_user_session \
         WHERE session_hash = $1 AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(hash_secret(raw_token))
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("查询浏览器会话失败: {}", e))?;

    let Some((id, user_id, tenant_id, csrf_hash, expires_at)) = row else {
        return Ok(None);
    };

    // 节流写回：同一会话 5 分钟内只更新一次（失败不影响鉴权结果）
    if let Ok(write_pool) = database::get_write_pool() {
        let _ = sqlx::query(
            "UPDATE public.sys_user_session SET last_seen_at = now() \
             WHERE id = $1 AND (last_seen_at IS NULL \
             OR last_seen_at < now() - make_interval(secs => $2))",
        )
        .bind(id)
        .bind(LAST_SEEN_THROTTLE_SECS as f64)
        .execute(&write_pool)
        .await;
    }

    Ok(Some(UserSession {
        id,
        user_id,
        tenant_id,
        csrf_hash,
        expires_at,
    }))
}

/// 重新绑定会话的 CSRF 令牌（同意页刷新 / 旧 CSRF Cookie 失配时自愈）
pub async fn rotate_csrf_token(session_id: i64, raw_csrf_token: &str) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;
    sqlx::query(
        "UPDATE public.sys_user_session SET csrf_hash = $1 \
         WHERE id = $2 AND revoked_at IS NULL",
    )
    .bind(hash_secret(raw_csrf_token))
    .bind(session_id)
    .execute(&pool)
    .await
    .map_err(|e| format!("更新会话 CSRF 令牌失败: {}", e))?;
    Ok(())
}

/// 撤销单个会话（登出）
pub async fn revoke_session(raw_token: &str) -> Result<u64, String> {
    let raw_token = raw_token.trim();
    if raw_token.is_empty() {
        return Ok(0);
    }
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;
    let affected = sqlx::query(
        "UPDATE public.sys_user_session SET revoked_at = now() \
         WHERE session_hash = $1 AND revoked_at IS NULL",
    )
    .bind(hash_secret(raw_token))
    .execute(&pool)
    .await
    .map_err(|e| format!("撤销浏览器会话失败: {}", e))?
    .rows_affected();

    if affected > 0 {
        info!("浏览器会话已撤销");
    }
    Ok(affected)
}

/// 撤销某个用户的全部会话（改密 / 封禁 / 删除用户时调用）
pub async fn revoke_all_user_sessions(user_id: i64) -> Result<u64, String> {
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;
    let affected = sqlx::query(
        "UPDATE public.sys_user_session SET revoked_at = now() \
         WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .map_err(|e| format!("撤销用户会话失败: {}", e))?
    .rows_affected();

    if affected > 0 {
        warn!(
            "用户 {} 的全部浏览器会话已撤销（共 {} 个）",
            user_id, affected
        );
    }
    Ok(affected)
}

/// 清理历史会话：过期超过保留期的行物理删除（由定时任务调用）
pub async fn cleanup_expired_sessions() -> Result<u64, String> {
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;
    let removed = sqlx::query(
        "DELETE FROM public.sys_user_session \
         WHERE expires_at < now() - make_interval(days => $1)",
    )
    .bind(EXPIRED_RETENTION_DAYS as i32)
    .execute(&pool)
    .await
    .map_err(|e| format!("清理过期会话失败: {}", e))?
    .rows_affected();

    if removed > 0 {
        info!("过期浏览器会话已清理: {}", removed);
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_is_trimmed_and_capped_by_chars() {
        assert_eq!(truncate_user_agent(None), None);
        assert_eq!(truncate_user_agent(Some("   ")), None);
        assert_eq!(
            truncate_user_agent(Some("  Mozilla/5.0  ")),
            Some("Mozilla/5.0".to_string())
        );
        // 600 个中文字符 → 截到 512 个字符（不是字节）
        let long: String = "资".repeat(600);
        let truncated = truncate_user_agent(Some(&long)).expect("应保留非空 UA");
        assert_eq!(truncated.chars().count(), 512);
        assert!(truncated.is_char_boundary(truncated.len()));
    }

    #[test]
    fn session_ttl_default_is_twelve_hours() {
        // 未设置环境变量时使用 12 小时
        assert_eq!(SESSION_TTL_SECS, 43_200);
        assert_eq!(session_ttl_secs(), SESSION_TTL_SECS);
    }
}
