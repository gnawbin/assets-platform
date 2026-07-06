//! 大文件分片上传 HTTP API 路由
//!
//! 实现 S3 原生分片上传的 5 个核心接口：
//! - POST /api/upload/init 初始化分片上传
//! - POST /api/upload/{id}/chunk 上报分片完成
//! - GET /api/upload/{id}/progress 查询上传进度
//! - POST /api/upload/{id}/complete 完成合并
//! - DELETE /api/upload/{id} 取消上传

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::api::response::{ApiError, ApiResponse};
use crate::storage::upload::UploadManager;

// ======================== 请求/响应类型 ========================

/// 初始化上传请求（占位）
#[derive(Debug, Deserialize)]
pub struct InitUploadRequest {
    pub filename: String,
    pub file_size: i64,
    pub mime_type: String,
    pub file_group_id: Option<String>,
    pub change_reason: Option<String>,
    pub file_md5: Option<String>,
}

/// 提交上传请求
#[derive(Debug, Deserialize)]
pub struct CommitUploadRequest {
    pub context_type: String,
    pub context_id: i64,
}

/// 上报分片请求
#[derive(Debug, Deserialize)]
pub struct ReportChunkRequest {
    pub part_number: i32,
    pub etag: String,
}

/// 开始上传响应
#[derive(Debug, Serialize)]
pub struct StartUploadResponse {
    pub upload_id: String,
    pub s3_upload_id: String,
    pub chunk_size: i64,
    pub total_chunks: i32,
    pub presigned_urls: Vec<String>,
}

/// 上传进度响应
#[derive(Debug, Serialize)]
pub struct UploadProgressResponse {
    pub status: String,
    pub received_chunks: Vec<i32>,
    pub total_chunks: i32,
    pub progress_pct: i32,
}

/// 完成上传响应
#[derive(Debug, Serialize)]
pub struct CompleteUploadResponse {
    pub file_url: String,
    pub etag: String,
}

// ======================== 共享状态 ========================

/// 上传路由共享状态
pub struct UploadRouterState {
    pub upload_mgr: UploadManager,
}

// ======================== 路由处理函数 ========================

/// 初始化分片上传（占位）
///
/// 只创建数据库记录（status=pending），不调用 S3。
/// 后续需调用 start_upload 才开始真正的 S3 分片上传。
pub async fn init_upload(
    State(state): State<Arc<UploadRouterState>>,
    Json(req): Json<InitUploadRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    if req.filename.is_empty() {
        return Err(ApiError::bad_request("文件名不能为空"));
    }
    if req.file_size <= 0 {
        return Err(ApiError::bad_request("文件大小必须大于 0"));
    }
    if req.file_size > 5 * 1024 * 1024 * 1024 * 1024 {
        return Err(ApiError::bad_request("文件大小超过 5TB 限制"));
    }

    let created_by: i64 = 1;
    let schema = crate::database::current_schema_name();

    match state
        .upload_mgr
        .init(
            &schema,
            &req.filename,
            req.file_size,
            &req.mime_type,
            created_by,
            req.file_group_id.as_deref(),
            None, // context_type
            None, // context_id
            req.change_reason.as_deref(),
            req.file_md5.as_deref(),
        )
        .await
    {
        Ok(record_id) => {
            let resp = serde_json::json!({
                "upload_id": record_id.to_string(),
                "need_start": true,
            });
            Ok(Json(ApiResponse::success(resp)))
        }
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 开始上传（从 pending 转 uploading）
///
/// 创建 S3 MultipartUpload，返回 presigned URLs 供前端直传。
pub async fn start_upload(
    State(state): State<Arc<UploadRouterState>>,
    Path(upload_id): Path<String>,
) -> Result<Json<ApiResponse<StartUploadResponse>>, ApiError> {
    let upload_id: i64 = upload_id
        .parse()
        .map_err(|_| ApiError::bad_request("upload_id 格式不正确"))?;

    let schema = crate::database::current_schema_name();

    match state.upload_mgr.start_upload(&schema, upload_id).await {
        Ok(result) => {
            let resp = StartUploadResponse {
                upload_id: result.upload_id,
                s3_upload_id: result.s3_upload_id,
                chunk_size: result.chunk_size,
                total_chunks: result.total_chunks,
                presigned_urls: result.presigned_urls,
            };
            Ok(Json(ApiResponse::success(resp)))
        }
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 提交上传（from completed to committed）
///
/// 关联业务实体，标记上传为已提交。
pub async fn commit_upload(
    State(state): State<Arc<UploadRouterState>>,
    Path(upload_id): Path<String>,
    Json(req): Json<CommitUploadRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let upload_id: i64 = upload_id
        .parse()
        .map_err(|_| ApiError::bad_request("upload_id 格式不正确"))?;

    let schema = crate::database::current_schema_name();

    match state
        .upload_mgr
        .commit(&schema, upload_id, &req.context_type, req.context_id)
        .await
    {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 上报分片上传完成
///
/// 前端上传某个分片到 S3 后，调用此接口告知后端该分片已完成。
pub async fn report_chunk(
    State(state): State<Arc<UploadRouterState>>,
    Path(upload_id): Path<String>,
    Json(req): Json<ReportChunkRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let upload_id: i64 = upload_id
        .parse()
        .map_err(|_| ApiError::bad_request("upload_id 格式不正确"))?;

    let schema = crate::database::current_schema_name();

    match state
        .upload_mgr
        .report_chunk(&schema, upload_id, req.part_number, &req.etag)
        .await
    {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 查询上传进度
///
/// 返回已接收的分片列表和进度百分比，用于前端断点续传。
pub async fn get_progress(
    State(state): State<Arc<UploadRouterState>>,
    Path(upload_id): Path<String>,
) -> Result<Json<ApiResponse<UploadProgressResponse>>, ApiError> {
    let upload_id: i64 = upload_id
        .parse()
        .map_err(|_| ApiError::bad_request("upload_id 格式不正确"))?;

    let schema = crate::database::current_schema_name();

    match state.upload_mgr.get_progress(&schema, upload_id).await {
        Ok(progress) => {
            let resp = UploadProgressResponse {
                status: progress.status,
                received_chunks: progress.received_chunks,
                total_chunks: progress.total_chunks,
                progress_pct: progress.progress_pct,
            };
            Ok(Json(ApiResponse::success(resp)))
        }
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 完成上传（合并分片）
///
/// 所有分片上传完成后调用此接口，后端执行 S3 CompleteMultipartUpload。
pub async fn complete_upload(
    State(state): State<Arc<UploadRouterState>>,
    Path(upload_id): Path<String>,
) -> Result<Json<ApiResponse<CompleteUploadResponse>>, ApiError> {
    let upload_id: i64 = upload_id
        .parse()
        .map_err(|_| ApiError::bad_request("upload_id 格式不正确"))?;

    let schema = crate::database::current_schema_name();

    match state.upload_mgr.complete(&schema, upload_id).await {
        Ok(result) => {
            let resp = CompleteUploadResponse {
                file_url: result.file_url,
                etag: result.etag,
            };
            Ok(Json(ApiResponse::success(resp)))
        }
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 取消上传
///
/// 取消进行中的分片上传，清理 S3 上的临时数据。
pub async fn abort_upload(
    State(state): State<Arc<UploadRouterState>>,
    Path(upload_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let upload_id: i64 = upload_id
        .parse()
        .map_err(|_| ApiError::bad_request("upload_id 格式不正确"))?;

    let schema = crate::database::current_schema_name();

    match state.upload_mgr.abort(&schema, upload_id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}
