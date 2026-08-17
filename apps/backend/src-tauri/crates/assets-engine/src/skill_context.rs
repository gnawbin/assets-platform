use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Skill 执行上下文（从 Rust 传入 Python）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillContext {
    pub input_text: String,
    pub config: HashMap<String, serde_json::Value>,
    pub user_id: i64,
    pub tenant_id: i64,
    pub document_id: Option<String>,
    pub cursor_position: Option<usize>,
}

impl SkillContext {
    pub fn new(
        input_text: String,
        config: HashMap<String, serde_json::Value>,
        user_id: i64,
        tenant_id: i64,
    ) -> Self {
        SkillContext {
            input_text,
            config,
            user_id,
            tenant_id,
            document_id: None,
            cursor_position: None,
        }
    }

    pub fn with_document(mut self, document_id: String) -> Self {
        self.document_id = Some(document_id);
        self
    }

    pub fn with_cursor(mut self, position: usize) -> Self {
        self.cursor_position = Some(position);
        self
    }
}

/// Skill 执行结果（从 Python 返回 Rust）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillResult {
    pub output: String,
    pub output_type: String,
    pub position: String,
    pub metadata: Option<HashMap<String, String>>,
}

impl SkillResult {
    pub fn new(output: String, output_type: &str, position: &str) -> Self {
        SkillResult {
            output,
            output_type: output_type.to_string(),
            position: position.to_string(),
            metadata: None,
        }
    }

    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = Some(metadata);
        self
    }
}
