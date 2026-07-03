//! 大文件上传相关 Tauri Command
//!
//! 提供两步提交的 Tauri 命令接口：
//! - upload_init: 初始化上传占位（status=pending）
//! - upload_start: 开始上传（创建 S3 MultipartUpload，返回 presigned URLs）
//! - upload_commit: 提交上传（关联业务实体）
//! - upload_get_version_history: 获取文件版本历史
//! - upload_rollback: 回滚到指定版本

use crate::storage::s3::{S3Client, S3Config};
use crate::storage::upload::UploadManager;

/// 初始化上传（占位）
///
/// 只创建数据库记录（status=pending），不调用 S3。
/// 返回 upload_id（数据库记录 ID），后续需调用 upload_start 开始上传。
#[tauri::command]
pub async fn upload_init(
    filename: String,
    fileSize: i64,
    mimeType: String,
    fileGroupId: Option<String>,
    changeReason: Option<String>,
    fileMd5: Option<String>,
) -> Result<String, String> {
    let pool = crate::database::get_pool().map_err(|e| format!("获取数据库连接池失败: {}", e))?;
    let s3_config = S3Config::from_env().map_err(|e| format!("S3 配置加载失败: {}", e))?;
    let s3_client = S3Client::new(s3_config.clone())
        .await
        .map_err(|e| format!("S3 客户端初始化失败: {}", e))?;
    let upload_mgr = UploadManager::new(pool, s3_client, s3_config);

    // 当前用户 ID（Tauri v2 中从 invoke 上下文获取）
    let created_by: i64 = 1;

    let schema = "public".to_string();

    let record_id = upload_mgr
        .init(
            &schema,
            &filename,
            fileSize,
            &mimeType,
            created_by,
            fileGroupId.as_deref(),
            None, // context_type — 前端不传，commit 时再补充
            None, // context_id
            changeReason.as_deref(),
            fileMd5.as_deref(),
        )
        .await?;

    Ok(record_id.to_string())
}

/// 开始上传（将 pending 转为 uploading）
///
/// 创建 S3 MultipartUpload，返回 presigned URLs 供前端直传。
#[tauri::command]
pub async fn upload_start(uploadId: String) -> Result<serde_json::Value, String> {
    let upload_id: i64 = uploadId
        .parse()
        .map_err(|_| "upload_id 格式不正确".to_string())?;

    let pool = crate::database::get_pool().map_err(|e| format!("获取数据库连接池失败: {}", e))?;
    let s3_config = S3Config::from_env().map_err(|e| format!("S3 配置加载失败: {}", e))?;
    let s3_client = S3Client::new(s3_config.clone())
        .await
        .map_err(|e| format!("S3 客户端初始化失败: {}", e))?;
    let upload_mgr = UploadManager::new(pool, s3_client, s3_config);

    let schema = "public".to_string();

    let result = upload_mgr.start_upload(&schema, upload_id).await?;

    Ok(serde_json::json!({
        "uploadId": result.upload_id,
        "s3UploadId": result.s3_upload_id,
        "chunkSize": result.chunk_size,
        "totalChunks": result.total_chunks,
        "presignedUrls": result.presigned_urls,
    }))
}

/// 提交上传（从 completed 转为 committed，关联业务实体）
#[tauri::command]
pub async fn upload_commit(
    uploadId: String,
    contextType: String,
    contextId: String,
) -> Result<(), String> {
    let upload_id: i64 = uploadId
        .parse()
        .map_err(|_| "upload_id 格式不正确".to_string())?;
    let context_id: i64 = contextId
        .parse()
        .map_err(|_| "context_id 格式不正确".to_string())?;

    let pool = crate::database::get_pool().map_err(|e| format!("获取数据库连接池失败: {}", e))?;
    let s3_config = S3Config::from_env().map_err(|e| format!("S3 配置加载失败: {}", e))?;
    let s3_client = S3Client::new(s3_config.clone())
        .await
        .map_err(|e| format!("S3 客户端初始化失败: {}", e))?;
    let upload_mgr = UploadManager::new(pool, s3_client, s3_config);

    let schema = "public".to_string();

    upload_mgr
        .commit(&schema, upload_id, &contextType, context_id)
        .await
}

/// 获取文件版本历史
#[tauri::command]
pub async fn upload_get_version_history(
    fileGroupId: String,
) -> Result<Vec<serde_json::Value>, String> {
    let pool = crate::database::get_pool().map_err(|e| format!("获取数据库连接池失败: {}", e))?;
    let s3_config = S3Config::from_env().map_err(|e| format!("S3 配置加载失败: {}", e))?;
    let s3_client = S3Client::new(s3_config.clone())
        .await
        .map_err(|e| format!("S3 客户端初始化失败: {}", e))?;
    let upload_mgr = UploadManager::new(pool, s3_client, s3_config);

    let schema = "public".to_string();

    let records = upload_mgr
        .get_version_history(&schema, &fileGroupId)
        .await?;

    let versions: Vec<serde_json::Value> = records
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id.to_string(),
                "version": r.version,
                "isLatest": r.is_latest,
                "originalFilename": r.original_filename,
                "fileSize": r.file_size,
                "fileUrl": r.file_url,
                "changeReason": r.change_reason,
                "createdAt": r.created_at,
            })
        })
        .collect();

    Ok(versions)
}
