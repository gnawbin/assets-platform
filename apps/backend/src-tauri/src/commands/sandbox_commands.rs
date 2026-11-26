//! 沙箱（CubeSandbox）Tauri 命令
//!
//! 与 HTTP 路由（`crates/api/src/sandbox_routes.rs`）共用 `service::sandbox_service`，
//! 租户取 `database::current_schema_name()`（桌面端切换租户后自动指向新 schema）。
//!
//! 前端调用注意（Tauri v2）：`invoke()` 的参数名是 **camelCase**
//! （`#[tauri::command(rename_all = "camelCase")]` 已自动映射 `user_id` → `userId`）。

use sandbox::{ExecResult, FileEntry, HealthInfo, SandboxHandle};
use service::sandbox_service::{self, SessionKey};

/// 会话键：租户 schema + 用户 + 业务会话
fn session_key(user_id: i64, scope: &str) -> SessionKey {
    SessionKey::new(database::current_schema_name(), user_id, scope)
}

/// 控制面健康检查
#[tauri::command(rename_all = "camelCase")]
pub async fn sandbox_health() -> Result<HealthInfo, String> {
    sandbox_service::health().await
}

/// 取得（或创建）会话沙箱
#[tauri::command(rename_all = "camelCase")]
pub async fn sandbox_session(user_id: i64, scope: String) -> Result<SandboxHandle, String> {
    sandbox_service::ensure_session(session_key(user_id, &scope)).await
}

/// 执行代码（会话内复用沙箱）
#[tauri::command(rename_all = "camelCase")]
pub async fn sandbox_exec_code(
    user_id: i64,
    scope: String,
    code: String,
    timeout_secs: Option<u32>,
) -> Result<ExecResult, String> {
    sandbox_service::exec_code(session_key(user_id, &scope), code, timeout_secs).await
}

/// 执行命令
#[tauri::command(rename_all = "camelCase")]
pub async fn sandbox_run_command(
    user_id: i64,
    scope: String,
    command: String,
    argv: Vec<String>,
) -> Result<ExecResult, String> {
    sandbox_service::run_command(session_key(user_id, &scope), command, argv).await
}

/// 上传文件（前端把 `Uint8Array` 转成数字数组传入）
#[tauri::command(rename_all = "camelCase")]
pub async fn sandbox_upload_file(
    user_id: i64,
    scope: String,
    path: String,
    data: Vec<u8>,
) -> Result<(), String> {
    sandbox_service::upload_file(session_key(user_id, &scope), path, data).await
}

/// 下载文件（返回字节数组）
#[tauri::command(rename_all = "camelCase")]
pub async fn sandbox_read_file(
    user_id: i64,
    scope: String,
    path: String,
) -> Result<Vec<u8>, String> {
    sandbox_service::download_file(session_key(user_id, &scope), path).await
}

/// 列目录
#[tauri::command(rename_all = "camelCase")]
pub async fn sandbox_list_files(
    user_id: i64,
    scope: String,
    path: String,
) -> Result<Vec<FileEntry>, String> {
    sandbox_service::list_files(session_key(user_id, &scope), path).await
}

/// 暂停会话沙箱（保留现场）
#[tauri::command(rename_all = "camelCase")]
pub async fn sandbox_pause_session(user_id: i64, scope: String) -> Result<(), String> {
    sandbox_service::pause_session(session_key(user_id, &scope)).await
}

/// 恢复会话沙箱
#[tauri::command(rename_all = "camelCase")]
pub async fn sandbox_resume_session(user_id: i64, scope: String) -> Result<SandboxHandle, String> {
    sandbox_service::resume_session(session_key(user_id, &scope)).await
}

/// 释放会话沙箱（销毁 MicroVM）
#[tauri::command(rename_all = "camelCase")]
pub async fn sandbox_release_session(user_id: i64, scope: String) -> Result<(), String> {
    sandbox_service::release_session(session_key(user_id, &scope)).await
}
