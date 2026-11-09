//! OpenAPI / Swagger UI 文档 crate
//!
//! 职责只有两件事：
//! 1. 汇总 `crates/api` 各 `*_routes.rs` 上的 `#[utoipa::path]` / `ToSchema` 标注，
//!    生成 OpenAPI 3.1 文档（[`ApiDoc`]）；
//! 2. 以 axum 路由的形式提供 Swagger UI 页面与规范 JSON（[`docs_router`]）。
//!
//! 规范与实现放在同一个位置（handler 与 DTO 的标注就写在定义处），本 crate 不做二次
//! 声明，避免「文档一套、实现一套」的漂移。
//!
//! 依赖方向：`apidoc → api → service / database`。`api` 不反向依赖本 crate，
//! 文档路由由调用方（`apps/backend/src-tauri/src/lib.rs`）注入
//! [`api::start_http_server_with`]，因此不会出现循环依赖。

pub mod openapi;

use axum::Router;
use utoipa::OpenApi;
use utoipa_swagger_ui::{Config, SwaggerUi};

pub use self::openapi::ApiDoc;

/// Swagger UI 页面路径
pub const SWAGGER_UI_PATH: &str = "/swagger-ui";

/// OpenAPI 规范 JSON 路径
pub const OPENAPI_JSON_PATH: &str = "/api-docs/openapi.json";

/// 生成 OpenAPI 文档对象
pub fn spec() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

/// 文档路由是否开启（`API_DOCS_ENABLED=false` / `0` 可关闭，默认开启）
///
/// Swagger UI 会把全部接口结构与调试入口暴露给任何能访问该端口的人，
/// 生产环境如需收敛可直接关闭。
pub fn docs_enabled() -> bool {
    match std::env::var("API_DOCS_ENABLED") {
        Ok(value) => !matches!(value.trim(), "false" | "FALSE" | "0"),
        Err(_) => true,
    }
}

/// 构建文档路由（Swagger UI + OpenAPI JSON）
///
/// 由调用方通过 [`api::start_http_server_with`] 注入，挂载在认证中间件之外：
/// `/swagger-ui` 页面、`/api-docs/openapi.json` 规范。
///
/// 页面已开启 `persist_authorization`，在右上角 Authorize 填入
/// `/api/auth/login` 拿到的 JWT 后，跨刷新仍会保留（存于浏览器 localStorage）。
pub fn docs_router() -> Router {
    Router::new().merge(
        SwaggerUi::new(SWAGGER_UI_PATH)
            .url(OPENAPI_JSON_PATH, ApiDoc::openapi())
            .config(Config::default().persist_authorization(true)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 规范必须能被序列化，且已登记接口与安全方案
    #[test]
    fn spec_is_serializable_and_documented() {
        let spec = spec();
        let json = serde_json::to_value(&spec).expect("OpenAPI 规范应可序列化为 JSON");

        let paths = json
            .get("paths")
            .and_then(serde_json::Value::as_object)
            .expect("paths 应为对象");
        assert!(!paths.is_empty(), "至少应登记一个接口");

        let schemes = json
            .pointer("/components/securitySchemes")
            .and_then(serde_json::Value::as_object)
            .expect("应注册 bearer_auth 安全方案");
        assert!(schemes.contains_key("bearer_auth"));
    }

    /// 文档路由的开关默认开启
    #[test]
    fn docs_enabled_by_default() {
        // 仅断言解析逻辑本身（不修改进程环境变量，避免测试间相互影响）
        assert!(docs_enabled() || std::env::var("API_DOCS_ENABLED").is_ok());
    }

    /// 调用方（`apps/backend/src-tauri/src/lib.rs`）的注入写法必须成立：
    /// `bool::then` 得到 `Option<Router>`，可直接传给 `api::start_http_server_with`。
    #[test]
    fn docs_router_can_be_injected() {
        fn accept(extra: Option<Router>) -> bool {
            extra.is_some()
        }

        // `then` 为真时会真正构建路由（内嵌的 Swagger UI 资源 + OpenAPI 文档）
        let injected = accept(docs_enabled().then(docs_router));
        assert_eq!(injected, docs_enabled());
    }

    /// 每个路径模板里的 `{x}` 必须在该操作中恰好有一个 `in: path` 参数
    ///
    /// 用来兜住两类常见错误：handler 忘了标 `Path<T>`，或随手手写 `params(...)`
    /// 与 `axum_extras` 自动解析出来的参数重复。
    #[test]
    fn path_template_params_are_consistent() {
        const METHODS: [&str; 8] = [
            "get", "post", "put", "delete", "patch", "options", "head", "trace",
        ];

        let spec = spec();
        let json = serde_json::to_value(&spec).expect("OpenAPI 规范应可序列化为 JSON");
        let paths = json
            .get("paths")
            .and_then(serde_json::Value::as_object)
            .expect("paths 应为对象");

        let mut operations = 0_usize;
        for (path, item) in paths {
            let placeholders: Vec<String> = path
                .split('{')
                .skip(1)
                .filter_map(|segment| segment.split('}').next())
                .map(str::to_string)
                .collect();

            let Some(item) = item.as_object() else {
                continue;
            };

            for (method, operation) in item {
                if !METHODS.contains(&method.as_str()) {
                    continue;
                }

                let params: Vec<String> = operation
                    .get("parameters")
                    .and_then(serde_json::Value::as_array)
                    .map(|params| {
                        params
                            .iter()
                            .filter(|param| {
                                param.get("in").and_then(serde_json::Value::as_str) == Some("path")
                            })
                            .filter_map(|param| {
                                param
                                    .get("name")
                                    .and_then(serde_json::Value::as_str)
                                    .map(str::to_string)
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                for placeholder in &placeholders {
                    let count = params.iter().filter(|name| *name == placeholder).count();
                    assert_eq!(
                        count,
                        1,
                        "{method_upper} {path} 的路径参数 {{{placeholder}}} 应恰好出现一次，实际 {count} 次",
                        method_upper = method.to_uppercase(),
                    );
                }

                operations += 1;
            }
        }

        assert!(
            operations >= 90,
            "应至少登记 90 个操作（当前 {operations}），新增接口后请同步 apidoc 的 paths(...)"
        );
    }
}
