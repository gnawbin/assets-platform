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

use std::sync::Arc;

use axum::Router;
use utoipa::OpenApi;
use utoipa_swagger_ui::{Config, SwaggerUi};

pub use self::openapi::ApiDoc;

/// Swagger UI 页面路径
pub const SWAGGER_UI_PATH: &str = "/swagger-ui";

/// OpenAPI 规范 JSON 路径
pub const OPENAPI_JSON_PATH: &str = "/api-docs/openapi.json";

/// 体检用：Swagger UI 入口文件
const INDEX_FILE: &str = "index.html";

/// 体检用：Swagger UI 主脚本（体积最能反映资源真假）
const BUNDLE_FILE: &str = "swagger-ui-bundle.js";

/// 真实 `swagger-ui-bundle.js` 的下限（v5.32.6 约 1.5 MB；占位桩不足 1 KB）
const BUNDLE_MIN_BYTES: usize = 100 * 1024;

/// 体检内嵌的 Swagger UI 静态资源，返回问题描述（`None` 表示正常）
///
/// `utoipa-swagger-ui` 在 **build script** 阶段把 swagger-ui 的 `dist/` 用
/// `include_bytes!` 嵌进二进制，它**只校验 zip 里能否解出 `dist/`，不校验内容**。
/// 因此若构建时 `SWAGGER_UI_DOWNLOAD_URL` 指向自行拼的「占位 zip」，编译照样成功，
/// 但 `/swagger-ui` 只会返回一行纯文本：
///
/// ```text
/// /* fake index.html */ url: "https://x" deepLinking: true, layout: "StandaloneLayout"
/// ```
///
/// 更麻烦的是 cargo 会缓存这次 build script 的产物（`target/*/build/utoipa-swagger-ui-*/out`），
/// 之后即使把环境变量去掉，只要 build script 没重跑，二进制里仍然是占位资源。
/// 这里把「构建期的问题」提前成一条可读的启动告警 / 测试失败，而不是等用户打开页面才发现。
pub fn dist_problem() -> Option<String> {
    let config = Arc::new(Config::default());

    let index = match utoipa_swagger_ui::serve(INDEX_FILE, Arc::clone(&config)) {
        Ok(Some(file)) => file.bytes.to_vec(),
        Ok(None) => return Some(format!("二进制里没有 {INDEX_FILE}")),
        Err(error) => return Some(format!("读取 {INDEX_FILE} 失败：{error}")),
    };

    let bundle = match utoipa_swagger_ui::serve(BUNDLE_FILE, Arc::clone(&config)) {
        Ok(Some(file)) => file.bytes.len(),
        Ok(None) => return Some(format!("二进制里没有 {BUNDLE_FILE}")),
        Err(error) => return Some(format!("读取 {BUNDLE_FILE} 失败：{error}")),
    };

    dist_problem_from(&index, bundle)
}

/// [`dist_problem`] 的纯函数内核（便于用占位样本做单测）
fn dist_problem_from(index: &[u8], bundle_len: usize) -> Option<String> {
    let looks_like_html =
        index.starts_with(b"<!") || index.windows(5).any(|window| window == b"<html");
    if !looks_like_html {
        let head = String::from_utf8_lossy(&index[..index.len().min(80)]);
        return Some(format!(
            "{INDEX_FILE} 不是 HTML（{} 字节，开头为 {head:?}）",
            index.len()
        ));
    }

    if bundle_len < BUNDLE_MIN_BYTES {
        return Some(format!(
            "{BUNDLE_FILE} 只有 {bundle_len} 字节（真实约 1.5 MB，低于 {BUNDLE_MIN_BYTES} 即占位资源）"
        ));
    }

    None
}

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
///
/// 构建期若嵌入了占位资源（见 [`dist_problem`]），这里会打一条 WARN —— 否则只能
/// 等用户打开 `/swagger-ui` 看到一行纯文本才发现。
pub fn docs_router() -> Router {
    if let Some(problem) = dist_problem() {
        tracing::warn!(
            "Swagger UI 静态资源不可用，/swagger-ui 会显示成纯文本: {}（请用官方 release zip 重新构建，见 docs/API-Swagger规范.md §5.2）",
            problem
        );
    }

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

    /// 占位 `index.html`（一行 `/* fake index.html */ …`）必须被识别
    #[test]
    fn dist_problem_detects_placeholder_index() {
        let fake_index =
            br#"/* fake index.html */ url: "https://x" deepLinking: true, layout: "StandaloneLayout""#;
        let problem = dist_problem_from(fake_index, 1_526_593).expect("占位 index 应被识别");
        assert!(problem.contains("不是 HTML"), "{}", problem);
    }

    /// 真实 index.html + 1.5 MB bundle 不应报警（避免误伤正常构建）
    #[test]
    fn dist_problem_accepts_real_dist() {
        let real_index =
            b"<!-- HTML for static distribution bundle build -->\n<!DOCTYPE html>\n<html></html>";
        assert_eq!(dist_problem_from(real_index, 1_526_593), None);
    }

    /// 半成品 dist（index.html 换了、bundle 还是桩）同样要报出来
    #[test]
    fn dist_problem_detects_tiny_bundle() {
        let real_index = b"<!DOCTYPE html><html></html>";
        let problem = dist_problem_from(real_index, 95).expect("小体积 bundle 应被识别");
        assert!(problem.contains(BUNDLE_FILE), "{}", problem);
    }

    /// 内嵌的 Swagger UI 资源必须是真实 dist，而不是占位桩
    ///
    /// `SWAGGER_UI_DOWNLOAD_URL` 指向占位 zip 时编译照样成功，但 `/swagger-ui`
    /// 只会返回 `/* fake index.html */ url: "https://x" ...` 一行纯文本。
    /// 这类问题在 CI 里应当尽早失败，而不是等用户打开页面才发现。
    #[test]
    fn embedded_swagger_ui_dist_is_real() {
        if let Some(problem) = dist_problem() {
            panic!(
                "内嵌的 Swagger UI 静态资源不可用: {}\n\
                 修复: cargo clean -p utoipa-swagger-ui && cargo build -p server（构建机需联网，\n\
                 或改用 features = [\"axum\", \"vendored\"]），详见 docs/API-Swagger规范.md §5.2",
                problem
            );
        }
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
