use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Skill 类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SkillType {
    Builtin,
    Custom,
}

/// Skill 元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMeta {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub version: String,
    pub author: String,
    pub skill_type: SkillType,
    pub config_schema: serde_json::Value,
    pub file_path: String,
}

/// Skill 注册表
pub struct SkillRegistry {
    skills: HashMap<String, SkillMeta>,
}

impl SkillRegistry {
    /// 创建空的 Skill 注册表
    pub fn new() -> Self {
        let mut registry = SkillRegistry {
            skills: HashMap::new(),
        };

        // 注册内置 Skill
        registry.register_builtin_skills();

        registry
    }

    /// 注册内置 Skill
    fn register_builtin_skills(&mut self) {
        let builtins = vec![
            SkillMeta {
                id: "rag-qa".into(),
                name: "RAG 问答".into(),
                description: "基于知识库回答选中文本中的问题".into(),
                icon: "🤖".into(),
                version: "1.0.0".into(),
                author: "system".into(),
                skill_type: SkillType::Builtin,
                config_schema: serde_json::json!({
                    "top_k": {"type": "int", "default": 5, "description": "检索数量"},
                    "min_score": {"type": "float", "default": 0.7, "description": "最低相似度"},
                    "include_sources": {"type": "bool", "default": true, "description": "显示来源"},
                }),
                file_path: "skills/builtin/rag_qa.py".into(),
            },
            SkillMeta {
                id: "summarize".into(),
                name: "生成摘要".into(),
                description: "为选中文本生成摘要".into(),
                icon: "📝".into(),
                version: "1.0.0".into(),
                author: "system".into(),
                skill_type: SkillType::Builtin,
                config_schema: serde_json::json!({
                    "max_length": {"type": "int", "default": 200, "description": "摘要最大长度"},
                }),
                file_path: "skills/builtin/summarize.py".into(),
            },
            SkillMeta {
                id: "translate-en".into(),
                name: "翻译成英文".into(),
                description: "将选中文本翻译为英文".into(),
                icon: "🌐".into(),
                version: "1.0.0".into(),
                author: "system".into(),
                skill_type: SkillType::Builtin,
                config_schema: serde_json::json!({}),
                file_path: "skills/builtin/translate.py".into(),
            },
            SkillMeta {
                id: "translate-zh".into(),
                name: "翻译成中文".into(),
                description: "将选中文本翻译为中文".into(),
                icon: "🌐".into(),
                version: "1.0.0".into(),
                author: "system".into(),
                skill_type: SkillType::Builtin,
                config_schema: serde_json::json!({}),
                file_path: "skills/builtin/translate.py".into(),
            },
            SkillMeta {
                id: "discover-relations".into(),
                name: "发现关联".into(),
                description: "在知识图谱中查找关联节点".into(),
                icon: "🔗".into(),
                version: "1.0.0".into(),
                author: "system".into(),
                skill_type: SkillType::Builtin,
                config_schema: serde_json::json!({
                    "threshold": {"type": "float", "default": 0.7, "description": "关联阈值"},
                }),
                file_path: "skills/builtin/discover_relations.py".into(),
            },
            SkillMeta {
                id: "extract-table".into(),
                name: "提取表格".into(),
                description: "从文本中提取结构化表格数据".into(),
                icon: "📊".into(),
                version: "1.0.0".into(),
                author: "system".into(),
                skill_type: SkillType::Builtin,
                config_schema: serde_json::json!({}),
                file_path: "skills/builtin/extract_table.py".into(),
            },
            SkillMeta {
                id: "auto-tag".into(),
                name: "自动打标签".into(),
                description: "为文档自动生成标签".into(),
                icon: "🏷️".into(),
                version: "1.0.0".into(),
                author: "system".into(),
                skill_type: SkillType::Builtin,
                config_schema: serde_json::json!({
                    "max_tags": {"type": "int", "default": 5, "description": "最大标签数"},
                }),
                file_path: "skills/builtin/auto_tag.py".into(),
            },
            SkillMeta {
                id: "polish-writing".into(),
                name: "优化文案".into(),
                description: "优化选中文本的表达和语法".into(),
                icon: "✏️".into(),
                version: "1.0.0".into(),
                author: "system".into(),
                skill_type: SkillType::Builtin,
                config_schema: serde_json::json!({
                    "style": {"type": "string", "default": "formal", "description": "风格: formal/casual/professional"},
                }),
                file_path: "skills/builtin/polish_writing.py".into(),
            },
            SkillMeta {
                id: "code-review".into(),
                name: "代码审查".into(),
                description: "审查选中代码的质量和安全".into(),
                icon: "🔍".into(),
                version: "1.0.0".into(),
                author: "system".into(),
                skill_type: SkillType::Builtin,
                config_schema: serde_json::json!({
                    "language": {"type": "string", "default": "auto", "description": "编程语言"},
                }),
                file_path: "skills/builtin/code_review.py".into(),
            },
            SkillMeta {
                id: "doc-parse".into(),
                name: "文档解析".into(),
                description: "解析 PDF/DOCX 为 Markdown".into(),
                icon: "📄".into(),
                version: "1.0.0".into(),
                author: "system".into(),
                skill_type: SkillType::Builtin,
                config_schema: serde_json::json!({}),
                file_path: "skills/builtin/doc_parse.py".into(),
            },
            SkillMeta {
                id: "ocr-image".into(),
                name: "图片 OCR".into(),
                description: "识别图片中的文字".into(),
                icon: "🖼️".into(),
                version: "1.0.0".into(),
                author: "system".into(),
                skill_type: SkillType::Builtin,
                config_schema: serde_json::json!({}),
                file_path: "skills/builtin/ocr_image.py".into(),
            },
            SkillMeta {
                id: "graphrag-global".into(),
                name: "图谱全局搜索".into(),
                description: "跨社区全局知识检索".into(),
                icon: "🕸️".into(),
                version: "1.0.0".into(),
                author: "system".into(),
                skill_type: SkillType::Builtin,
                config_schema: serde_json::json!({
                    "top_k": {"type": "int", "default": 10, "description": "返回结果数"},
                }),
                file_path: "skills/builtin/graphrag_pipeline.py".into(),
            },
            SkillMeta {
                id: "graphrag-local".into(),
                name: "图谱局部搜索".into(),
                description: "指定实体局部知识检索".into(),
                icon: "🔍".into(),
                version: "1.0.0".into(),
                author: "system".into(),
                skill_type: SkillType::Builtin,
                config_schema: serde_json::json!({
                    "entity_ids": {"type": "array", "default": [], "description": "实体ID列表"},
                }),
                file_path: "skills/builtin/graphrag_pipeline.py".into(),
            },
            SkillMeta {
                id: "asset-sync".into(),
                name: "资产同步".into(),
                description: "将资产信息同步为知识条目".into(),
                icon: "🔄".into(),
                version: "1.0.0".into(),
                author: "system".into(),
                skill_type: SkillType::Builtin,
                config_schema: serde_json::json!({
                    "asset_type": {"type": "string", "default": "all", "description": "资产类型: all/hardware/intangible"},
                }),
                file_path: "skills/builtin/asset_sync.py".into(),
            },
            SkillMeta {
                id: "batch-embed".into(),
                name: "批量向量化".into(),
                description: "对选中内容批量生成向量".into(),
                icon: "⚡".into(),
                version: "1.0.0".into(),
                author: "system".into(),
                skill_type: SkillType::Builtin,
                config_schema: serde_json::json!({}),
                file_path: "skills/builtin/batch_embed.py".into(),
            },
        ];

        for skill in builtins {
            self.skills.insert(skill.id.clone(), skill);
        }
    }

    /// 获取所有 Skill 列表
    pub fn list_skills(&self) -> Vec<&SkillMeta> {
        self.skills.values().collect()
    }

    /// 根据 ID 获取 Skill
    pub fn get_skill(&self, id: &str) -> Option<&SkillMeta> {
        self.skills.get(id)
    }

    /// 注册自定义 Skill
    pub fn register_custom_skill(&mut self, meta: SkillMeta) {
        let mut meta = meta;
        meta.skill_type = SkillType::Custom;
        self.skills.insert(meta.id.clone(), meta);
    }

    /// 移除 Skill
    pub fn unregister_skill(&mut self, id: &str) -> bool {
        self.skills.remove(id).is_some()
    }

    /// 获取 Skill 数量
    pub fn skill_count(&self) -> usize {
        self.skills.len()
    }
}
