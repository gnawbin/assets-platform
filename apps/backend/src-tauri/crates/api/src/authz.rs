//! HTTP 鉴权执行点（L2 功能权限）
//!
//! 设计文档 §6.3 / §6.4 / §6.7 的落地实现：
//!
//! 1. **路由 → 权限点映射**：`sys_menu.http_path` + `http_method` 与
//!    axum 的 [`MatchedPath`]（路由模板，如 `/api/assets/hardware/{id}`）格式完全一致，
//!    因此可以做「模板 ↔ 模板」等值比对，无需自写路径归一化；
//! 2. **代码侧兜底表** [`ROUTE_PERMS`]：DB 映射缺失时使用，避免上线即 403，
//!    同时让「哪些路由受哪个权限点保护」在代码里可审计；
//! 3. **判定算法** [`AuthPrincipal::allows`] + 通配规则 [`matches_scope`]；
//! 4. **失败语义**：未映射 / 权限不足都是 403（fail-closed），
//!    403 响应体新增 `scope` 字段并回填 `WWW-Authenticate`；
//! 5. **过渡开关** `AUTHZ_MODE`：`off` / `warn`（缺省，只记日志不拒绝）/ `enforce`。
//!
//! **API Key（服务身份）不受 `warn` 影响**：API Key 是 Phase 1 新增能力，
//! 没有兼容包袱，因此 scope 判定对它始终 enforce，且禁止调用
//! `system:*`（管理面，含 API Key 自身）与 `upload:*`（需要用户会话的上传流）。
//!
//! 归属边界（§16 T-1）：判定只存在于 `api` crate 的 HTTP 层，
//! `service` / `database` 不感知任何「能不能调」的语义；本模块也**不 panic**，
//! 自检失败只记日志，不影响 Tauri 启动（T-2）。

use std::{
    sync::OnceLock,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{MatchedPath, Request},
    middleware::Next,
    response::Response,
};
use dashmap::DashMap;
use tracing::{debug, error, info, warn};

use super::auth::{AuthPrincipal, PrincipalKind};
use super::response::ApiError;

/// 路由映射与权限缓存的 TTL（设计文档 §6.8：60s）
const PERMS_CACHE_TTL: Duration = Duration::from_secs(60);

/// 限流窗口长度（秒）
const RATE_LIMIT_WINDOW_SECS: u64 = 60;

/// 鉴权模式（由环境变量 `AUTHZ_MODE` 决定，缺省 `warn`，见 §16 T-7）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthzMode {
    /// 不挂任何判定（等价于改造前行为）
    Off,
    /// 只记录「本应 403」的日志，不拒绝（过渡期默认值）
    Warn,
    /// 严格执行：未映射 / 权限不足一律 403
    Enforce,
}

impl AuthzMode {
    /// 从字面量解析，非法值回落到 `Warn` 并告警（缺省即安全）
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" | "disable" | "disabled" | "none" => Self::Off,
            "enforce" | "strict" | "on" => Self::Enforce,
            "warn" | "" => Self::Warn,
            other => {
                warn!(
                    "AUTHZ_MODE={} 无法识别，按 warn（只告警不拒绝）处理",
                    other
                );
                Self::Warn
            }
        }
    }

    /// 读取当前进程的鉴权模式
    pub fn current() -> Self {
        match std::env::var("AUTHZ_MODE") {
            Ok(value) => Self::parse(&value),
            Err(_) => Self::Warn,
        }
    }

    /// 是否为强制模式
    pub fn is_enforce(self) -> bool {
        self == Self::Enforce
    }
}

/// 服务身份（API Key）**永远**不可调用的权限点前缀（设计文档 §6.6）
///
/// - `system:`：管理面（含 API Key 自身的管理接口，防自举提权）
/// - `upload:`：大文件上传需要用户会话（S3 预签名 + 会话内 commit）
pub const SERVICE_FORBIDDEN_PREFIXES: &[&str] = &["system:", "upload:"];

/// 服务身份（API Key）**不可调用**的具体路由：`(方法, 路由模板)`
///
/// 这些 handler 强制提取 `Extension<UserContext>`（写入 `created_by` / `updated_by`），
/// 而服务身份没有 `user_id`。若不拦截，请求会因缺少扩展而变成 500；
/// 按设计文档 §6.6「禁止用户态接口」应当明确 403。
///
/// **新增依赖 `UserContext` 的 handler 时请同步登记**，
/// `cargo test -p api authz` 会校验本表每项都能在 [`ROUTE_PERMS`] 中找到。
pub const SERVICE_FORBIDDEN_ROUTES: &[(&str, &str)] = &[
    ("POST", "/api/assets/hardware"),
    ("PUT", "/api/assets/hardware/{id}"),
    ("POST", "/api/assets/intangible"),
    ("PUT", "/api/assets/intangible/{id}"),
];


/// 仅需登录、不需要权限点的路由（显式声明，代码可审计）
pub const AUTH_ONLY_ROUTES: &[(&str, &str)] = &[
    ("GET", "/api/users/me"),
    ("GET", "/api/menus/user"),
];

/// 完全公开（不经过认证中间件）的路由，仅用于自检对账
pub const PUBLIC_ROUTES_DECL: &[(&str, &str)] = &[
    ("POST", "/api/auth/login"),
    ("POST", "/api/auth/register"),
    ("POST", "/api/auth/refresh"),
    ("POST", "/api/auth/logout"),
    // 存活探测（无认证、无敏感信息）
    ("GET", "/healthz"),
    // OAuth 2.1 授权端点：方案 A 在 handler 内校验 Bearer，方案 B 靠会话 Cookie（§7.8）
    ("GET", "/oauth/authorize"),
    ("POST", "/oauth/authorize"),
    // 浏览器会话查询 / 登出（只读 Cookie）
    ("GET", "/api/auth/session"),
    ("POST", "/api/auth/session/logout"),
];

/// 代码侧兜底映射：`(HTTP 方法, 路由模板, 权限点)`
///
/// 与 `sys_menu.http_path` / `http_method` 等值比对；DB 有映射时以 DB 为准
/// （运维可在权限配置页调整），DB 缺失时使用本表（设计文档 §6.3 手段 1+2）。
///
/// 新增受保护路由时**必须**在此登记，`cargo test -p api authz` 会拦截遗漏。
pub const ROUTE_PERMS: &[(&str, &str, &str)] = &[
    // ===== 资产分类 =====
    ("GET", "/api/categories", "asset:category:list"),
    ("GET", "/api/categories/parents", "asset:category:list"),
    ("POST", "/api/categories", "asset:category:add"),
    ("PUT", "/api/categories/{id}", "asset:category:edit"),
    ("DELETE", "/api/categories/{id}", "asset:category:delete"),
    // ===== 固定资产 / 无形资产 =====
    ("GET", "/api/assets/hardware", "asset:hardware:list"),
    ("POST", "/api/assets/hardware", "asset:hardware:add"),
    ("PUT", "/api/assets/hardware/{id}", "asset:hardware:edit"),
    ("DELETE", "/api/assets/hardware/{id}", "asset:hardware:delete"),
    ("GET", "/api/assets/intangible", "asset:software:list"),
    ("POST", "/api/assets/intangible", "asset:software:add"),
    ("PUT", "/api/assets/intangible/{id}", "asset:software:edit"),
    (
        "DELETE",
        "/api/assets/intangible/{id}",
        "asset:software:delete",
    ),
    // ===== 部门 =====
    ("GET", "/api/departments", "system:department:list"),
    ("POST", "/api/departments", "system:department:add"),
    ("PUT", "/api/departments/{id}", "system:department:edit"),
    ("DELETE", "/api/departments/{id}", "system:department:delete"),
    // ===== 用户 =====
    ("GET", "/api/users", "system:user:list"),
    ("POST", "/api/users", "system:user:add"),
    ("PUT", "/api/users/{id}", "system:user:edit"),
    ("DELETE", "/api/users/{id}", "system:user:delete"),
    (
        "POST",
        "/api/users/{id}/reset-password",
        "system:user:reset",
    ),
    ("GET", "/api/users/{id}/roles", "system:user:list"),
    ("POST", "/api/users/{id}/roles", "system:user:assign"),
    ("GET", "/api/users/{id}/tenants", "system:user:list"),
    // ===== 角色 / 菜单权限 =====
    ("GET", "/api/roles", "system:permission:list"),
    ("POST", "/api/roles", "system:role:add"),
    ("DELETE", "/api/roles/{id}", "system:role:delete"),
    ("GET", "/api/roles/{id}/menus", "system:permission:list"),
    ("POST", "/api/roles/{id}/menus", "system:role:assign"),
    ("GET", "/api/menus/tree", "system:permission:list"),
    // ===== 租户 =====
    ("GET", "/api/tenants", "system:tenant:list"),
    ("POST", "/api/tenants", "system:tenant:add"),
    ("PUT", "/api/tenants/{id}", "system:tenant:edit"),
    ("DELETE", "/api/tenants/{id}", "system:tenant:delete"),
    ("POST", "/api/tenants/assign", "system:tenant:assign"),
    ("POST", "/api/tenants/switch", "system:tenant:switch"),
    // ===== 注册审核 =====
    ("GET", "/api/auth/registrations", "system:registration:list"),
    (
        "POST",
        "/api/auth/registrations/{id}/approve",
        "system:registration:approve",
    ),
    (
        "POST",
        "/api/auth/registrations/{id}/reject",
        "system:registration:approve",
    ),
    // ===== 知识库 =====
    ("GET", "/api/knowledge/list", "knowledge:list"),
    ("GET", "/api/knowledge/tree", "knowledge:list"),
    ("GET", "/api/knowledge/{id}", "knowledge:list"),
    ("POST", "/api/knowledge", "knowledge:add"),
    ("POST", "/api/knowledge/node", "knowledge:add"),
    ("PUT", "/api/knowledge/{id}", "knowledge:edit"),
    ("PUT", "/api/knowledge/node/{id}", "knowledge:edit"),
    ("PUT", "/api/knowledge/node/{id}/move", "knowledge:edit"),
    ("DELETE", "/api/knowledge/{id}", "knowledge:delete"),
    ("DELETE", "/api/knowledge/node/{id}", "knowledge:delete"),
    // ===== 流程（领用 / 归还 / 调拨 / 维修 / 报废 / 采购）=====
    ("GET", "/api/process/receive", "process:receive:list"),
    ("POST", "/api/process/receive", "process:receive:add"),
    ("PUT", "/api/process/receive/{id}", "process:receive:edit"),
    (
        "DELETE",
        "/api/process/receive/{id}",
        "process:receive:delete",
    ),
    ("GET", "/api/process/return", "process:return:list"),
    ("POST", "/api/process/return", "process:return:add"),
    ("PUT", "/api/process/return/{id}", "process:return:edit"),
    ("DELETE", "/api/process/return/{id}", "process:return:delete"),
    ("GET", "/api/process/transfer", "process:transfer:list"),
    ("POST", "/api/process/transfer", "process:transfer:add"),
    ("PUT", "/api/process/transfer/{id}", "process:transfer:edit"),
    (
        "DELETE",
        "/api/process/transfer/{id}",
        "process:transfer:delete",
    ),
    ("GET", "/api/process/repair", "process:repair:list"),
    ("POST", "/api/process/repair", "process:repair:add"),
    ("PUT", "/api/process/repair/{id}", "process:repair:edit"),
    ("DELETE", "/api/process/repair/{id}", "process:repair:delete"),
    ("GET", "/api/process/scrap", "process:scrap:list"),
    ("POST", "/api/process/scrap", "process:scrap:add"),
    ("PUT", "/api/process/scrap/{id}", "process:scrap:edit"),
    ("DELETE", "/api/process/scrap/{id}", "process:scrap:delete"),
    ("GET", "/api/process/purchase", "process:purchase:list"),
    ("POST", "/api/process/purchase", "process:purchase:add"),
    ("PUT", "/api/process/purchase/{id}", "process:purchase:edit"),
    (
        "DELETE",
        "/api/process/purchase/{id}",
        "process:purchase:delete",
    ),
    // ===== 运维采集 =====
    ("GET", "/api/ops/providers", "ops:connection:list"),
    ("GET", "/api/ops/connections", "ops:connection:list"),
    ("GET", "/api/ops/connections/{id}", "ops:connection:list"),
    ("POST", "/api/ops/connections", "ops:connection:create"),
    ("PUT", "/api/ops/connections/{id}", "ops:connection:update"),
    (
        "DELETE",
        "/api/ops/connections/{id}",
        "ops:connection:delete",
    ),
    ("POST", "/api/ops/connections/test", "ops:connection:test"),
    (
        "POST",
        "/api/ops/connections/{id}/test",
        "ops:connection:test",
    ),
    (
        "POST",
        "/api/ops/connections/{id}/collect",
        "ops:connection:collect",
    ),
    ("GET", "/api/ops/links", "ops:resource:list"),
    ("POST", "/api/ops/links/{linkId}/bind", "ops:link:bind"),
    // ===== 大文件上传 =====
    ("POST", "/api/upload/init", "upload:file:add"),
    ("POST", "/api/upload/{id}/start", "upload:file:add"),
    ("POST", "/api/upload/{id}/chunk", "upload:file:add"),
    ("POST", "/api/upload/{id}/complete", "upload:file:add"),
    ("POST", "/api/upload/{id}/commit", "upload:file:add"),
    ("GET", "/api/upload/{id}/progress", "upload:file:list"),
    ("DELETE", "/api/upload/{id}", "upload:file:delete"),
    // ===== 智能问答（SSE）=====
    ("POST", "/api/chat/stream", "chat:completion:add"),
    // ===== Agent Skill =====
    ("GET", "/api/skills", "system:skill:list"),
    ("GET", "/api/skills/count", "system:skill:list"),
    ("GET", "/api/skills/{skill_id}", "system:skill:list"),
    ("POST", "/api/skills/execute", "system:skill:execute"),
    ("POST", "/api/skills/register", "system:skill:add"),
    ("DELETE", "/api/skills/{skill_id}", "system:skill:delete"),
    // ===== API Key 管理（Phase 1 新增，禁止 API Key 管理 API Key）=====
    ("GET", "/api/api-keys", "system:apikey:list"),
    ("POST", "/api/api-keys", "system:apikey:add"),
    ("POST", "/api/api-keys/{id}/rotate", "system:apikey:edit"),
    ("DELETE", "/api/api-keys/{id}", "system:apikey:delete"),
    // ===== OAuth 客户端管理（Phase 2 新增，禁止 API Key / OAuth 令牌管理）=====
    ("GET", "/api/oauth-clients", "system:oauthclient:list"),
    ("POST", "/api/oauth-clients", "system:oauthclient:add"),
    ("PUT", "/api/oauth-clients/{id}", "system:oauthclient:edit"),
    ("DELETE", "/api/oauth-clients/{id}", "system:oauthclient:delete"),
];

/// 路由 → 权限点缓存：`(METHOD, 路由模板)` → (写入时刻, 权限点集合)
fn perms_cache() -> &'static DashMap<(String, String), (Instant, Vec<String>)> {
    static CACHE: OnceLock<DashMap<(String, String), (Instant, Vec<String>)>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

/// 限流计数：`api_key_id` → (窗口序号, 本窗口已用次数)
fn rate_limit_counters() -> &'static DashMap<i64, (u64, u32)> {
    static COUNTERS: OnceLock<DashMap<i64, (u64, u32)>> = OnceLock::new();
    COUNTERS.get_or_init(DashMap::new)
}

/// 清空路由 → 权限点缓存（管理端保存权限配置后调用）
pub fn clear_perms_cache() {
    perms_cache().clear();
}

/// 代码侧兜底映射查询
pub fn static_perms_for(method: &str, path: &str) -> Option<&'static str> {
    ROUTE_PERMS
        .iter()
        .find(|(route_method, route_path, _)| {
            *route_method == method && *route_path == path
        })
        .map(|(_, _, perms)| *perms)
}

/// 判断某路由是否「仅需登录」
pub fn is_auth_only_route(method: &str, path: &str) -> bool {
    AUTH_ONLY_ROUTES
        .iter()
        .any(|(route_method, route_path)| *route_method == method && *route_path == path)
}

/// 计算路由所需的权限点
///
/// 返回 `None` 表示「未登记」（fail-closed 场景）；`Some(vec![])` 表示「仅需登录」。
///
/// 取值顺序（设计文档 §6.3）：
/// 1. 显式豁免表 [`AUTH_ONLY_ROUTES`]
/// 2. DB：`sys_menu.http_path` + `http_method`（运维可在权限配置页调整），60s 缓存
/// 3. 代码兜底表 [`ROUTE_PERMS`]
pub async fn required_perms(method: &str, path: &str) -> Option<Vec<String>> {
    if is_auth_only_route(method, path) {
        return Some(Vec::new());
    }

    let cache_key = (method.to_string(), path.to_string());
    if let Some(entry) = perms_cache().get(&cache_key) {
        if entry.0.elapsed() < PERMS_CACHE_TTL {
            return if entry.1.is_empty() {
                None
            } else {
                Some(entry.1.clone())
            };
        }
    }
    perms_cache().remove(&cache_key);

    // DB 优先：同一条路由可能登记多个权限点（AND 关系）
    let from_db = load_db_perms(method, path).await;
    if let Some(perms) = from_db {
        if !perms.is_empty() {
            perms_cache().insert(cache_key, (Instant::now(), perms.clone()));
            return Some(perms);
        }
    }

    // 代码兜底
    let perms = static_perms_for(method, path).map(|perms| vec![perms.to_string()]);
    perms_cache().insert(cache_key, (Instant::now(), perms.clone().unwrap_or_default()));
    perms
}

/// 从 `sys_menu` 读取路由 → 权限点映射（失败返回 `None`，交由代码兜底）
async fn load_db_perms(method: &str, path: &str) -> Option<Vec<String>> {
    let Ok(pool) = database::get_read_pool() else {
        return None;
    };

    let result: Result<Vec<String>, sqlx::Error> = sqlx::query_scalar(
        r#"
        SELECT perms
        FROM public.sys_menu
        WHERE http_path = $1
          AND http_method = $2
          AND perms IS NOT NULL
          AND (deleted IS NULL OR deleted = 0)
        "#,
    )
    .bind(path)
    .bind(method)
    .fetch_all(&pool)
    .await;

    match result {
        Ok(perms) => Some(perms),
        Err(e) => {
            warn!(
                "读取路由权限映射失败（改用代码兜底表）: {} {} -> {}",
                method, path, e
            );
            None
        }
    }
}

/// 通配规则：`*` 与尾部 `:*`，精确匹配优先
///
/// ```text
/// matches_scope("asset:*", "asset:hardware:list") == true
/// matches_scope("*", "system:user:add")          == true
/// matches_scope("asset:hardware:list", "asset:hardware:list") == true
/// ```
pub fn matches_scope(have: &str, need: &str) -> bool {
    if have == "*" || have == need {
        return true;
    }
    if let Some(prefix) = have.strip_suffix(":*") {
        return need.starts_with(prefix) && need.as_bytes().get(prefix.len()) == Some(&b':');
    }
    false
}

/// 判断某权限点是否属于「服务身份禁止」的范围
pub fn is_service_forbidden(perms: &str) -> bool {
    SERVICE_FORBIDDEN_PREFIXES
        .iter()
        .any(|prefix| perms.starts_with(prefix))
}

/// 判断某路由是否属于「必须带用户上下文」的接口（服务身份一律 403）
pub fn is_service_forbidden_route(method: &str, path: &str) -> bool {
    SERVICE_FORBIDDEN_ROUTES
        .iter()
        .any(|(m, p)| m.eq_ignore_ascii_case(method) && *p == path)
}

/// 限流：固定窗口 60s，超限返回 false
///
/// 单实例部署，进程内计数即可（设计文档 §6.8 不引入 Redis）。
pub fn check_rate_limit(api_key_id: i64, limit_per_min: i32) -> bool {
    if limit_per_min <= 0 {
        return true;
    }
    let now_window = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() / RATE_LIMIT_WINDOW_SECS)
        .unwrap_or(0);

    let mut entry = rate_limit_counters()
        .entry(api_key_id)
        .or_insert((now_window, 0));
    if entry.0 != now_window {
        *entry = (now_window, 0);
    }
    entry.1 = entry.1.saturating_add(1);
    entry.1 <= limit_per_min as u32
}

/// 鉴权中间件（挂在 `auth_middleware` **内层**，因此总能拿到 [`AuthPrincipal`]）
///
/// 失败语义（设计文档 §6.7）：
/// | 场景 | 状态码 | 响应体 |
/// |---|---|---|
/// | 未解析到路由模板（如 404） | 交给后续层 | - |
/// | 未认证（缺 principal） | 401 | `{code:401,message}` |
/// | 路由未登记权限点 | 403 | `{code:403,message:"该接口未配置访问权限"}` |
/// | 权限不足 | 403 | `{code:403,message:"权限不足",scope:[...]}` |
/// | 超限 | 429 | `{code:429,message:"请求过于频繁"}` |
pub async fn authz_middleware(req: Request, next: Next) -> Result<Response, ApiError> {
    let mode = AuthzMode::current();
    if mode == AuthzMode::Off {
        return Ok(next.run(req).await);
    }

    // 未匹配到路由模板（404 / 405）：交给后续层，避免把 404 变成 403
    let Some(matched) = req
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| matched.as_str().to_string())
    else {
        return Ok(next.run(req).await);
    };

    let method = req.method().as_str().to_uppercase();

    let Some(principal) = req.extensions().get::<AuthPrincipal>().cloned() else {
        // 正常情况下 auth_middleware 一定注入；缺失说明中间件顺序被改坏
        error!("鉴权层缺少 AuthPrincipal（检查中间件挂载顺序）: {} {}", method, matched);
        return Err(ApiError::unauthorized("未认证"));
    };

    // API Key（服务身份）与 OAuth 令牌都不存在兼容包袱，判定始终 enforce；
    // 普通用户 JWT 在 warn 模式下只告警不拒绝（设计文档 §16 T-7）。
    let enforce = mode.is_enforce() || principal.kind != PrincipalKind::User;

    let Some(required) = required_perms(&method, &matched).await else {
        if enforce {
            error!(
                "路由未登记权限点（fail-closed）: {} {}",
                method, matched
            );
            return Err(ApiError::forbidden("该接口未配置访问权限"));
        }
        warn!(
            "路由未登记权限点，AUTHZ_MODE=warn 放行: {} {}",
            method, matched
        );
        return Ok(next.run(req).await);
    };

    if principal.kind != PrincipalKind::User {
        // 服务身份（API Key）与 OAuth 令牌都不得触碰管理面 / 上传流
        // （§6.6 防提权 + 防自举）；两者都没有 `UserContext` / `Claims`，
        // 因此需要用户归属的写接口与「仅需登录」的用户态接口一律 403。
        let label = if principal.kind == PrincipalKind::Service {
            "API Key"
        } else {
            "OAuth 访问令牌"
        };

        if required.iter().any(|perms| is_service_forbidden(perms)) {
            warn!(
                subject = %principal.subject,
                route = %matched,
                "{} 调用了管理类接口，拒绝",
                label
            );
            return Err(ApiError::forbidden_with_scope(&required));
        }
        if is_service_forbidden_route(&method, &matched) {
            warn!(
                subject = %principal.subject,
                route = %matched,
                "{} 调用了需要用户归属的写接口，拒绝",
                label
            );
            return Err(ApiError::forbidden(format!(
                "该接口需要用户身份，{} 不可调用",
                label
            )));
        }
        // 需要 claims.sub 的用户态接口（仅需登录）对非用户主体一律 403
        if required.is_empty() {
            warn!(
                subject = %principal.subject,
                route = %matched,
                "{} 调用了用户态接口，拒绝",
                label
            );
            return Err(ApiError::forbidden(format!(
                "该接口需要用户身份，{} 不可调用",
                label
            )));
        }

        // 速率限制只作用于 API Key（OAuth 令牌有独立 TTL 与客户端级配额）
        if principal.kind == PrincipalKind::Service {
            let limit = principal.rate_limit_per_min.unwrap_or(600);
            if let Some(api_key_id) = principal.api_key_id {
                if !check_rate_limit(api_key_id, limit) {
                    warn!(
                        subject = %principal.subject,
                        route = %matched,
                        limit,
                        "API Key 超出速率限制"
                    );
                    return Err(ApiError::too_many_requests("请求过于频繁"));
                }
            }
        }
    }

    if principal.allows(&required) {
        return Ok(next.run(req).await);
    }

    if enforce {
        warn!(
            subject = %principal.subject,
            kind = ?principal.kind,
            route = %matched,
            required = ?required,
            "鉴权失败：权限不足"
        );
        return Err(ApiError::forbidden_with_scope(&required));
    }

    warn!(
        subject = %principal.subject,
        route = %matched,
        required = ?required,
        "鉴权失败，AUTHZ_MODE=warn 放行（配置 AUTHZ_MODE=enforce 启用强制）"
    );
    Ok(next.run(req).await)
}


/// 路由覆盖率自检报告（**只读、不 panic**，设计文档 §6.3 手段 3）
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RouteCoverageReport {
    /// 代码兜底表登记的路由数
    pub code_routes: usize,
    /// DB 中登记了 `http_path` 的路由数
    pub db_routes: usize,
    /// 仅需登录的路由数
    pub auth_only: usize,
    /// 公开路由数
    pub public: usize,
    /// 仅由代码兜底表覆盖（DB 未登记）的路由
    pub code_only: Vec<String>,
    /// DB 登记但代码侧没有对应路由（历史残留 / 已删除接口）
    pub db_only: Vec<String>,
}

impl RouteCoverageReport {
    /// 未映射路由数（DB 残留，需人工确认）
    pub fn stale_db_routes(&self) -> usize {
        self.db_only.len()
    }

    /// 打印自检结果（启动时调用，失败只告警不中断）
    pub fn log(&self, mode: AuthzMode) {
        info!(
            "鉴权自检：代码兜底表 {} 条 / DB 登记 {} 条 / 仅需登录 {} 条 / 公开 {} 条 / AUTHZ_MODE={:?}",
            self.code_routes, self.db_routes, self.auth_only, self.public, mode
        );
        if !self.code_only.is_empty() {
            info!(
                "鉴权自检：{} 条路由仅由代码兜底表 ROUTE_PERMS 覆盖（DB 未登记 http_path，可后续补录）：{:?}",
                self.code_only.len(),
                self.code_only
            );
        }
        if !self.db_only.is_empty() {
            warn!(
                "鉴权自检：DB 中 {} 条 http_path 未匹配到代码路由（历史残留，建议清理）：{:?}",
                self.db_only.len(),
                self.db_only
            );
        }
        if mode == AuthzMode::Warn {
            warn!(
                "AUTHZ_MODE=warn：权限不足只记录日志不拒绝。确认权限配置无误后改为 AUTHZ_MODE=enforce 启用强制鉴权"
            );
        }
    }
}

/// 执行路由覆盖率自检
///
/// 对比「代码兜底表」与「DB 映射」，返回报告供调用方打日志。
/// DB 不可用时只返回代码侧统计（`db_routes = 0`），**不报错、不 panic**（T-2）。
pub async fn audit_route_coverage() -> RouteCoverageReport {
    let mut report = RouteCoverageReport {
        code_routes: ROUTE_PERMS.len(),
        auth_only: AUTH_ONLY_ROUTES.len(),
        public: PUBLIC_ROUTES_DECL.len(),
        ..Default::default()
    };

    let db_rows = load_db_route_table().await;
    report.db_routes = db_rows.len();

    for (method, path) in &db_rows {
        if static_perms_for(method, path).is_none() && !is_auth_only_route(method, path) {
            report.db_only.push(format!("{} {}", method, path));
        }
    }

    for (method, path, _) in ROUTE_PERMS {
        let registered = db_rows
            .iter()
            .any(|(db_method, db_path)| db_method == method && db_path == path);
        if !registered {
            report.code_only.push(format!("{} {}", method, path));
        }
    }

    report
}

/// 读取 DB 中登记了权限点的路由表
async fn load_db_route_table() -> Vec<(String, String)> {
    let Ok(pool) = database::get_read_pool() else {
        debug!("鉴权自检：数据库连接池不可用，跳过 DB 映射比对");
        return Vec::new();
    };

    let result: Result<Vec<(String, String)>, sqlx::Error> = sqlx::query_as(
        r#"
        SELECT DISTINCT http_method, http_path
        FROM public.sys_menu
        WHERE http_path IS NOT NULL
          AND http_method IS NOT NULL
          AND (deleted IS NULL OR deleted = 0)
        "#,
    )
    .fetch_all(&pool)
    .await;

    match result {
        Ok(rows) => rows,
        Err(e) => {
            debug!("鉴权自检：读取 sys_menu 路由映射失败，跳过比对: {}", e);
            Vec::new()
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthPrincipal;

    fn service_principal(scopes: &[&str]) -> AuthPrincipal {
        AuthPrincipal {
            kind: PrincipalKind::Service,
            subject: "k:1".to_string(),
            user_id: None,
            api_key_id: Some(1),
            tenant_id: None,
            schema_name: "public".to_string(),
            scopes: scopes.iter().map(|scope| scope.to_string()).collect(),
            is_super_admin: false,
            rate_limit_per_min: Some(600),
        }
    }

    fn user_principal(scopes: &[&str], is_super_admin: bool) -> AuthPrincipal {
        AuthPrincipal {
            kind: PrincipalKind::User,
            subject: "u:1".to_string(),
            user_id: Some(1),
            api_key_id: None,
            tenant_id: Some(1),
            schema_name: "tenant_a".to_string(),
            scopes: scopes.iter().map(|scope| scope.to_string()).collect(),
            is_super_admin,
            rate_limit_per_min: None,
        }
    }

    // ===== 判定算法（§6.4）=====

    #[test]
    fn wildcard_matching_rules() {
        assert!(matches_scope("asset:hardware:list", "asset:hardware:list"));
        assert!(matches_scope("asset:*", "asset:hardware:list"));
        assert!(matches_scope("*", "system:user:add"));
        // 通配必须落在冒号边界上，不能把 asset:hard 当成 asset:hardware 的前缀
        assert!(!matches_scope("asset:hard:*", "asset:hardware:list"));
        assert!(!matches_scope("asset", "asset:hardware:list"));
        assert!(!matches_scope("asset:*", "assets:hardware:list"));
    }

    #[test]
    fn super_admin_bypasses_everything() {
        let principal = user_principal(&[], true);
        assert!(principal.allows(&["asset:hardware:list".to_string()]));
        assert!(principal.allows(&["system:user:add".to_string()]));
    }

    #[test]
    fn normal_user_needs_every_required_perm() {
        let principal = user_principal(&["asset:hardware:list"], false);
        assert!(principal.allows(&["asset:hardware:list".to_string()]));
        assert!(!principal.allows(&[
            "asset:hardware:list".to_string(),
            "asset:hardware:delete".to_string()
        ]));
        // 仅需登录（required 为空）恒通过
        assert!(principal.allows(&[]));
    }

    #[test]
    fn api_key_scopes_are_matched_by_wildcard() {
        let principal = service_principal(&["asset:*"]);
        assert!(principal.allows(&["asset:hardware:list".to_string()]));
        assert!(!principal.allows(&["process:receive:add".to_string()]));
    }


    // ===== 兜底映射表完整性（§6.3 手段 2）=====

    #[test]
    fn route_perms_has_no_duplicate_keys() {
        let mut seen = std::collections::HashSet::new();
        for (method, path, _) in ROUTE_PERMS {
            assert!(
                seen.insert((*method, *path)),
                "重复登记的路由: {} {}",
                method,
                path
            );
        }
    }

    #[test]
    fn route_perms_entries_are_well_formed() {
        for (method, path, perms) in ROUTE_PERMS {
            assert!(
                matches!(*method, "GET" | "POST" | "PUT" | "DELETE" | "PATCH"),
                "非法 HTTP 方法: {} {}",
                method,
                path
            );
            assert!(
                path.starts_with("/api/"),
                "路由必须位于 /api/ 下: {} {}",
                method,
                path
            );
            let segments: Vec<&str> = perms.split(':').collect();
            assert!(
                segments.len() == 2 || segments.len() == 3,
                "权限点必须是 模块:资源[:动作]: {}",
                perms
            );
            assert!(
                perms
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b':'),
                "权限点只允许小写字母、数字与冒号: {}",
                perms
            );
        }
    }

    #[test]
    fn auth_only_and_public_routes_are_not_in_perm_table() {
        for (method, path) in AUTH_ONLY_ROUTES {
            assert!(
                static_perms_for(method, path).is_none(),
                "仅需登录的路由不应再登记权限点: {} {}",
                method,
                path
            );
            assert!(is_auth_only_route(method, path));
        }
        for (method, path) in PUBLIC_ROUTES_DECL {
            assert!(
                static_perms_for(method, path).is_none(),
                "公开路由不应登记权限点: {} {}",
                method,
                path
            );
        }
    }

    #[test]
    fn known_routes_have_expected_perms() {
        assert_eq!(
            static_perms_for("GET", "/api/assets/hardware"),
            Some("asset:hardware:list")
        );
        assert_eq!(
            static_perms_for("POST", "/api/chat/stream"),
            Some("chat:completion:add")
        );
        assert_eq!(
            static_perms_for("POST", "/api/upload/init"),
            Some("upload:file:add")
        );
        assert_eq!(
            static_perms_for("GET", "/api/api-keys"),
            Some("system:apikey:list")
        );
        assert_eq!(static_perms_for("POST", "/api/auth/login"), None);
    }

    // ===== 服务身份（API Key）约束（§6.6）=====

    #[test]
    fn service_principal_cannot_touch_management_surface() {
        assert!(is_service_forbidden("system:apikey:list"));
        assert!(is_service_forbidden("upload:file:add"));
        assert!(!is_service_forbidden("asset:hardware:list"));
        assert!(!is_service_forbidden("chat:completion:add"));
    }

    #[test]
    fn service_forbidden_routes_are_registered_in_route_perms() {
        // 表内每条必须是真实登记的受保护路由，否则 403 永远命中不到
        for (method, path) in SERVICE_FORBIDDEN_ROUTES {
            assert!(
                ROUTE_PERMS
                    .iter()
                    .any(|(m, p, _)| m == method && p == path),
                "SERVICE_FORBIDDEN_ROUTES 中的路由未在 ROUTE_PERMS 登记: {} {}",
                method,
                path
            );
        }
        assert!(is_service_forbidden_route("POST", "/api/assets/hardware"));
        assert!(is_service_forbidden_route("put", "/api/assets/hardware/{id}"));
        assert!(!is_service_forbidden_route("GET", "/api/assets/hardware"));
        assert!(!is_service_forbidden_route("POST", "/api/ops/links/{id}/bind"));
    }

    #[test]
    fn all_api_key_routes_are_system_scoped() {
        // 管理接口必须落在 system: 前缀下，才能被 SERVICE_FORBIDDEN_PREFIXES 拦住
        for (method, path, perms) in ROUTE_PERMS {
            if path.starts_with("/api/api-keys") {
                assert!(
                    is_service_forbidden(perms),
                    "API Key 管理接口必须是 system: 权限点: {} {}",
                    method,
                    path
                );
            }
        }
    }


    // ===== 模式解析（§16 T-7 缺省即安全）=====

    #[test]
    fn authz_mode_parsing_defaults_to_warn() {
        assert_eq!(AuthzMode::parse(""), AuthzMode::Warn);
        assert_eq!(AuthzMode::parse("warn"), AuthzMode::Warn);
        assert_eq!(AuthzMode::parse("enforce"), AuthzMode::Enforce);
        assert_eq!(AuthzMode::parse(" OFF "), AuthzMode::Off);
        // 无法识别时不误判为 enforce
        assert_eq!(AuthzMode::parse("yes"), AuthzMode::Warn);
        assert!(!AuthzMode::Warn.is_enforce());
    }

    // ===== 限流 ====

    #[test]
    fn rate_limit_counts_until_limit() {
        let key_id = 9_000_001;
        for _ in 0..3 {
            assert!(check_rate_limit(key_id, 3));
        }
        assert!(!check_rate_limit(key_id, 3), "超过窗口上限应拒绝");
        // limit <= 0 视为不限流
        assert!(check_rate_limit(key_id, 0));
    }

    // ===== 自检报告 ====

    #[test]
    fn coverage_report_counts_stale_routes() {
        let report = RouteCoverageReport {
            code_routes: ROUTE_PERMS.len(),
            db_routes: 38,
            auth_only: AUTH_ONLY_ROUTES.len(),
            public: PUBLIC_ROUTES_DECL.len(),
            code_only: vec!["GET /api/skills".to_string()],
            db_only: vec![
                "PUT /api/process/receive/{id}/approve".to_string(),
                "PUT /api/process/return/{id}/confirm".to_string(),
            ],
        };
        assert_eq!(report.stale_db_routes(), 2);
        assert!(report.code_routes > 100, "全量路由应不少于 100 条");
    }

    // ===== OAuth 2.1（Phase 2）=====

    #[test]
    fn oauth_routes_are_registered() {
        // 授权端点是**完全公开**路由（方案 A 在 handler 内校验 Bearer，方案 B 靠会话 Cookie），
        // 既不登记权限点，也不算「仅需登录」路由（§7.8）
        assert!(!is_auth_only_route("GET", "/oauth/authorize"));
        assert!(!is_auth_only_route("POST", "/oauth/authorize"));
        assert!(PUBLIC_ROUTES_DECL.contains(&("GET", "/oauth/authorize")));
        assert!(PUBLIC_ROUTES_DECL.contains(&("POST", "/oauth/authorize")));
        assert_eq!(static_perms_for("GET", "/oauth/authorize"), None);
        // 浏览器会话端点同样公开
        assert!(PUBLIC_ROUTES_DECL.contains(&("GET", "/api/auth/session")));
        assert!(PUBLIC_ROUTES_DECL.contains(&("POST", "/api/auth/session/logout")));
        // 健康检查公开
        assert!(PUBLIC_ROUTES_DECL.contains(&("GET", "/healthz")));

        // 客户端管理接口必须有权限点，且落在 system: 前缀（API Key / OAuth 令牌不可调用）
        assert_eq!(
            static_perms_for("GET", "/api/oauth-clients"),
            Some("system:oauthclient:list")
        );
        assert_eq!(
            static_perms_for("POST", "/api/oauth-clients"),
            Some("system:oauthclient:add")
        );
        assert_eq!(
            static_perms_for("PUT", "/api/oauth-clients/{id}"),
            Some("system:oauthclient:edit")
        );
        assert_eq!(
            static_perms_for("DELETE", "/api/oauth-clients/{id}"),
            Some("system:oauthclient:delete")
        );

        for (method, path, perms) in ROUTE_PERMS {
            if path.starts_with("/api/oauth-clients") {
                assert!(
                    is_service_forbidden(perms),
                    "OAuth 客户端管理接口必须是 system: 权限点: {} {}",
                    method,
                    path
                );
            }
        }
    }

    #[test]
    fn oauth_principal_is_scoped_and_never_super_admin() {
        let principal = AuthPrincipal {
            kind: PrincipalKind::OAuth,
            subject: "u:1@client:oc_test".to_string(),
            user_id: Some(1),
            api_key_id: None,
            tenant_id: Some(1),
            schema_name: "tenant_a".to_string(),
            scopes: vec!["asset:hardware:list".to_string()],
            is_super_admin: false,
            rate_limit_per_min: None,
        };

        assert_ne!(principal.kind, PrincipalKind::User);
        assert!(!principal.is_super_admin);
        assert!(principal.allows(&["asset:hardware:list".to_string()]));
        assert!(!principal.allows(&["asset:hardware:delete".to_string()]));
        // 管理面权限点对任何非用户主体都应被前缀规则拦住
        assert!(is_service_forbidden("system:oauthclient:list"));
        assert!(!is_service_forbidden("asset:hardware:list"));
    }
}

