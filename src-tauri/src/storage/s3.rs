//! S3 客户端工具类
//!
//! 纯工具类，只封装 S3 协议操作，不涉及任何业务逻辑。
//! 支持 S3v4 协议兼容的对象存储（AWS S3、MinIO、RustFS 等）。

use std::time::Duration;

use anyhow::{anyhow, Result};
use aws_credential_types::Credentials;
use aws_sdk_s3::config::{BehaviorVersion, Region};
use aws_sdk_s3::types::CompletedPart;
use aws_sdk_s3::Client as S3NativeClient;
use serde::{Deserialize, Serialize};

// ======================== S3 配置 ========================

/// S3 存储配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Config {
    pub endpoint: String,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    pub bucket: String,
    pub public_base_url: String,
}

impl Default for S3Config {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:9000".to_string(),
            region: "us-east-1".to_string(),
            access_key: "minioadmin".to_string(),
            secret_key: "minioadmin".to_string(),
            bucket: "assets-platform".to_string(),
            public_base_url: "http://localhost:9000/assets-platform".to_string(),
        }
    }
}

impl S3Config {
    /// 从环境变量加载 S3 配置
    pub fn from_env() -> Result<Self> {
        let endpoint =
            std::env::var("S3_ENDPOINT").map_err(|_| anyhow!("缺少 S3_ENDPOINT 环境变量"))?;
        let region = std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string());
        let access_key =
            std::env::var("S3_ACCESS_KEY").map_err(|_| anyhow!("缺少 S3_ACCESS_KEY 环境变量"))?;
        let secret_key =
            std::env::var("S3_SECRET_KEY").map_err(|_| anyhow!("缺少 S3_SECRET_KEY 环境变量"))?;
        let bucket = std::env::var("S3_BUCKET").map_err(|_| anyhow!("缺少 S3_BUCKET 环境变量"))?;
        let public_base_url = std::env::var("S3_PUBLIC_URL")
            .unwrap_or_else(|_| format!("{}/{}", endpoint.trim_end_matches('/'), bucket));

        Ok(Self {
            endpoint,
            region,
            access_key,
            secret_key,
            bucket,
            public_base_url,
        })
    }
}

// ======================== S3 错误 ========================

/// S3 操作错误
#[derive(Debug, thiserror::Error)]
pub enum S3Error {
    #[error("S3 操作失败: {0}")]
    OperationFailed(String),

    #[error("配置错误: {0}")]
    ConfigError(String),

    #[error("AWS SDK 错误: {0}")]
    AwsError(#[from] aws_sdk_s3::Error),
}

// ======================== S3 客户端 ========================

/// S3 客户端，封装所有 S3 协议操作
pub struct S3Client {
    client: S3NativeClient,
    config: S3Config,
}

impl S3Client {
    /// 从环境配置创建
    pub async fn from_env() -> Result<Self, S3Error> {
        let config = S3Config::from_env().map_err(|e| S3Error::ConfigError(e.to_string()))?;
        Self::new(config).await
    }

    /// 从自定义配置创建
    pub async fn new(config: S3Config) -> Result<Self, S3Error> {
        let credentials = Credentials::new(
            &config.access_key,
            &config.secret_key,
            None,
            None,
            "assets-platform",
        );

        // 直接构造 S3 Config，跳过 AWS 默认凭证链（避免 ~/.aws/credentials 未找到警告）
        // TokioSleep 由 SDK 在 Tokio 运行时下自动配置，无需显式指定
        let s3_config = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(config.region.clone()))
            .endpoint_url(&config.endpoint)
            .credentials_provider(credentials)
            .build();

        let client = S3NativeClient::from_conf(s3_config);

        Ok(Self { client, config })
    }

    /// 获取 S3 配置引用
    pub fn config(&self) -> &S3Config {
        &self.config
    }

    // ---------- Multipart Upload ----------

    /// 创建分片上传，返回 S3 UploadId
    pub async fn create_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        mime_type: &str,
    ) -> Result<String, S3Error> {
        let resp = self
            .client
            .create_multipart_upload()
            .bucket(bucket)
            .key(key)
            .content_type(mime_type)
            .send()
            .await
            .map_err(|e| S3Error::AwsError(aws_sdk_s3::Error::from(e)))?;

        resp.upload_id
            .ok_or_else(|| S3Error::OperationFailed("S3 未返回 upload_id".to_string()))
    }

    /// 生成分片上传的 Presigned URL（前端直传用，默认 1 小时有效期）
    pub async fn presign_upload_part(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        part_number: i32,
    ) -> Result<String, S3Error> {
        use aws_sdk_s3::presigning::PresigningConfig;
        use std::time::SystemTime;

        let presign_config = PresigningConfig::builder()
            .expires_in(Duration::from_secs(3600)) // 1 小时
            .start_time(SystemTime::now())
            .build()
            .map_err(|e| S3Error::OperationFailed(format!("Presign 配置错误: {}", e)))?;

        let url = self
            .client
            .upload_part()
            .bucket(bucket)
            .key(key)
            .upload_id(upload_id)
            .part_number(part_number)
            .presigned(presign_config)
            .await
            .map_err(|e| S3Error::OperationFailed(format!("生成 Presigned URL 失败: {}", e)))?;

        Ok(url.uri().to_string())
    }

    /// 批量生成所有分片的 Presigned URL
    pub async fn presign_upload_parts(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        total_parts: i32,
    ) -> Result<Vec<String>, S3Error> {
        let mut urls = Vec::with_capacity(total_parts as usize);
        for part_number in 1..=total_parts {
            let url = self
                .presign_upload_part(bucket, key, upload_id, part_number)
                .await?;
            urls.push(url);
        }
        Ok(urls)
    }

    /// 完成分片合并
    pub async fn complete_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        parts: &[CompletedPart],
    ) -> Result<String, S3Error> {
        let resp = self
            .client
            .complete_multipart_upload()
            .bucket(bucket)
            .key(key)
            .upload_id(upload_id)
            .multipart_upload(
                aws_sdk_s3::types::CompletedMultipartUpload::builder()
                    .set_parts(Some(parts.to_vec()))
                    .build(),
            )
            .send()
            .await
            .map_err(|e| S3Error::AwsError(aws_sdk_s3::Error::from(e)))?;

        Ok(resp.e_tag.unwrap_or_default())
    }

    /// 取消分片上传
    pub async fn abort_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
    ) -> Result<(), S3Error> {
        self.client
            .abort_multipart_upload()
            .bucket(bucket)
            .key(key)
            .upload_id(upload_id)
            .send()
            .await
            .map_err(|e| S3Error::AwsError(aws_sdk_s3::Error::from(e)))?;

        Ok(())
    }

    // ---------- 对象操作 ----------

    /// 生成文件下载的 Presigned URL
    pub async fn presign_get_object(
        &self,
        bucket: &str,
        key: &str,
        expires_in: Duration,
    ) -> Result<String, S3Error> {
        use aws_sdk_s3::presigning::PresigningConfig;
        use std::time::SystemTime;

        let presign_config = PresigningConfig::builder()
            .expires_in(expires_in)
            .start_time(SystemTime::now())
            .build()
            .map_err(|e| S3Error::OperationFailed(format!("Presign 配置错误: {}", e)))?;

        let url = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .presigned(presign_config)
            .await
            .map_err(|e| S3Error::OperationFailed(format!("生成下载 Presigned URL 失败: {}", e)))?;

        Ok(url.uri().to_string())
    }

    /// 删除对象
    pub async fn delete_object(&self, bucket: &str, key: &str) -> Result<(), S3Error> {
        self.client
            .delete_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| S3Error::AwsError(aws_sdk_s3::Error::from(e)))?;

        Ok(())
    }

    /// 批量删除对象
    pub async fn delete_objects(&self, bucket: &str, keys: &[String]) -> Result<(), S3Error> {
        if keys.is_empty() {
            return Ok(());
        }

        let objects: Vec<aws_sdk_s3::types::ObjectIdentifier> = keys
            .iter()
            .map(|k| {
                aws_sdk_s3::types::ObjectIdentifier::builder()
                    .key(k)
                    .build()
                    .expect("构建 ObjectIdentifier 失败")
            })
            .collect();

        self.client
            .delete_objects()
            .bucket(bucket)
            .delete(
                aws_sdk_s3::types::Delete::builder()
                    .set_objects(Some(objects))
                    .build()
                    .expect("构建 Delete 请求失败"),
            )
            .send()
            .await
            .map_err(|e| S3Error::AwsError(aws_sdk_s3::Error::from(e)))?;

        Ok(())
    }

    /// 列出对象
    pub async fn list_objects(&self, bucket: &str, prefix: &str) -> Result<Vec<String>, S3Error> {
        let mut keys = Vec::new();
        let mut continuation_token: Option<String> = None;

        loop {
            let mut req = self.client.list_objects_v2().bucket(bucket).prefix(prefix);
            if let Some(token) = continuation_token.take() {
                req = req.continuation_token(&token);
            }

            let resp = req
                .send()
                .await
                .map_err(|e| S3Error::AwsError(aws_sdk_s3::Error::from(e)))?;
            if let Some(contents) = resp.contents {
                for obj in contents {
                    if let Some(key) = obj.key {
                        keys.push(key);
                    }
                }
            }

            if resp.is_truncated == Some(true) {
                continuation_token = resp.next_continuation_token;
            } else {
                break;
            }
        }

        Ok(keys)
    }

    /// 检查对象是否存在
    pub async fn object_exists(&self, bucket: &str, key: &str) -> Result<bool, S3Error> {
        match self
            .client
            .head_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| S3Error::AwsError(aws_sdk_s3::Error::from(e)))
        {
            Ok(_) => Ok(true),
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("NoSuchKey") || err_str.contains("NotFound") {
                    Ok(false)
                } else {
                    Err(e)
                }
            }
        }
    }

    /// 获取对象元数据
    pub async fn head_object(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<aws_sdk_s3::operation::head_object::HeadObjectOutput, S3Error> {
        let resp = self
            .client
            .head_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| S3Error::AwsError(aws_sdk_s3::Error::from(e)))?;
        Ok(resp)
    }
}
