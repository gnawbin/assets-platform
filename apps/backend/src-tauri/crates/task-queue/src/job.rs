//! 任务契约（payload）
//!
//! 契约演进规则见设计文档 §7.6：**只增不改**，新增字段一律 `#[serde(default)]`，
//! worker 必须能反序列化 N-1 版本的旧任务（线上 `apalis.jobs` 可能积压）。
//!
//! 安全约束（设计文档 §16.1）：
//! - `storage_url` 只放对象存储引用（`s3://bucket/key`），禁止本机绝对路径与长期凭证；
//! - `tenant` 必须由服务端从鉴权上下文写入，绝不采信请求体里的同名字段。

use serde::{Deserialize, Serialize};

use crate::error::TaskError;

/// 文档/图片解析队列
pub const QUEUE_PARSE: &str = "parse";
/// 视频/音频解析队列（重任务隔离，避免饿死短任务）
pub const QUEUE_PARSE_VIDEO: &str = "parse-video";
/// 分块 + 向量化队列
pub const QUEUE_VECTORIZE: &str = "vectorize";

/// 当前契约版本
pub const CONTRACT_VERSION: u8 = 1;

/// 契约版本的 serde 默认值
const fn default_version() -> u8 {
    CONTRACT_VERSION
}

/// 文件类型（路由与资源画像的依据，见设计文档 §15.2）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocType {
    /// PDF
    #[default]
    Pdf,
    /// Office：doc / docx / xls / xlsx
    Office,
    /// 图片：jpg / jpeg / png / gif / bmp / webp / tiff
    Image,
    /// 音频：mp3 / wav / ogg / flac / m4a / aac
    Audio,
    /// 视频：mp4 / avi / mov / mkv / wmv / flv
    Video,
}

impl DocType {
    /// 扩展名 → 文件类型（大小写 / 前导点容错），与 `parser::SUPPORTED_FORMATS` 保持一致
    #[must_use]
    pub fn from_ext(ext: &str) -> Option<Self> {
        match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "pdf" => Some(Self::Pdf),
            "doc" | "docx" | "xls" | "xlsx" => Some(Self::Office),
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "tiff" => Some(Self::Image),
            "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" => Some(Self::Audio),
            "mp4" | "avi" | "mov" | "mkv" | "wmv" | "flv" => Some(Self::Video),
            _ => None,
        }
    }

    /// 日志 / 指标用的字符串表示
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Office => "office",
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
        }
    }

    /// 是否重任务（音频 / 视频：模型推理 + ffmpeg，必须单独队列）
    #[must_use]
    pub const fn is_heavy(self) -> bool {
        matches!(self, Self::Audio | Self::Video)
    }

    /// 该类型应进入的队列
    #[must_use]
    pub const fn queue(self) -> &'static str {
        if self.is_heavy() {
            QUEUE_PARSE_VIDEO
        } else {
            QUEUE_PARSE
        }
    }
}

/// 解析任务
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseTask {
    /// 租户 schema（worker 内 `database::with_schema` 的唯一依据）
    pub tenant: String,
    /// 业务主键：知识资产 id
    pub asset_id: i64,
    /// 文件类型（缺省按扩展名推断，见 [`ParseTask::new`]）
    #[serde(default)]
    pub doc_type: DocType,
    /// 对象存储引用：`s3://bucket/key`（桌面本地调试可用 `file://` 或绝对路径）
    pub storage_url: String,
    /// 原文件名（仅用于日志与结果元数据，**不用于拼接本地路径**）
    pub file_name: String,
    /// 任务 id（生产者生成，同时作为 apalis 任务 id；worker 用它回写 `task_ticket`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// 资产标题（向量化时的 `title` 列）
    #[serde(default)]
    pub title: String,
    /// OKF 类型（向量化时的 `okf_type` 列，空串表示不填）
    #[serde(default)]
    pub okf_type: String,
    /// 标签
    #[serde(default)]
    pub tags: Vec<String>,
    /// 知识树节点（向量化分片的 `tree_node_id`）
    #[serde(default)]
    pub tree_node_id: Option<i64>,
    /// 解析选项：语义沿用 doc-parser `/parse` 的 `options`（`skip_index` / `frame_interval` 等）
    #[serde(default)]
    pub options: serde_json::Value,
    /// 文件内容 `md5`（用于幂等键；缺省时退化为 `tenant` + `asset_id`）
    #[serde(default)]
    pub file_md5: Option<String>,
    /// 幂等键（缺省由 [`ParseTask::idempotency_key`] 推导）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
    /// 调用链追踪 id
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    /// 契约版本
    #[serde(default = "default_version")]
    pub version: u8,
}

impl ParseTask {
    /// 构造解析任务：按扩展名推断 `doc_type`（无法识别时回落 `Pdf`，由 [`Self::validate`] 兜住）
    #[must_use]
    pub fn new(
        tenant: impl Into<String>,
        asset_id: i64,
        storage_url: impl Into<String>,
        file_name: impl Into<String>,
    ) -> Self {
        let file_name = file_name.into();
        let ext = std::path::Path::new(&file_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        Self {
            tenant: tenant.into(),
            asset_id,
            doc_type: DocType::from_ext(ext).unwrap_or_default(),
            storage_url: storage_url.into(),
            file_name,
            task_id: None,
            title: String::new(),
            okf_type: String::new(),
            tags: Vec::new(),
            tree_node_id: None,
            options: serde_json::Value::Null,
            file_md5: None,
            dedupe_key: None,
            trace_id: None,
            version: CONTRACT_VERSION,
        }
    }

    /// 任务应进入的队列
    #[must_use]
    pub const fn queue(&self) -> &'static str {
        self.doc_type.queue()
    }

    /// 解析完成后是否**不**做向量化（沿用 doc-parser `skip_index` 的缺省语义：默认跳过）
    #[must_use]
    pub fn skip_index(&self) -> bool {
        self.options
            .get("skip_index")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true)
    }

    /// 幂等键：优先使用显式 `dedupe_key`，否则由 `tenant + asset_id + md5` 推导
    #[must_use]
    pub fn idempotency_key(&self) -> String {
        if let Some(key) = &self.dedupe_key {
            return key.clone();
        }
        format!(
            "parse:{}:{}:{}",
            self.tenant,
            self.asset_id,
            self.file_md5.as_deref().unwrap_or("-")
        )
    }

    /// 入队前校验（失败即 [`TaskError::Invalid`]，不重试）
    pub fn validate(&self) -> Result<(), TaskError> {
        if self.tenant.trim().is_empty() {
            return Err(TaskError::Invalid("tenant 为空".to_string()));
        }
        if self.asset_id <= 0 {
            return Err(TaskError::Invalid(format!(
                "asset_id 非法: {}",
                self.asset_id
            )));
        }
        validate_storage_url(&self.storage_url)
    }
}

/// 向量化任务
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorizeTask {
    /// 租户 schema
    pub tenant: String,
    /// 业务主键：知识资产 id
    pub asset_id: i64,
    /// 待向量化文本（超过 64KB 请改为走 `result_ref`，见设计文档 §15.3）
    pub text: String,
    /// 任务 id（生产者生成，worker 用它回写 `task_ticket`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// 资产标题
    #[serde(default)]
    pub title: String,
    /// OKF 类型
    #[serde(default)]
    pub okf_type: String,
    /// 标签
    #[serde(default)]
    pub tags: Vec<String>,
    /// 知识树节点
    #[serde(default)]
    pub tree_node_id: Option<i64>,
    /// 幂等键
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
    /// 调用链追踪 id
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    /// 契约版本
    #[serde(default = "default_version")]
    pub version: u8,
}

impl VectorizeTask {
    /// 队列名（固定 [`QUEUE_VECTORIZE`]）
    #[must_use]
    pub const fn queue(&self) -> &'static str {
        QUEUE_VECTORIZE
    }

    /// 幂等键
    #[must_use]
    pub fn idempotency_key(&self) -> String {
        if let Some(key) = &self.dedupe_key {
            return key.clone();
        }
        format!("vectorize:{}:{}", self.tenant, self.asset_id)
    }

    /// 入队前校验
    pub fn validate(&self) -> Result<(), TaskError> {
        if self.tenant.trim().is_empty() {
            return Err(TaskError::Invalid("tenant 为空".to_string()));
        }
        if self.asset_id <= 0 {
            return Err(TaskError::Invalid(format!(
                "asset_id 非法: {}",
                self.asset_id
            )));
        }
        if self.text.trim().is_empty() {
            return Err(TaskError::Invalid("待向量化文本为空".to_string()));
        }
        Ok(())
    }
}

/// 存储引用校验：禁止空值、路径穿越与不受支持的形式
fn validate_storage_url(url: &str) -> Result<(), TaskError> {
    let url = url.trim();
    if url.is_empty() {
        return Err(TaskError::Invalid("storage_url 为空".to_string()));
    }
    if url.contains("..") {
        return Err(TaskError::Invalid(format!("storage_url 含路径穿越: {url}")));
    }
    if url.starts_with("s3://") || url.starts_with("file://") {
        return Ok(());
    }
    // 桌面本地调试允许绝对路径；但必须存在，避免把无效引用排进队列
    let path = std::path::Path::new(url);
    if path.is_absolute() && path.is_file() {
        return Ok(());
    }
    Err(TaskError::Invalid(format!(
        "storage_url 只支持 s3:// / file:// / 已存在的本地绝对路径: {url}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doc_type_from_ext() {
        assert_eq!(DocType::from_ext("PDF"), Some(DocType::Pdf));
        assert_eq!(DocType::from_ext(".docx"), Some(DocType::Office));
        assert_eq!(DocType::from_ext("tiff"), Some(DocType::Image));
        assert_eq!(DocType::from_ext("m4a"), Some(DocType::Audio));
        assert_eq!(DocType::from_ext("mkv"), Some(DocType::Video));
        assert_eq!(DocType::from_ext("exe"), None);
    }

    #[test]
    fn test_queue_routing_isolates_heavy_tasks() {
        assert_eq!(DocType::Pdf.queue(), QUEUE_PARSE);
        assert_eq!(DocType::Office.queue(), QUEUE_PARSE);
        assert_eq!(DocType::Image.queue(), QUEUE_PARSE);
        assert_eq!(
            DocType::Video.queue(),
            QUEUE_PARSE_VIDEO,
            "❌ 视频必须与文档分队列，否则长任务会饿死短任务"
        );
        assert_eq!(DocType::Audio.queue(), QUEUE_PARSE_VIDEO);
    }

    #[test]
    fn test_new_infers_doc_type_from_file_name() {
        let task = ParseTask::new("tenant_a", 7, "s3://bucket/a.mp4", "a.mp4");
        assert_eq!(task.doc_type, DocType::Video);
        assert_eq!(task.queue(), QUEUE_PARSE_VIDEO);
        assert_eq!(task.version, CONTRACT_VERSION);
    }

    #[test]
    fn test_validate_rejects_bad_input() {
        let mut task = ParseTask::new("tenant_a", 7, "file:///tmp/a.pdf", "a.pdf");
        assert!(task.validate().is_ok());

        task.tenant = "  ".to_string();
        assert!(matches!(task.validate(), Err(TaskError::Invalid(_))));
        task.tenant = "tenant_a".to_string();

        task.storage_url = "s3://bucket/../etc/passwd".to_string();
        assert!(
            matches!(task.validate(), Err(TaskError::Invalid(_))),
            "❌ 应拒绝路径穿越"
        );

        task.storage_url = "http://example.com/a.pdf".to_string();
        assert!(
            matches!(task.validate(), Err(TaskError::Invalid(_))),
            "❌ 应拒绝不受支持的外链形式"
        );
    }

    #[test]
    fn test_asset_id_must_be_positive() {
        let task = ParseTask::new("tenant_a", 0, "s3://bucket/a.pdf", "a.pdf");
        assert!(matches!(task.validate(), Err(TaskError::Invalid(_))));
    }

    #[test]
    fn test_idempotency_key_prefers_explicit_value() {
        let mut task = ParseTask::new("tenant_a", 7, "s3://bucket/a.pdf", "a.pdf");
        assert_eq!(task.idempotency_key(), "parse:tenant_a:7:-");
        task.file_md5 = Some("abc123".to_string());
        assert_eq!(task.idempotency_key(), "parse:tenant_a:7:abc123");
        task.dedupe_key = Some("custom".to_string());
        assert_eq!(task.idempotency_key(), "custom");
    }

    #[test]
    fn test_skip_index_defaults_to_true() {
        let mut task = ParseTask::new("tenant_a", 7, "s3://bucket/a.pdf", "a.pdf");
        assert!(
            task.skip_index(),
            "❌ 缺省应与 doc-parser 一致（跳过向量化）"
        );

        task.options = serde_json::json!({ "skip_index": false });
        assert!(!task.skip_index());
    }

    #[test]
    fn test_deserialize_old_payload_with_new_fields_missing() {
        // N-1 版本任务：只有 tenant / asset_id / storage_url / file_name
        let json = serde_json::json!({
            "tenant": "tenant_a",
            "asset_id": 7,
            "storage_url": "s3://bucket/a.pdf",
            "file_name": "a.pdf",
        });
        let task: ParseTask = serde_json::from_value(json).expect("❌ 旧任务必须可反序列化");
        assert_eq!(task.doc_type, DocType::Pdf);
        assert_eq!(task.version, CONTRACT_VERSION);
        assert!(task.tags.is_empty());
        assert!(task.options.is_null());
        assert!(task.validate().is_ok());
    }

    #[test]
    fn test_vectorize_task_contract() {
        let task = VectorizeTask {
            tenant: "tenant_a".to_string(),
            asset_id: 7,
            text: "hello".to_string(),
            task_id: None,
            title: "t".to_string(),
            okf_type: "raw_source".to_string(),
            tags: vec!["x".to_string()],
            tree_node_id: Some(3),
            dedupe_key: None,
            trace_id: None,
            version: CONTRACT_VERSION,
        };
        assert_eq!(task.queue(), QUEUE_VECTORIZE);
        assert_eq!(task.idempotency_key(), "vectorize:tenant_a:7");
        assert!(task.validate().is_ok());

        let empty = VectorizeTask {
            text: "   ".to_string(),
            ..task
        };
        assert!(matches!(empty.validate(), Err(TaskError::Invalid(_))));
    }
}
