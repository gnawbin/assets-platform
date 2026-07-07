//! 分片上传管理工具类
//!
//! 纯工具类，封装分片上传的数据库记录管理，不依赖任何业务模型。
//! 依赖 S3Client 进行 S3 协议操作。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::storage::s3::{S3Client, S3Config};

// ======================== 数据模型 ========================

/// 文件上传记录（对应数据库 {schema}.file_uploads 表）
///
/// 支持附件版本管理（file_group_id/version/is_latest）和两步提交（context_type/context_id）。
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct FileUploadRecord {
    pub id: i64,
    // ---- 版本管理字段 ----
    pub file_group_id: String,         // UUID，同一文件的不同版本共用
    pub version: i32,                  // 版本号，从 1 开始递增
    pub is_latest: bool,               // 是否为当前最新版本
    pub change_reason: Option<String>, // 变更原因
    pub file_md5: Option<String>,      // 文件 MD5
    // ---- S3 分片上传字段 ----
    pub upload_id: Option<String>, // S3 Multipart Upload ID（pending 时可为空）
    pub bucket: Option<String>,    // S3 存储桶
    pub object_key: Option<String>, // S3 对象键
    pub original_filename: String,
    pub file_size: i64,
    pub mime_type: Option<String>,
    pub chunk_size: i32,
    pub total_chunks: i32,
    pub received_chunks: Vec<i32>,
    pub status: String, // pending/uploading/completed/committed/cancelled/failed
    pub file_url: Option<String>,
    pub etag: Option<String>,
    // ---- 业务上下文 ----
    pub context_type: Option<String>, // 业务类型：knowledge/asset/document
    pub context_id: Option<i64>,      // 业务实体 ID
    pub commit_at: Option<DateTime<Utc>>, // 正式提交时间
    pub created_by: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted: i16,
}

/// 初始化上传返回
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadInitResult {
    pub upload_id: String,           // 数据库记录 ID（字符串）
    pub s3_upload_id: String,        // S3 UploadId
    pub chunk_size: i64,             // 分片大小（字节）
    pub total_chunks: i32,           // 总分片数
    pub presigned_urls: Vec<String>, // 每个分片的直传 URL
}

/// 上传进度
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadProgress {
    pub status: String,
    pub received_chunks: Vec<i32>,
    pub total_chunks: i32,
    pub progress_pct: i32,
}

/// 完成上传返回
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadCompleteResult {
    pub file_url: String,
    pub etag: String,
}

// ======================== 分片大小计算 ========================

/// 计算分片大小和数量
///
/// 规则：
/// - 最小分片：5MB（S3 要求）
/// - 最大分片：5GB（S3 要求）
/// - 最多分片：10000（S3 要求）
/// - 自动调整分片大小以不超过 10000 个分片
fn calculate_chunks(file_size: i64) -> (i64, i32) {
    const MIN_CHUNK_SIZE: i64 = 5 * 1024 * 1024; // 5MB
    const MAX_CHUNK_SIZE: i64 = 5 * 1024 * 1024 * 1024; // 5GB
    const MAX_PARTS: i32 = 10000;

    // 默认分片大小 5MB
    let mut chunk_size = MIN_CHUNK_SIZE;

    // 如果默认分片会导致超过 10000 个分片，增大分片大小
    let parts = (file_size + chunk_size - 1) / chunk_size;
    if parts as i32 > MAX_PARTS {
        chunk_size = (file_size + MAX_PARTS as i64 - 1) / MAX_PARTS as i64;
        // 对齐到 1MB 边界
        chunk_size = ((chunk_size + 1024 * 1024 - 1) / (1024 * 1024)) * (1024 * 1024);
        if chunk_size > MAX_CHUNK_SIZE {
            chunk_size = MAX_CHUNK_SIZE;
        }
    }

    let total_chunks = ((file_size + chunk_size - 1) / chunk_size) as i32;
    (chunk_size, total_chunks)
}

// ======================== Object Key 生成 ========================

/// 生成 S3 Object Key（支持版本管理）
///
/// 格式：uploads/{YYYY-MM}/{file_group_id}/v{version}/{uuid}-{filename}
fn generate_object_key(file_group_id: &str, version: i32, original_filename: &str) -> String {
    let now = Utc::now();
    let date_prefix = now.format("%Y-%m").to_string();
    let uuid = uuid::Uuid::new_v4();
    format!(
        "uploads/{}/{}/v{}/{}-{}",
        date_prefix, file_group_id, version, uuid, original_filename
    )
}

// ======================== UploadManager ========================

/// 分片上传管理器
pub struct UploadManager {
    pool: PgPool,
    s3: S3Client,
    config: S3Config,
}

impl UploadManager {
    pub fn new(pool: PgPool, s3: S3Client, config: S3Config) -> Self {
        Self { pool, s3, config }
    }

    /// 初始化上传（占位）
    ///
    /// 只创建数据库记录（status=pending），不调用 S3。
    /// 后续需调用 start_upload() 才开始真正的 S3 分片上传。
    pub async fn init(
        &self,
        schema: &str,
        filename: &str,
        file_size: i64,
        mime_type: &str,
        created_by: i64,
        file_group_id: Option<&str>, // 传入则续版本，None 则新建
        context_type: Option<&str>,  // 业务上下文类型
        context_id: Option<i64>,     // 业务实体 ID
        change_reason: Option<&str>, // 变更原因
        file_md5: Option<&str>,      // 文件 MD5
    ) -> Result<i64, String> {
        // 1. 确定 file_group_id 和 version
        let (file_group_id, version) = if let Some(gid) = file_group_id {
            // 已有 group：查最大版本 +1
            let max_ver = sqlx::query_scalar::<_, Option<i32>>(sqlx::AssertSqlSafe(format!(
                "SELECT MAX(version) FROM {}.file_uploads WHERE file_group_id = $1 AND deleted = 0",
                schema
            )))
            .bind(gid)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| format!("查询最大版本号失败: {}", e))?
            .unwrap_or(0);
            (gid.to_string(), max_ver + 1)
        } else {
            // 新建 group：生成 UUID，version = 1
            let gid = uuid::Uuid::new_v4().to_string();
            (gid, 1)
        };

        // 2. 标记旧版本为非最新（版本号 > 1 表示是替换）
        if version > 1 {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {}.file_uploads SET is_latest = false WHERE file_group_id = $1 AND deleted = 0",
                schema
            )))
            .bind(&file_group_id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("标记旧版本失败: {}", e))?;
        }

        // 3. 计算分片大小和数量
        let (chunk_size, total_chunks) = calculate_chunks(file_size);
        if total_chunks > 10000 {
            return Err("文件过大，分片数超过 S3 限制（10000）".to_string());
        }

        // 4. 使用 Snowflake 生成 ID
        let id = crate::utils::snowflake::next_id() as i64;

        // 5. 保存上传记录到数据库（status = pending，不调 S3）
        sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            INSERT INTO {}.file_uploads
                (id, file_group_id, version, is_latest, change_reason, file_md5,
                 original_filename, file_size, mime_type,
                 chunk_size, total_chunks, received_chunks, status,
                 context_type, context_id, created_by, created_at, updated_at)
            VALUES
                ($1, $2, $3, $4, $5, $6,
                 $7, $8, $9,
                 $10, $11, $12, 'pending',
                 $13, $14, $15, NOW(), NOW())
            "#,
            schema
        )))
        .bind(id)
        .bind(&file_group_id)
        .bind(version)
        .bind(version == 1) // 首个版本 is_latest=true
        .bind(change_reason)
        .bind(file_md5)
        .bind(filename)
        .bind(file_size)
        .bind(mime_type)
        .bind(chunk_size as i32)
        .bind(total_chunks)
        .bind(&Vec::<i32>::new())
        .bind(context_type)
        .bind(context_id)
        .bind(created_by)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("保存上传记录失败: {}", e))?;

        Ok(id)
    }

    /// 开始上传（从 pending 转为 uploading，创建 S3 MultipartUpload）
    ///
    /// 1. 验证 status = pending
    /// 2. 调用 S3 CreateMultipartUpload
    /// 3. 生成 Presigned URLs
    /// 4. 更新数据库：status=uploading, upload_id, bucket, object_key
    /// 5. 返回 presigned_urls 供前端直传
    pub async fn start_upload(
        &self,
        schema: &str,
        upload_id: i64,
    ) -> Result<UploadInitResult, String> {
        let record = sqlx::query_as::<_, FileUploadRecord>(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM {}.file_uploads WHERE id = $1 AND deleted = 0 FOR UPDATE",
            schema
        )))
        .bind(upload_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("查询上传记录失败: {}", e))?
        .ok_or_else(|| "上传记录不存在".to_string())?;

        if record.status != "pending" {
            return Err(format!("上传状态不是 pending，当前状态: {}", record.status));
        }

        // 生成 Object Key（含版本路径）
        let object_key = generate_object_key(
            &record.file_group_id,
            record.version,
            &record.original_filename,
        );
        let (chunk_size, total_chunks) = calculate_chunks(record.file_size);

        // 创建 S3 MultipartUpload
        let s3_upload_id = self
            .s3
            .create_multipart_upload(
                &self.config.bucket,
                &object_key,
                record
                    .mime_type
                    .as_deref()
                    .unwrap_or("application/octet-stream"),
            )
            .await
            .map_err(|e| format!("创建 S3 分片上传失败: {}", e))?;

        // 生成 Presigned URLs
        let presigned_urls = self
            .s3
            .presign_upload_parts(
                &self.config.bucket,
                &object_key,
                &s3_upload_id,
                total_chunks,
            )
            .await
            .map_err(|e| format!("生成 Presigned URL 失败: {}", e))?;

        // 更新数据库
        sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            UPDATE {}.file_uploads
            SET status = 'uploading',
                upload_id = $1, bucket = $2, object_key = $3,
                chunk_size = $4, total_chunks = $5,
                updated_at = NOW()
            WHERE id = $6
            "#,
            schema
        )))
        .bind(&s3_upload_id)
        .bind(&self.config.bucket)
        .bind(&object_key)
        .bind(chunk_size as i32)
        .bind(total_chunks)
        .bind(upload_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("更新上传记录失败: {}", e))?;

        Ok(UploadInitResult {
            upload_id: upload_id.to_string(),
            s3_upload_id,
            chunk_size,
            total_chunks,
            presigned_urls,
        })
    }

    /// 上报分片上传完成
    ///
    /// 使用原子 SQL 操作追加分片到 received_chunks，避免并发覆盖。
    /// PostgreSQL 数组追加 + 幂等性检查在单条 SQL 中完成。
    ///
    /// 性能说明：PostgreSQL 的数组操作是 O(n)，n 最大 10000 分片，性能可接受。
    /// 如需更高性能可改用 jsonb 或关联表，但当前方案已足够。
    pub async fn report_chunk(
        &self,
        schema: &str,
        upload_id: i64,
        part_number: i32,
        _etag: &str,
    ) -> Result<(), String> {
        // 参数校验（不依赖数据库查询）
        if part_number < 1 {
            tracing::error!(
                "[report_chunk] 参数错误: upload_id={}, part_number={} < 1",
                upload_id,
                part_number
            );
            return Err(format!("分片序号 {} 不合法，必须大于等于 1", part_number));
        }

        tracing::info!(
            "[report_chunk] 开始: upload_id={}, part_number={}",
            upload_id,
            part_number
        );

        // 原子操作：UPDATE + 数组追加，单条 SQL 完成
        // - received_chunks || ARRAY[$1]：追加分片号到数组末尾
        // - NOT ($1 = ANY(received_chunks))：幂等性，已存在则跳过
        // - status = 'uploading'：状态校验
        // 全部在 PostgreSQL 事务中原子完成，无需 FOR UPDATE 行锁
        let result = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            UPDATE {}.file_uploads
            SET received_chunks = received_chunks || $1::int[],
                updated_at = NOW()
            WHERE id = $2
              AND deleted = 0
              AND status = 'uploading'
              AND $1 <@ ARRAY(SELECT generate_series(1, total_chunks)) -- 分片号合法
              AND NOT ($1 = ANY(received_chunks)) -- 幂等性，已存在则跳过
            "#,
            schema
        )))
        .bind(&vec![part_number])
        .bind(upload_id)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            tracing::error!(
                "[report_chunk] SQL 执行失败: upload_id={}, part_number={}, error={}",
                upload_id,
                part_number,
                e
            );
            format!("更新分片记录失败: {}", e)
        })?;

        // 检查是否有行被更新
        if result.rows_affected() == 0 {
            tracing::warn!(
                "[report_chunk] 0 rows affected: upload_id={}, part_number={}",
                upload_id,
                part_number
            );
            // 0 行影响可能是：记录不存在、状态不对、分片已存在、分片号不合法
            // 查一次数据库确定具体原因（仅在异常时查询，不影响正常路径性能）
            let record = sqlx::query_as::<_, FileUploadRecord>(sqlx::AssertSqlSafe(format!(
                "SELECT * FROM {}.file_uploads WHERE id = $1 AND deleted = 0",
                schema
            )))
            .bind(upload_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| format!("查询上传记录失败: {}", e))?
            .ok_or_else(|| "上传记录不存在".to_string())?;

            tracing::warn!(
                "[report_chunk] 状态检查: upload_id={}, status={}, total_chunks={}, received={:?}",
                upload_id,
                record.status,
                record.total_chunks,
                record.received_chunks
            );

            // 根据记录状态判断错误
            if record.status != "uploading" {
                return Err(format!(
                    "上传状态不是 uploading，当前状态: {}",
                    record.status
                ));
            }

            if part_number > record.total_chunks {
                return Err(format!(
                    "分片序号 {} 不合法，有效范围: 1~{}",
                    part_number, record.total_chunks
                ));
            }

            // 分片已上报，幂等返回成功
            if record.received_chunks.contains(&part_number) {
                tracing::info!(
                    "[report_chunk] 幂等跳过: upload_id={}, part_number={}",
                    upload_id,
                    part_number
                );
                return Ok(());
            }

            tracing::error!(
                "[report_chunk] 未知原因导致 0 rows affected: upload_id={}, part_number={}",
                upload_id,
                part_number
            );
        } else {
            tracing::info!(
                "[report_chunk] 成功: upload_id={}, part_number={}, rows_affected={}",
                upload_id,
                part_number,
                result.rows_affected()
            );
        }

        Ok(())
    }

    /// 查询上传进度
    ///
    /// 返回已接收的分片列表和进度百分比
    pub async fn get_progress(
        &self,
        schema: &str,
        upload_id: i64,
    ) -> Result<UploadProgress, String> {
        let record = sqlx::query_as::<_, FileUploadRecord>(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM {}.file_uploads WHERE id = $1 AND deleted = 0",
            schema
        )))
        .bind(upload_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("查询上传记录失败: {}", e))?
        .ok_or_else(|| "上传记录不存在".to_string())?;

        let received_count = record.received_chunks.len() as i32;
        let progress_pct = if record.total_chunks > 0 {
            (received_count * 100) / record.total_chunks
        } else {
            0
        };

        Ok(UploadProgress {
            status: record.status,
            received_chunks: record.received_chunks,
            total_chunks: record.total_chunks,
            progress_pct,
        })
    }

    /// 完成上传（合并分片）
    ///
    /// 1. 检查所有分片是否已上传
    /// 2. 调用 S3 CompleteMultipartUpload
    /// 3. 更新数据库状态为 completed（待提交）
    /// 4. 返回文件访问 URL
    pub async fn complete(
        &self,
        schema: &str,
        upload_id: i64,
    ) -> Result<UploadCompleteResult, String> {
        // 1. 获取上传记录
        let record = sqlx::query_as::<_, FileUploadRecord>(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM {}.file_uploads WHERE id = $1 AND deleted = 0 FOR UPDATE",
            schema
        )))
        .bind(upload_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("查询上传记录失败: {}", e))?
        .ok_or_else(|| "上传记录不存在".to_string())?;

        if record.status != "uploading" {
            return Err(format!(
                "上传状态不是 uploading，当前状态: {}",
                record.status
            ));
        }

        // 2. 检查所有分片是否已上传
        let received_set: std::collections::HashSet<i32> =
            record.received_chunks.iter().cloned().collect();
        let all_parts: Vec<i32> = (1..=record.total_chunks).collect();
        let missing: Vec<i32> = all_parts
            .iter()
            .filter(|p| !received_set.contains(p))
            .cloned()
            .collect();

        if !missing.is_empty() {
            return Err(format!(
                "还有 {} 个分片未上传: {:?}",
                missing.len(),
                missing
            ));
        }

        // 3. 构建 CompletedPart 列表
        let parts: Vec<aws_sdk_s3::types::CompletedPart> = record
            .received_chunks
            .iter()
            .map(|part_number| {
                aws_sdk_s3::types::CompletedPart::builder()
                    .part_number(*part_number)
                    .e_tag(format!("\"uploaded-{}\"", part_number))
                    .build()
            })
            .collect();

        // 4. 获取 S3 信息（uploading 状态时这些字段必有值）
        let bucket = record.bucket.as_deref().unwrap_or(&self.config.bucket);
        let object_key = record
            .object_key
            .as_deref()
            .ok_or_else(|| "缺少 Object Key".to_string())?;
        let s3_upload_id = record
            .upload_id
            .as_deref()
            .ok_or_else(|| "缺少 S3 UploadId".to_string())?;

        // 5. 调用 S3 CompleteMultipartUpload
        let s3_etag = self
            .s3
            .complete_multipart_upload(bucket, object_key, s3_upload_id, &parts)
            .await
            .map_err(|e| format!("S3 合并分片失败: {}", e))?;

        // 6. 生成文件访问 URL
        let file_url = format!("{}/{}", self.config.public_base_url, object_key);

        // 7. 更新数据库状态为 completed（待后续 commit）
        sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            UPDATE {}.file_uploads
            SET status = 'completed', file_url = $1, etag = $2, updated_at = NOW()
            WHERE id = $3
            "#,
            schema
        )))
        .bind(&file_url)
        .bind(&s3_etag)
        .bind(upload_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("更新上传状态失败: {}", e))?;

        Ok(UploadCompleteResult {
            file_url,
            etag: s3_etag,
        })
    }

    /// 提交上传（从 completed 转为 committed，关联业务实体）
    ///
    /// 1. 验证 status = completed
    /// 2. 更新 context_type、context_id、commit_at
    /// 3. 标记 status = committed
    pub async fn commit(
        &self,
        schema: &str,
        upload_id: i64,
        context_type: &str,
        context_id: i64,
    ) -> Result<(), String> {
        let record = sqlx::query_as::<_, FileUploadRecord>(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM {}.file_uploads WHERE id = $1 AND deleted = 0 FOR UPDATE",
            schema
        )))
        .bind(upload_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("查询上传记录失败: {}", e))?
        .ok_or_else(|| "上传记录不存在".to_string())?;

        if record.status != "completed" {
            return Err(format!(
                "上传状态不是 completed，当前状态: {}",
                record.status
            ));
        }

        sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            UPDATE {}.file_uploads
            SET status = 'committed',
                context_type = $1, context_id = $2,
                commit_at = NOW(),
                updated_at = NOW()
            WHERE id = $3
            "#,
            schema
        )))
        .bind(context_type)
        .bind(context_id)
        .bind(upload_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("提交上传记录失败: {}", e))?;

        Ok(())
    }

    /// 取消上传
    ///
    /// 1. 调用 S3 AbortMultipartUpload
    /// 2. 更新数据库状态为 cancelled
    pub async fn abort(&self, schema: &str, upload_id: i64) -> Result<(), String> {
        // 1. 获取上传记录
        let record = sqlx::query_as::<_, FileUploadRecord>(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM {}.file_uploads WHERE id = $1 AND deleted = 0",
            schema
        )))
        .bind(upload_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("查询上传记录失败: {}", e))?
        .ok_or_else(|| "上传记录不存在".to_string())?;

        if record.status != "uploading" {
            return Err(format!(
                "上传状态不是 uploading，当前状态: {}",
                record.status
            ));
        }

        // 2. 尝试取消 S3 上的分片上传
        if let (Some(bucket), Some(object_key), Some(s3_upload_id)) =
            (&record.bucket, &record.object_key, &record.upload_id)
        {
            if let Err(e) = self
                .s3
                .abort_multipart_upload(bucket, object_key, s3_upload_id)
                .await
            {
                // 忽略 "NoSuchUpload" 错误（S3 上可能已被清理）
                tracing::warn!("取消 S3 分片上传失败（可忽略）: {}", e);
            }
        }

        // 3. 更新数据库状态
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {}.file_uploads SET status = 'cancelled', updated_at = NOW() WHERE id = $1",
            schema
        )))
        .bind(upload_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("更新上传状态失败: {}", e))?;

        Ok(())
    }

    /// 获取版本历史
    pub async fn get_version_history(
        &self,
        schema: &str,
        file_group_id: &str,
    ) -> Result<Vec<FileUploadRecord>, String> {
        let records = sqlx::query_as::<_, FileUploadRecord>(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT * FROM {}.file_uploads
            WHERE file_group_id = $1 AND deleted = 0 AND status = 'committed'
            ORDER BY version DESC
            "#,
            schema
        )))
        .bind(file_group_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("查询版本历史失败: {}", e))?;

        Ok(records)
    }

    /// 清理过期上传（定时任务用）
    ///
    /// 删除超过指定小时数未完成的上传记录
    /// 同时调用 S3 AbortMultipartUpload 清理 S3 上的临时分片
    pub async fn clean_expired(&self, schema: &str, expire_hours: i64) -> Result<i64, String> {
        // 1. 查询过期上传记录（包括 pending、uploading）
        let records = sqlx::query_as::<_, FileUploadRecord>(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT * FROM {}.file_uploads
            WHERE status IN ('pending', 'uploading')
              AND created_at < NOW() - INTERVAL '{} hours'
              AND deleted = 0
            "#,
            schema, expire_hours
        )))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("查询过期上传记录失败: {}", e))?;

        let count = records.len() as i64;

        for record in &records {
            // 尝试清理 S3 上的分片（仅 uploading 状态有 S3 上传）
            if record.status == "uploading" {
                if let (Some(bucket), Some(object_key), Some(s3_upload_id)) =
                    (&record.bucket, &record.object_key, &record.upload_id)
                {
                    if let Err(e) = self
                        .s3
                        .abort_multipart_upload(bucket, object_key, s3_upload_id)
                        .await
                    {
                        tracing::warn!("清理 S3 分片上传失败（upload_id={}）: {}", record.id, e);
                    }
                }
            }

            // 软删除记录
            if let Err(e) = sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {}.file_uploads SET deleted = 1, updated_at = NOW() WHERE id = $1",
                schema
            )))
            .bind(record.id)
            .execute(&self.pool)
            .await
            {
                tracing::warn!("清理上传记录失败（id={}）: {}", record.id, e);
            }
        }

        Ok(count)
    }
}
