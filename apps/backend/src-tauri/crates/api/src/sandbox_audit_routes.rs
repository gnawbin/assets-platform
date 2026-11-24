//! 沙箱审计内部接收端点（上游文档 §6.8 通道 2）
//!
//! - **不**挂 `protected_routes`：服务身份没有 `UserContext`，会被 `authz` 一律 403；
//! - 只挂 `auth_middleware`（支持 API Key），handler 内校验
//!   `PrincipalKind::Service` + `sandbox:audit` scope；
//! - 批量写入 `public.sys_oper_log`，单条失败只记 WARN（不反压沙箱主链路）。

use std::sync::Arc;

use axum::extract::State;
use axum::{Extension, Json};
use serde::Serialize;
use serde_json::Value;

use service::sandbox_audit;

use super::auth::{AuthPrincipal, PrincipalKind};
use super::response::{ApiError, ApiResponse};

/// 推送结果
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AuditIngestResult {
    /// 收到的条数
    pub received: usize,
    /// 实际写入条数
    pub inserted: usize,
    /// 被去重跳过条数
    pub deduped: usize,
    /// 写库失败条数
    pub failed: usize,
    /// 超出单批上限被丢弃条数
    pub dropped: usize,
}

/// 仅服务身份 + `sandbox:audit` scope 可推送
fn ensure_service_audit(principal: &AuthPrincipal) -> Result<(), ApiError> {
    if principal.kind != PrincipalKind::Service {
        return Err(ApiError::forbidden("仅服务身份可推送沙箱审计事件"));
    }
    let required = vec!["sandbox:audit".to_string()];
    if !principal.allows(&required) {
        return Err(ApiError::forbidden_with_scope(&required));
    }
    Ok(())
}

/// 接收 OCSF 事件（单条或 `{ "events": [...] }` 批量）
#[utoipa::path(
    post,
    path = "/internal/sandbox/audit",
    tag = "沙箱",
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "写入结果", body = ApiResponse<AuditIngestResult>),
        (status = 401, description = "未认证", body = ApiError),
        (status = 403, description = "非服务身份或缺少 sandbox:audit scope", body = ApiError),
    )
)]
pub async fn ingest(
    State(pool): State<Arc<sqlx::PgPool>>,
    Extension(principal): Extension<AuthPrincipal>,
    Json(payload): Json<Value>,
) -> Result<Json<ApiResponse<AuditIngestResult>>, ApiError> {
    ensure_service_audit(&principal)?;

    let events = sandbox_audit::parse_batch(&payload);
    let outcome = sandbox_audit::record(pool.as_ref(), &events).await;

    Ok(Json(ApiResponse::success(AuditIngestResult {
        received: events.len(),
        inserted: outcome.inserted,
        deduped: outcome.deduped,
        failed: outcome.failed,
        dropped: outcome.dropped,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(kind: PrincipalKind, scopes: &[&str]) -> AuthPrincipal {
        AuthPrincipal {
            kind,
            subject: "k:1".to_string(),
            user_id: None,
            api_key_id: Some(1),
            tenant_id: Some(1),
            schema_name: "tenant_a".to_string(),
            scopes: scopes.iter().map(|scope| scope.to_string()).collect(),
            is_super_admin: false,
            rate_limit_per_min: Some(600),
        }
    }

    #[test]
    fn rejects_user_identity() {
        let error = ensure_service_audit(&principal(PrincipalKind::User, &["sandbox:audit"]))
            .expect_err("用户身份必须拒绝");
        assert_eq!(error.code, 403);
    }

    #[test]
    fn rejects_missing_scope() {
        let error = ensure_service_audit(&principal(PrincipalKind::Service, &["sandbox:llm"]))
            .expect_err("缺少 scope 必须拒绝");
        assert_eq!(error.code, 403);
        assert!(error.scope.as_deref().is_some_and(|scopes| scopes == ["sandbox:audit".to_string()]));
    }

    #[test]
    fn accepts_service_with_scope() {
        assert!(ensure_service_audit(&principal(PrincipalKind::Service, &["sandbox:audit"])).is_ok());
    }
}