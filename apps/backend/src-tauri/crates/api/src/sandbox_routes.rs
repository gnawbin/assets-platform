//! 沙箱（CubeSandbox）HTTP API 路由
//!
//! 全部挂在 `protected_routes`（`auth_middleware` + `authz_middleware` 已生效），
//! 租户来自 `UserContext::schema_name`，用户来自 `UserContext::user_id`。
//!
//! 错误语义（设计文档 §8.1）：
//! - 能力未启用 / 服务不可用 → 503；
//! - 参数问题（代码超长、路径非法）→ 400；
//! - 其余 → 500；
//! `hint()` 已由 `service::sandbox_service::to_message` 拼进 message，前端直接展示。

use axum::extract::{Path, Query};
use axum::{Extension, Json};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
use serde::{Deserialize, Serialize};

use sandbox::{ExecResult, FileEntry, HealthInfo, SandboxHandle};
use service::sandbox_service::{self, SessionKey};

use super::auth::UserContext;
use super::response::{ApiError, ApiResponse};

/// 创建会话请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateSessionRequest {
    /// 业务会话标识（对话 ID / 工作流运行 ID / Skill ID；缺省 `default`）
    #[serde(default)]
    pub scope: String,
}

/// 执行代码请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ExecBody {
    /// 代码片段（缺省解释器 python3）
    pub code: String,
    /// 单次执行超时（秒），缺省取 `SANDBOX_TIMEOUT_SECS`
    #[serde(default, alias = "timeoutSecs")]
    pub timeout_secs: Option<u32>,
}

/// 执行命令请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CommandBody {
    /// 命令（无 `argv` 时按 `/bin/sh -lc` 解释）
    pub command: String,
    /// 命令参数（给出时按可执行文件 + 参数处理，不经过 shell）
    #[serde(default)]
    pub argv: Vec<String>,
}

/// 写文件请求（内容 base64，避免 JSON 字符串承载二进制）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct FileWriteBody {
    /// 沙箱内绝对路径
    pub path: String,
    /// 文件内容（base64 标准编码）
    #[serde(alias = "contentBase64")]
    pub content_base64: String,
}

/// 文件查询参数
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct FileQuery {
    /// 沙箱内绝对路径（下载时为文件路径，列目录时为目录路径）
    pub path: String,
}

/// 文件内容响应
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct FileContent {
    /// 沙箱内绝对路径
    pub path: String,
    /// 文件内容（base64 标准编码）
    pub content_base64: String,
    /// 原始字节数
    pub size: usize,
}

/// 会话键：租户取 schema 名，scope 为空时回落 `default`
fn session_key(user: &UserContext, scope: &str) -> SessionKey {
    let scope = if scope.trim().is_empty() {
        "default"
    } else {
        scope.trim()
    };
    SessionKey::new(user.schema_name.clone(), user.user_id, scope)
}

/// 服务层错误串 → HTTP 错误（带状态码分类）
fn service_error(message: String) -> ApiError {
    if message.contains("能力不支持") || message.contains("SANDBOX_ENABLED") {
        ApiError::service_unavailable(message)
    } else if message.contains("请求非法") || message.contains("超过上限") {
        ApiError::bad_request(message)
    } else {
        ApiError::internal_error(message)
    }
}

/// 控制面健康检查
#[utoipa::path(
    get,
    path = "/api/sandbox/health",
    tag = "沙箱",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "沙箱控制面健康状态", body = ApiResponse<HealthInfo>),
        (status = 401, description = "未认证", body = ApiError),
    )
)]
pub async fn health() -> Result<Json<ApiResponse<HealthInfo>>, ApiError> {
    sandbox_service::health()
        .await
        .map(|info| Json(ApiResponse::success(info)))
        .map_err(service_error)
}

/// 创建（或复用）会话沙箱
#[utoipa::path(
    post,
    path = "/api/sandbox/sessions",
    tag = "沙箱",
    security(("bearer_auth" = [])),
    request_body = CreateSessionRequest,
    responses(
        (status = 200, description = "沙箱句柄", body = ApiResponse<SandboxHandle>),
        (status = 400, description = "请求非法", body = ApiError),
        (status = 503, description = "沙箱未启用", body = ApiError),
    )
)]
pub async fn create_session(
    Extension(user): Extension<UserContext>,
    Json(body): Json<CreateSessionRequest>,
) -> Result<Json<ApiResponse<SandboxHandle>>, ApiError> {
    sandbox_service::ensure_session(session_key(&user, &body.scope))
        .await
        .map(|handle| Json(ApiResponse::success(handle)))
        .map_err(service_error)
}

/// 查询会话沙箱（不存在时按需创建）
#[utoipa::path(
    get,
    path = "/api/sandbox/sessions/{scope}",
    tag = "沙箱",
    security(("bearer_auth" = [])),
    params(("scope" = String, Path, description = "业务会话标识")),
    responses(
        (status = 200, description = "沙箱句柄", body = ApiResponse<SandboxHandle>),
        (status = 503, description = "沙箱未启用", body = ApiError),
    )
)]
pub async fn get_session(
    Extension(user): Extension<UserContext>,
    Path(scope): Path<String>,
) -> Result<Json<ApiResponse<SandboxHandle>>, ApiError> {
    sandbox_service::ensure_session(session_key(&user, &scope))
        .await
        .map(|handle| Json(ApiResponse::success(handle)))
        .map_err(service_error)
}

/// 执行代码
#[utoipa::path(
    post,
    path = "/api/sandbox/sessions/{scope}/exec",
    tag = "沙箱",
    security(("bearer_auth" = [])),
    params(("scope" = String, Path, description = "业务会话标识")),
    request_body = ExecBody,
    responses(
        (status = 200, description = "执行结果", body = ApiResponse<ExecResult>),
        (status = 400, description = "代码超限 / 参数非法", body = ApiError),
        (status = 503, description = "沙箱未启用", body = ApiError),
    )
)]
pub async fn exec_code(
    Extension(user): Extension<UserContext>,
    Path(scope): Path<String>,
    Json(body): Json<ExecBody>,
) -> Result<Json<ApiResponse<ExecResult>>, ApiError> {
    sandbox_service::exec_code(session_key(&user, &scope), body.code, body.timeout_secs)
        .await
        .map(|result| Json(ApiResponse::success(result)))
        .map_err(service_error)
}

/// 执行命令
#[utoipa::path(
    post,
    path = "/api/sandbox/sessions/{scope}/command",
    tag = "沙箱",
    security(("bearer_auth" = [])),
    params(("scope" = String, Path, description = "业务会话标识")),
    request_body = CommandBody,
    responses(
        (status = 200, description = "执行结果", body = ApiResponse<ExecResult>),
        (status = 503, description = "沙箱未启用", body = ApiError),
    )
)]
pub async fn run_command(
    Extension(user): Extension<UserContext>,
    Path(scope): Path<String>,
    Json(body): Json<CommandBody>,
) -> Result<Json<ApiResponse<ExecResult>>, ApiError> {
    sandbox_service::run_command(session_key(&user, &scope), body.command, body.argv)
        .await
        .map(|result| Json(ApiResponse::success(result)))
        .map_err(service_error)
}

/// 写文件（base64 内容）
#[utoipa::path(
    post,
    path = "/api/sandbox/sessions/{scope}/files",
    tag = "沙箱",
    security(("bearer_auth" = [])),
    params(("scope" = String, Path, description = "业务会话标识")),
    request_body = FileWriteBody,
    responses(
        (status = 200, description = "写入成功（`data` 为 null）"),
        (status = 400, description = "base64 解码失败 / 路径非法", body = ApiError),
        (status = 503, description = "沙箱未启用", body = ApiError),
    )
)]
pub async fn upload_file(
    Extension(user): Extension<UserContext>,
    Path(scope): Path<String>,
    Json(body): Json<FileWriteBody>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let data = BASE64_STANDARD
        .decode(body.content_base64.trim())
        .map_err(|error| {
            ApiError::bad_request(format!("contentBase64 不是合法的 base64：{error}"))
        })?;

    sandbox_service::upload_file(session_key(&user, &scope), body.path, data)
        .await
        .map(|()| Json(ApiResponse::success(())))
        .map_err(service_error)
}

/// 读文件（返回 base64 内容）
#[utoipa::path(
    get,
    path = "/api/sandbox/sessions/{scope}/files",
    tag = "沙箱",
    security(("bearer_auth" = [])),
    params(
        ("scope" = String, Path, description = "业务会话标识"),
        ("path" = String, Query, description = "沙箱内绝对路径"),
    ),
    responses(
        (status = 200, description = "文件内容（base64）", body = ApiResponse<FileContent>),
        (status = 404, description = "文件不存在", body = ApiError),
        (status = 503, description = "沙箱未启用", body = ApiError),
    )
)]
pub async fn download_file(
    Extension(user): Extension<UserContext>,
    Path(scope): Path<String>,
    Query(query): Query<FileQuery>,
) -> Result<Json<ApiResponse<FileContent>>, ApiError> {
    let path = query.path;
    sandbox_service::download_file(session_key(&user, &scope), path.clone())
        .await
        .map(|data| {
            Json(ApiResponse::success(FileContent {
                path,
                content_base64: BASE64_STANDARD.encode(&data),
                size: data.len(),
            }))
        })
        .map_err(service_error)
}

/// 列目录
#[utoipa::path(
    get,
    path = "/api/sandbox/sessions/{scope}/files/list",
    tag = "沙箱",
    security(("bearer_auth" = [])),
    params(
        ("scope" = String, Path, description = "业务会话标识"),
        ("path" = String, Query, description = "沙箱内目录路径"),
    ),
    responses(
        (status = 200, description = "目录条目", body = ApiResponse<Vec<FileEntry>>),
        (status = 503, description = "沙箱未启用", body = ApiError),
    )
)]
pub async fn list_files(
    Extension(user): Extension<UserContext>,
    Path(scope): Path<String>,
    Query(query): Query<FileQuery>,
) -> Result<Json<ApiResponse<Vec<FileEntry>>>, ApiError> {
    sandbox_service::list_files(session_key(&user, &scope), query.path)
        .await
        .map(|entries| Json(ApiResponse::success(entries)))
        .map_err(service_error)
}

/// 暂停会话沙箱（保留现场）
#[utoipa::path(
    post,
    path = "/api/sandbox/sessions/{scope}/pause",
    tag = "沙箱",
    security(("bearer_auth" = [])),
    params(("scope" = String, Path, description = "业务会话标识")),
    responses(
        (status = 200, description = "已暂停（`data` 为 null）"),
        (status = 503, description = "沙箱未启用", body = ApiError),
    )
)]
pub async fn pause(
    Extension(user): Extension<UserContext>,
    Path(scope): Path<String>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    sandbox_service::pause_session(session_key(&user, &scope))
        .await
        .map(|()| Json(ApiResponse::success(())))
        .map_err(service_error)
}

/// 恢复会话沙箱
#[utoipa::path(
    post,
    path = "/api/sandbox/sessions/{scope}/resume",
    tag = "沙箱",
    security(("bearer_auth" = [])),
    params(("scope" = String, Path, description = "业务会话标识")),
    responses(
        (status = 200, description = "沙箱句柄", body = ApiResponse<SandboxHandle>),
        (status = 503, description = "沙箱未启用", body = ApiError),
    )
)]
pub async fn resume(
    Extension(user): Extension<UserContext>,
    Path(scope): Path<String>,
) -> Result<Json<ApiResponse<SandboxHandle>>, ApiError> {
    sandbox_service::resume_session(session_key(&user, &scope))
        .await
        .map(|handle| Json(ApiResponse::success(handle)))
        .map_err(service_error)
}

/// 释放会话沙箱（销毁 MicroVM）
#[utoipa::path(
    delete,
    path = "/api/sandbox/sessions/{scope}",
    tag = "沙箱",
    security(("bearer_auth" = [])),
    params(("scope" = String, Path, description = "业务会话标识")),
    responses(
        (status = 200, description = "已销毁（`data` 为 null）"),
        (status = 503, description = "沙箱未启用", body = ApiError),
    )
)]
pub async fn delete_session(
    Extension(user): Extension<UserContext>,
    Path(scope): Path<String>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    sandbox_service::release_session(session_key(&user, &scope))
        .await
        .map(|()| Json(ApiResponse::success(())))
        .map_err(service_error)
}
