//! 健康检查端点（设计文档 §16 T-7）
//!
//! `GET /healthz` 是**无需认证**的存活探测，供容器编排 / 负载均衡使用：
//!
//! - 进程活着且数据库可 `SELECT 1` → `200 {"status":"ok","db":"up"}`
//! - 数据库不可达 → `503 {"status":"degraded","db":"down"}`
//!
//! 刻意不返回任何配置细节（不泄露版本号以外的信息），也不触发鉴权逻辑。

use std::time::Duration;

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde_json::json;

/// 数据库探测超时（避免健康检查被慢查询拖死）
const DB_PING_TIMEOUT: Duration = Duration::from_secs(2);

/// 探测数据库连通性
async fn db_ping() -> bool {
    let Ok(pool) = database::get_read_pool() else {
        return false;
    };
    matches!(
        tokio::time::timeout(DB_PING_TIMEOUT, sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&pool)).await,
        Ok(Ok(1))
    )
}

/// 是否处于「明文 HTTP 运行 OAuth 授权服务器」的状态（§8.2 补偿控制 5）
///
/// `true` 时 `/healthz` 会带 `insecure_transport: true`，供巡检发现非合规部署。
pub fn insecure_transport() -> bool {
    let enabled = matches!(
        std::env::var("OAUTH_ENABLED")
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("on")
    );
    if !enabled {
        return false;
    }
    let issuer_is_http = std::env::var("OAUTH_ISSUER")
        .map(|issuer| issuer.trim().to_ascii_lowercase().starts_with("http://"))
        .unwrap_or(true);
    issuer_is_http
}

/// `GET /healthz`
pub async fn healthz() -> impl IntoResponse {
    let db_ok = db_ping().await;
    let status = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    let body = Json(json!({
        "status": if db_ok { "ok" } else { "degraded" },
        "db": if db_ok { "up" } else { "down" },
        "service": "assets-platform-api",
        // §8.2 补偿控制 5：明文跑授权服务器是可巡检的显式状态
        "insecure_transport": insecure_transport(),
    }));
    (status, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn healthz_reports_degraded_when_pool_missing() {
        // 单测环境没有初始化连接池 → 必须返回 503 而不是 panic
        let response = healthz().await.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn insecure_transport_only_when_oauth_enabled() {
        // 未启用 OAuth 时，无论 issuer 是什么都不算「明文跑授权服务器」
        if std::env::var("OAUTH_ENABLED").is_err() {
            assert!(!insecure_transport());
        }
    }
}
