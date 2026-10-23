//! 解析结果契约
//!
//! 字段与 Python 侧车 `models/parse_result.py` 的 `ParseResult` 严格一致，
//! 也与 `crates/service/src/doc_parser.rs` 现有的同名结构一致（该处将改为
//! `pub use parser::ParseResult`），保证 `local` / `remote` 两种后端对上层
//! 完全透明。

use serde::{Deserialize, Serialize};

/// 解析结果（与 doc-parser 的 ParseResult 对齐）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseResult {
    /// 原文件名
    pub file_name: String,
    /// 文件类型: pdf / document / image / audio / video
    pub file_type: String,
    /// 提取或生成的纯文本内容
    pub raw_text: String,
    /// 需要 VLM 语义描述的本地图片路径（图片原图 / 视频抽帧）
    #[serde(default)]
    pub images: Vec<String>,
    /// 可选元数据
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

impl ParseResult {
    /// 构造基础结果（`images` 为空、`metadata` 为 `None`）
    pub fn new(
        file_name: impl Into<String>,
        file_type: impl Into<String>,
        raw_text: impl Into<String>,
    ) -> Self {
        Self {
            file_name: file_name.into(),
            file_type: file_type.into(),
            raw_text: raw_text.into(),
            images: Vec::new(),
            metadata: None,
        }
    }

    /// 设置元数据
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// 设置需要 VLM 描述的图片路径
    pub fn with_images(mut self, images: Vec<String>) -> Self {
        self.images = images;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serde_field_names_match_python_contract() {
        let result = ParseResult::new("a.pdf", "pdf", "hello");
        let json = serde_json::to_value(&result).expect("❌ 序列化失败");
        assert_eq!(json["file_name"], "a.pdf");
        assert_eq!(json["file_type"], "pdf");
        assert_eq!(json["raw_text"], "hello");
        assert_eq!(json["images"], serde_json::json!([]));
        assert!(json["metadata"].is_null(), "❌ metadata 缺省应为 null");
    }

    #[test]
    fn test_deserialize_allows_missing_optional_fields() {
        let json = serde_json::json!({
            "file_name": "b.docx",
            "file_type": "document",
            "raw_text": ""
        });
        let result: ParseResult = serde_json::from_value(json).expect("❌ 反序列化失败");
        assert!(result.images.is_empty());
        assert!(result.metadata.is_none());
    }
}
