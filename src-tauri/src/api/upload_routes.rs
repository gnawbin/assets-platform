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

/// 初始化上传请求
#[derive(Debug, Deserialize)]
pub struct InitUploadRequest {
    pub filename: String,
    pub file_size: i64,
    pub mime_type: String,
}

/// 上报分片请求
#[derive(Debug, Deserialize)]
pub struct ReportChunkRequest {
    pub part_number: i32,
    pub etag: String,
}

/// 初始化上传响应
#[derive(Debug, Serialize)]
pub struct InitUploadResponse {
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

/// 初始化分片上传
///
/// 前端上传文件前先调用此接口，获取 S3 UploadId 和所有分片的 Presigned URL。
pub async fn init_upload(
    State(state): State<Arc<UploadRouterState>>,
    Json(req): Json<InitUploadRequest>,
) -> Result<Json<ApiResponse<InitUploadResponse>>, ApiError> {
    // 验证参数
    if req.filename.is_empty() {
        return Err(ApiError::bad_request("文件名不能为空"));
    }
    if req.file_size <= 0 {
        return Err(ApiError::bad_request("文件大小必须大于 0"));
    }
    if req.file_size > 5 * 1024 * 1024 * 1024 * 1024 {
        // 5TB（S3 限制）
        return Err(ApiError::bad_request("文件大小超过 5TB 限制"));
    }

    let created_by: i64 = 1;

    let schema = "public".to_string(); // upload routes will use their own schema context

    match state
        .upload_mgr
        .init(
            &schema,
            &req.filename,
            req.file_size,
            &req.mime_type,
            created_by,
        )
        .await
    {
        Ok(result) => {
            let resp = InitUploadResponse {
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

    let schema = "public".to_string();

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

    let schema = "public".to_string();

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

    let schema = "public".to_string();

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

    let schema = "public".to_string();

    match state.upload_mgr.abort(&schema, upload_id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}
