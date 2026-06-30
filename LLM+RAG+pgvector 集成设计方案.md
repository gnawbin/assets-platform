# LLM Gateway + RAG + pgvector 集成设计方案

> 为 assets-plateform 注入 AI 能力，参考 OpenKnowledge 架构
> 零破坏性增量改造，不影响现有资产管理和 OKF 知识体系

---

## 一、设计原则

| 原则 | 说明 |
|------|------|
| ✅ 不修改现有业务表 | `knowledge_asset` 只增字段，不改结构 |
| ✅ 不修改现有 Service | 新增 `llm/`、`rag_service.rs`，不碰已有代码 |
| ✅ 不修改现有 Command | 新增 Tauri Command，保持独立 |
| ✅ 前端增量开发 | 对话页面、RAG 设置页面为独立组件 |
| ✅ 多提供商可切换 | 用户可自由选择 OpenAI / Ollama / 阿里云等 |

---

## 二、总体架构

```
┌─────────────────────────────────────────────────────┐
│                     前端层                           │
│  ┌─────────┐  ┌──────────┐  ┌───────────────────┐  │
│  │ 知识库   │  │ RAG 对话 │  │ LLM 设置 (提供商) │  │
│  │ 页面     │  │ 页面     │  │ 页面              │  │
│  └────┬────┘  └────┬─────┘  └────────┬──────────┘  │
│       │            │                 │              │
│       └────────────┼─────────────────┘              │
│                    │ Tauri invoke                    │
└────────────────────┼────────────────────────────────┘
                     │
┌────────────────────┼────────────────────────────────┐
│              Rust 后端 (Tauri)                       │
│                                                     │
│  ┌──────────────────────────────────────────────┐   │
│  │              LLM 模块 (llm/)                  │   │
│  │  ┌────────────────────────────────────────┐  │   │
│  │  │         LLMProvider trait              │  │   │
│  │  │  ┌────────┐ ┌──────────┐ ┌──────────┐ │  │   │
│  │  │  │ chat() │ │embed()   │ │chat_stream│ │  │   │
│  │  │  └────────┘ └──────────┘ └──────────┘ │  │   │
│  │  └────────────────────────────────────────┘  │   │
│  │                                               │   │
│  │  ┌────────────┐ ┌────────────┐ ┌───────────┐ │   │
│  │  │ OpenAI     │ │ Ollama     │ │ Provider  │ │   │
│  │  │ Provider   │ │ Provider   │ │ Factory   │ │   │
│  │  └────────────┘ └────────────┘ └───────────┘ │   │
│  └──────────────────────────────────────────────┘   │
│                                                     │
│  ┌──────────────────────────────────────────────┐   │
│  │          RAG Service (service/)               │   │
│  │  ┌──────────────────┐  ┌──────────────────┐  │   │
│  │  │ 文档分块 + 向量化 │  │ pgvector 相似度   │  │   │
│  │  │ (embed_document) │  │ 搜索 (search)     │  │   │
│  │  └──────────────────┘  └──────────────────┘  │   │
│  │  ┌────────────────────────────────────────┐  │   │
│  │  │ 上下文组装 + LLM 问答 (query pipeline)  │  │   │
│  │  └────────────────────────────────────────┘  │   │
│  └──────────────────────────────────────────────┘   │
│                                                     │
│  ┌──────────────────────────────────────────────┐   │
│  │          PostgreSQL + pgvector                │   │
│  │  ┌────────────────┐  ┌────────────────────┐  │   │
│  │  │ knowledge_asset │  │ knowledge_tree     │  │   │
│  │  │ + embedding     │  │ (树形结构，不动)   │  │   │
│  │  └────────────────┘  └────────────────────┘  │   │
│  └──────────────────────────────────────────────┘   │
│                                                     │
│  ┌──────────────────────────────────────────────┐   │
│  │          Skill 引擎 (engine/)                  │   │
│  │  调用 LLMProvider → 执行 RAG / 摘要 / 翻译等   │   │
│  └──────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────┘
```

---

## 三、Rust LLM 模块设计

### 3.1 目录结构

```
src-tauri/src/llm/
├── mod.rs                   # 模块入口 + 重导出
├── config.rs                # 提供商配置结构体
├── types.rs                 # 公共类型定义
├── gateway.rs               # LLMProvider trait 定义
├── embedding.rs             # Embedding 辅助函数
├── providers/
│   ├── mod.rs
│   ├── openai.rs            # OpenAI 兼容提供商
│   ├── ollama.rs            # Ollama 本地提供商
│   └── factory.rs           # 提供商工厂
```

### 3.2 提供商配置

```rust
// config.rs

/// 提供商定义（对应 OpenKnowledge PROVIDERS 常量）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDefinition {
    pub id: String,        // "openai" | "anthropic" | "alibaba" | "zhipu" | "moonshot" | "ollama"
    pub name: String,      // "OpenAI" | "Anthropic" | "阿里云" | ...
    pub key_name: String,  // "OPENAI_API_KEY"
    pub default_base_url: String,
    pub supports_embedding: bool,
    pub embedding_models: Vec<String>,
}

/// 用户配置的提供商实例
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub api_key: String,
    pub base_url: String,
    pub chat_model: String,
    pub embedding_model: String,
}

/// 模型配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub context_window: u32,
}
```

### 3.3 核心 Trait

```rust
// gateway.rs
use async_trait::async_trait;
use futures::stream::BoxStream;

/// 聊天请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,  // "system" | "user" | "assistant"
    pub content: String,
}

/// 聊天响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// 流式聊天块
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChunk {
    pub content: String,
    pub done: bool,
}

/// LLM 提供商统一接口
#[async_trait]
pub trait LLMProvider: Send + Sync {
    /// 唯一标识
    fn provider_id(&self) -> &str;
    
    /// 聊天补全
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, String>;
    
    /// 生成嵌入向量
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String>;
    
    /// 流式聊天补全
    async fn chat_stream(&self, req: ChatRequest) -> Result<BoxStream<'static, ChatChunk>, String>;
}
```

### 3.4 OpenAI 兼容提供商实现

```rust
// providers/openai.rs

pub struct OpenAICompatibleProvider {
    config: ProviderConfig,
    client: reqwest::Client,
}

#[async_trait]
impl LLMProvider for OpenAICompatibleProvider {
    fn provider_id(&self) -> &str {
        &self.config.id
    }

    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, String> {
        let url = format!("{}/chat/completions", self.config.base_url.trim_end_matches('/'));
        
        let mut messages = vec![
            serde_json::json!({"role": "system", "content": "你是一个知识库助手，基于提供的知识回答问题。"})
        ];
        for msg in &req.messages {
            messages.push(serde_json::json!({
                "role": msg.role,
                "content": msg.content,
            }));
        }

        let body = serde_json::json!({
            "model": self.config.chat_model,
            "messages": messages,
            "temperature": req.temperature.unwrap_or(0.7),
            "max_tokens": req.max_tokens.unwrap_or(4096),
            "stream": false,
        });

        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP 请求失败: {}", e))?;

        let status = resp.status();
        let text = resp.text().await.map_err(|e| format!("读取响应失败: {}", e))?;

        if !status.is_success() {
            return Err(format!("API 返回错误 ({}): {}", status, text));
        }

        let data: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("解析响应失败: {}", e))?;

        let content = data["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let usage = data["usage"].as_object().map(|u| TokenUsage {
            prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
            total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
        });

        Ok(ChatResponse {
            content,
            model: data["model"].as_str().unwrap_or("").to_string(),
            usage,
        })
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let url = format!("{}/embeddings", self.config.base_url.trim_end_matches('/'));
        
        let body = serde_json::json!({
            "model": self.config.embedding_model,
            "input": texts,
        });

        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Embedding 请求失败: {}", e))?;

        let data: serde_json::Value = resp.json().await
            .map_err(|e| format!("解析 Embedding 响应失败: {}", e))?;

        let embeddings: Vec<Vec<f32>> = data["data"]
            .as_array()
            .ok_or("缺少 data 字段")?
            .iter()
            .map(|item| {
                item["embedding"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                    .collect()
            })
            .collect();

        Ok(embeddings)
    }

    async fn chat_stream(&self, req: ChatRequest) -> Result<BoxStream<'static, ChatChunk>, String> {
        // 实现 SSE 流式解析
        // 使用 reqwest::get 的 streaming 能力 + 解析 events
        todo!("流式实现需要 tokio_stream + 事件解析")
    }
}
```

### 3.5 Ollama 本地提供商

```rust
// providers/ollama.rs

pub struct OllamaProvider {
    base_url: String,
    client: reqwest::Client,
}

#[async_trait]
impl LLMProvider for OllamaProvider {
    fn provider_id(&self) -> &str {
        "ollama"
    }

    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, String> {
        let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));
        
        let body = serde_json::json!({
            "model": self.config.chat_model,
            "messages": req.messages.iter().map(|m| {
                serde_json::json!({"role": m.role, "content": m.content})
            }).collect::<Vec<_>>(),
            "stream": false,
        });

        let resp = self.client.post(&url).json(&body).send().await
            .map_err(|e| format!("Ollama 请求失败: {}", e))?;
        
        let data: serde_json::Value = resp.json().await
            .map_err(|e| format!("解析 Ollama 响应失败: {}", e))?;

        Ok(ChatResponse {
            content: data["message"]["content"].as_str().unwrap_or("").to_string(),
            model: data["model"].as_str().unwrap_or("").to_string(),
            usage: None,
        })
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let mut embeddings = Vec::new();
        for text in texts {
            let url = format!("{}/api/embeddings", self.base_url.trim_end_matches('/'));
            let body = serde_json::json!({
                "model": self.config.embedding_model,
                "prompt": text,
            });
            let resp = self.client.post(&url).json(&body).send().await
                .map_err(|e| format!("Ollama Embedding 请求失败: {}", e))?;
            let data: serde_json::Value = resp.json().await
                .map_err(|e| format!("解析 Ollama Embedding 响应失败: {}", e))?;
            embeddings.push(
                data["embedding"].as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                    .collect()
            );
        }
        Ok(embeddings)
    }

    async fn chat_stream(&self, req: ChatRequest) -> Result<BoxStream<'static, ChatChunk>, String> {
        todo!()
    }
}
```

### 3.6 提供商工厂

```rust
// providers/factory.rs

pub fn create_provider(config: &ProviderConfig, provider_def: &ProviderDefinition) -> Box<dyn LLMProvider> {
    match provider_def.id.as_str() {
        "openai" | "alibaba" | "zhipu" | "moonshot" | "deepseek" | "anthropic" => {
            Box::new(OpenAICompatibleProvider::new(config))
        }
        "ollama" => {
            Box::new(OllamaProvider::new(config))
        }
        other => panic!("不支持的提供商: {}", other),
    }
}

/// 所有支持的提供商定义
pub fn all_provider_definitions() -> Vec<ProviderDefinition> {
    vec![
        ProviderDefinition {
            id: "openai".into(),
            name: "OpenAI".into(),
            key_name: "OPENAI_API_KEY".into(),
            default_base_url: "https://api.openai.com/v1".into(),
            supports_embedding: true,
            embedding_models: vec!["text-embedding-3-small".into(), "text-embedding-3-large".into()],
        },
        ProviderDefinition {
            id: "alibaba".into(),
            name: "阿里云 (Qwen)".into(),
            key_name: "DASHSCOPE_API_KEY".into(),
            default_base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
            supports_embedding: true,
            embedding_models: vec!["text-embedding-v3".into()],
        },
        ProviderDefinition {
            id: "zhipu".into(),
            name: "智谱 AI".into(),
            key_name: "ZHIPU_API_KEY".into(),
            default_base_url: "https://open.bigmodel.cn/api/paas/v4".into(),
            supports_embedding: true,
            embedding_models: vec!["embedding-3".into()],
        },
        ProviderDefinition {
            id: "moonshot".into(),
            name: "Moonshot (Kimi)".into(),
            key_name: "MOONSHOT_API_KEY".into(),
            default_base_url: "https://api.moonshot.cn/v1".into(),
            supports_embedding: true,
            embedding_models: vec!["moonshot-embedding".into()],
        },
        ProviderDefinition {
            id: "ollama".into(),
            name: "Ollama (本地)".into(),
            key_name: "".into(),
            default_base_url: "http://localhost:11434".into(),
            supports_embedding: true,
            embedding_models: vec!["nomic-embed-text".into()],
        },
    ]
}
```

---

## 四、pgvector 集成方案

### 4.1 数据库变更

```sql
-- tenant_tables.sql 追加

-- 1. 启用 pgvector 扩展
CREATE EXTENSION IF NOT EXISTS vector;

-- 2. knowledge_asset 表增加 embedding 字段
--    text-embedding-3-small 维度 = 1536
ALTER TABLE {schema}.knowledge_asset
ADD COLUMN IF NOT EXISTS embedding vector(1536);

-- 3. 创建 IVFFlat 索引加速相似度搜索
CREATE INDEX IF NOT EXISTS idx_knowledge_asset_embedding
ON {schema}.knowledge_asset
USING ivfflat (embedding vector_cosine_ops)
WITH (lists = 100);
-- 说明：lists = sqrt(行数) 的近似值，>1M 行时用 sqrt(行数)
```

### 4.2 Rust 向量操作

```rust
// service/vec_service.rs

use crate::database;
use crate::database::models::KnowledgeAsset;

/// 获取当前 schema 前缀
fn schema_prefix() -> String {
    let schema = database::postgres::get_current_schema();
    format!("{}.", schema)
}

/// 将 Vec<f32> 转为 PostgreSQL vector 文本格式 "[x,y,z,...]"
fn vec_to_pg_string(embedding: &[f32]) -> String {
    let inner = embedding.iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", inner)
}

/// 保存嵌入向量到知识资产
pub async fn save_knowledge_asset_embedding(
    asset_id: i64,
    embedding: &[f32],
) -> Result<(), String> {
    let pool = database::get_write_pool()
        .map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = schema_prefix();
    let vec_str = vec_to_pg_string(embedding);

    let sql = format!(
        "UPDATE {}knowledge_asset SET embedding = $1::vector, updated_at = NOW() WHERE id = $2",
        prefix
    );

    sqlx::query(&sql)
        .bind(&vec_str)
        .bind(asset_id)
        .execute(&pool)
        .await
        .map_err(|e| format!("保存向量失败: {}", e))?;

    Ok(())
}

/// 向量相似度搜索（余弦距离，值越小越相似）
pub async fn search_similar_by_vector(
    query_embedding: &[f32],
    limit: i64,
    max_distance: f64,
    okf_type_filter: Option<&str>,
) -> Result<Vec<KnowledgeAsset>, String> {
    let pool = database::get_read_pool()
        .map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = schema_prefix();
    let vec_str = vec_to_pg_string(query_embedding);

    let mut sql = format!(
        r#"SELECT id, tree_node_id, title, content, content_html, okf_type, summary, source,
                  confidence, status, effective_at, expire_at, relation_ids, tags,
                  file_url, file_name, file_size, file_mime, file_md5, editor_mode,
                  created_by, created_at, updated_by, updated_at, deleted,
                  (embedding <=> $1::vector) AS distance
           FROM {}knowledge_asset
           WHERE embedding IS NOT NULL
             AND (deleted IS NULL OR deleted = 0)
             AND (embedding <=> $1::vector) < $2"#,
        prefix
    );

    if let Some(ot) = okf_type_filter {
        sql.push_str(&format!(" AND okf_type = '{}'", ot.replace('\'', "''")));
    }

    sql.push_str(" ORDER BY embedding <=> $1::vector LIMIT $3");

    // sqlx 不支持直接返回 distance 列到 KnowledgeAsset struct，
    // 所以用 query 手动映射
    let rows = sqlx::query(&sql)
        .bind(&vec_str)
        .bind(max_distance)
        .bind(limit)
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("向量搜索失败: {}", e))?;

    let mut results = Vec::new();
    for row in rows {
        use sqlx::Row;
        results.push(KnowledgeAsset {
            id: row.try_get("id").unwrap_or(0),
            tree_node_id: row.try_get("tree_node_id").unwrap_or(0),
            title: row.try_get("title").unwrap_or_default(),
            content: row.try_get("content").unwrap_or(None),
            content_html: row.try_get("content_html").unwrap_or(None),
            okf_type: row.try_get("okf_type").unwrap_or_default(),
            summary: row.try_get("summary").unwrap_or(None),
            source: row.try_get("source").unwrap_or(None),
            confidence: row.try_get("confidence").unwrap_or(None),
            status: row.try_get("status").unwrap_or_default(),
            effective_at: row.try_get("effective_at").unwrap_or(None),
            expire_at: row.try_get("expire_at").unwrap_or(None),
            relation_ids: row.try_get("relation_ids").unwrap_or(None),
            tags: row.try_get("tags").unwrap_or(None),
            file_url: row.try_get("file_url").unwrap_or(None),
            file_name: row.try_get("file_name").unwrap_or(None),
            file_size: row.try_get("file_size").unwrap_or(None),
            file_mime: row.try_get("file_mime").unwrap_or(None),
            file_md5: row.try_get("file_md5").unwrap_or(None),
            editor_mode: row.try_get("editor_mode").unwrap_or_default(),
            created_by: row.try_get("created_by").unwrap_or(None),
            created_at: row.try_get("created_at").unwrap_or(None),
            updated_by: row.try_get("updated_by").unwrap_or(None),
            updated_at: row.try_get("updated_at").unwrap_or(None),
            deleted: row.try_get("deleted").unwrap_or(0),
        });
    }

    Ok(results)
}
```

### 4.3 相似度搜索响应

```rust
/// 搜索响应（带距离分数）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultItem {
    pub asset: KnowledgeAsset,
    pub distance: f64,        // 余弦距离，0=完全匹配，>0.3 一般无关
    pub similarity: f64,      // 1 - distance，用于前端展示
}
```

---

## 五、RAG 问答流水线

### 5.1 文档处理流水线

```
用户上传文档
      │
      ▼
[现有] 文件上传 → S3 (复用 upload_routes.rs)
      │
      ▼
[新增] 创建 knowledge_tree 节点 (node_type = raw_file)
      │
      ▼
[新增] 创建 knowledge_asset 记录 (okf_type = raw_source)
      │
      ▼
[新增] 异步任务：
        1. 读取文件内容 (txt/md/pdf/docx)
        2. 文本分块 (chunk_size=512, overlap=64)
        3. 调用 LLMProvider::embed() 生成向量
        4. 调用 save_knowledge_asset_embedding() 存储
      │
      ▼
[新增] 更新 knowledge_asset.status = "completed"
```

### 5.2 文本分块策略

```rust
// service/chunk_service.rs

/// 文本分块配置
pub struct ChunkConfig {
    pub chunk_size: usize,   // 512 token 约 2000 字符
    pub chunk_overlap: usize, // 64 token 约 256 字符
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            chunk_size: 2000,
            chunk_overlap: 256,
        }
    }
}

/// 将长文本分割为块
pub fn chunk_text(text: &str, config: &ChunkConfig) -> Vec<String> {
    if text.len() <= config.chunk_size {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let end = (start + config.chunk_size).min(text.len());
        // 尝试在段落边界/句号处断开
        let chunk = &text[start..end];
        chunks.push(chunk.to_string());

        if end >= text.len() {
            break;
        }

        start = end.saturating_sub(config.chunk_overlap);
    }

    chunks
}
```

### 5.3 RAG Service 完整实现

```rust
// service/rag_service.rs

use crate::llm::{LLMProvider, ChatRequest, ChatMessage};
use super::vec_service;

/// RAG 问答结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RAGQueryResult {
    pub answer: String,
    pub sources: Vec<RAGSource>,
    pub model: String,
    pub usage: Option<TokenUsage>,
}

/// RAG 来源
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RAGSource {
    pub asset_id: i64,
    pub title: String,
    pub okf_type: String,
    pub summary: Option<String>,
    pub distance: f64,
}

/// RAG 问答服务
pub struct RAGService {
    provider: Box<dyn LLMProvider>,
}

impl RAGService {
    pub fn new(provider: Box<dyn LLMProvider>) -> Self {
        Self { provider }
    }

    /// 执行 RAG 问答
    pub async fn query(
        &self,
        question: &str,
        top_k: i64,
        max_distance: f64,
        okf_type_filter: Option<&str>,
    ) -> Result<RAGQueryResult, String> {
        // 1. 生成问题向量
        let embeddings = self.provider
            .embed(&[question.to_string()])
            .await?;

        if embeddings.is_empty() {
            return Err("生成问题向量失败".into());
        }

        let query_vec = &embeddings[0];

        // 2. 向量相似度搜索
        let similar_assets = vec_service::search_similar_by_vector(
            query_vec, top_k, max_distance, okf_type_filter
        ).await?;

        if similar_assets.is_empty() {
            return Ok(RAGQueryResult {
                answer: "未找到相关知识。".into(),
                sources: vec![],
                model: self.provider.provider_id().to_string(),
                usage: None,
            });
        }

        // 3. 组装上下文
        let mut sources = Vec::new();
        let mut context_parts = Vec::new();

        for asset in &similar_assets {
            context_parts.push(format!(
                "标题：{}\n类型：{}\n内容：{}",
                asset.title,
                asset.okf_type,
                asset.content.as_deref().unwrap_or("")
            ));

            sources.push(RAGSource {
                asset_id: asset.id,
                title: asset.title.clone(),
                okf_type: asset.okf_type.clone(),
                summary: asset.summary.clone(),
                distance: 0.0, // 实际距离需要在查询中带回
            });
        }

        let context = context_parts.join("\n\n---\n\n");

        // 4. 构造 System Prompt
        let system_prompt = format!(
            "你是一个专业的知识库助手。\n\
             请基于以下知识内容回答用户的问题。\n\
             - 如果知识不足以回答，请明确说明\n\
             - 引用知识来源时，标注对应的标题\n\
             - 回答使用中文\n\n\
             知识库内容：\n{}",
            context
        );

        // 5. LLM 问答
        let response = self.provider
            .chat(ChatRequest {
                messages: vec![
                    ChatMessage {
                        role: "system".into(),
                        content: system_prompt,
                    },
                    ChatMessage {
                        role: "user".into(),
                        content: question.to_string(),
                    },
                ],
                temperature: Some(0.3),
                max_tokens: Some(2048),
                stream: false,
            })
            .await?;

        Ok(RAGQueryResult {
            answer: response.content,
            sources,
            model: response.model,
            usage: response.usage,
        })
    }

    /// 对单个文档进行知识提取（用于自动标签/摘要）
    pub async fn extract_knowledge(
        &self,
        content: &str,
        title: &str,
    ) -> Result<ExtractResult, String> {
        let prompt = format!(
            "分析以下文本内容，提取关键知识信息。\n\
             标题：{}\n文本：{}\n\n\
             请以 JSON 格式返回：\n\
             {{\n\
               \"summary\": \"200字以内的摘要\",\n\
               \"tags\": [\"标签1\", \"标签2\"],\n\
               \"okf_type\": \"concept|fact|rule|param|process|case\"\n\
             }}",
            title, content
        );

        let response = self.provider
            .chat(ChatRequest {
                messages: vec![
                    ChatMessage {
                        role: "user".into(),
                        content: prompt,
                    },
                ],
                temperature: Some(0.2),
                max_tokens: Some(1024),
                stream: false,
            })
            .await?;

        // 解析 JSON 响应
        let extracted: ExtractResult = serde_json::from_str(&response.content)
            .unwrap_or(ExtractResult {
                summary: None,
                tags: vec![],
                okf_type: "raw_source".to_string(),
            });

        Ok(extracted)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractResult {
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub okf_type: String,
}
```

---

## 六、新增 Tauri Command

```rust
// commands/rag_commands.rs

use crate::llm::providers::factory::{create_provider, all_provider_definitions};
use crate::llm::config::ProviderConfig;
use crate::service::rag_service::{RAGService, RAGQueryResult};
use crate::service::vec_service;

/// 获取支持的提供商列表
#[tauri::command]
pub async fn get_llm_providers() -> Vec<ProviderDefinition> {
    all_provider_definitions()
}

/// 获取提供商配置
#[tauri::command]
pub async fn get_llm_config() -> Result<ProviderConfig, String> {
    // 从配置文件中读取当前激活的提供商配置
    let config = crate::config::get_llm_config().await
        .map_err(|e| format!("读取 LLM 配置失败: {}", e))?;
    Ok(config)
}

/// 保存提供商配置
#[tauri::command]
pub async fn save_llm_config(config: ProviderConfig) -> Result<(), String> {
    crate::config::save_llm_config(&config).await
        .map_err(|e| format!("保存 LLM 配置失败: {}", e))
}

/// RAG 问答
#[tauri::command]
pub async fn rag_query(
    question: String,
    top_k: Option<i64>,
    okf_type: Option<String>,
) -> Result<RAGQueryResult, String> {
    let config = crate::config::get_llm_config().await
        .map_err(|e| format!("读取 LLM 配置失败: {}", e))?;
    
    let provider_def = all_provider_definitions()
        .into_iter()
        .find(|p| p.id == config.id)
        .ok_or_else(|| format!("不支持的提供商: {}", config.id))?;

    let provider = create_provider(&config, &provider_def);
    let rag = RAGService::new(provider);

    rag.query(
        &question,
        top_k.unwrap_or(5),
        0.7,
        okf_type.as_deref(),
    ).await
}

/// 知识资产向量化（生成并保存 embedding）
#[tauri::command]
pub async fn embed_knowledge_asset(
    asset_id: String,
) -> Result<(), String> {
    let id: i64 = asset_id.parse()
        .map_err(|e| format!("无效的资产ID: {}", e))?;

    // 1. 读取知识资产内容
    let asset = crate::service::knowledge_asset_service::get_knowledge_asset(id).await?;
    let content = asset.content.ok_or("知识资产内容为空")?;

    // 2. 获取 LLM 提供商
    let config = crate::config::get_llm_config().await
        .map_err(|e| format!("读取 LLM 配置失败: {}", e))?;
    let provider_def = all_provider_definitions()
        .into_iter()
        .find(|p| p.id == config.id)
        .ok_or_else(|| format!("不支持的提供商: {}", config.id))?;
    let provider = create_provider(&config, &provider_def);

    // 3. 分块 + 向量化
    let chunks = crate::service::chunk_service::chunk_text(&content, &Default::default());
    let embeddings = provider.embed(&chunks).await?;

    // 4. 平均池化为单一向量
    let avg_embedding = if embeddings.len() == 1 {
        embeddings[0].clone()
    } else {
        let dim = embeddings[0].len();
        let mut avg = vec![0.0f32; dim];
        for emb in &embeddings {
            for (i, v) in emb.iter().enumerate() {
                avg[i] += v;
            }
        }
        let n = embeddings.len() as f32;
        for v in &mut avg {
            *v /= n;
        }
        avg
    };

    // 5. 存储向量
    vec_service::save_knowledge_asset_embedding(id, &avg_embedding).await
}

/// 向量相似度搜索
#[tauri::command]
pub async fn search_knowledge_similar(
    asset_id: Option<String>,
    text: Option<String>,
    limit: Option<i64>,
    max_distance: Option<f64>,
    okf_type: Option<String>,
) -> Result<Vec<KnowledgeAsset>, String> {
    let config = crate::config::get_llm_config().await
        .map_err(|e| format!("读取 LLM 配置失败: {}", e))?;
    let provider_def = all_provider_definitions()
        .into_iter()
        .find(|p| p.id == config.id)
        .ok_or_else(|| format!("不支持的提供商: {}", config.id))?;
    let provider = create_provider(&config, &provider_def);

    // 生成查询向量
    let query_text = match (asset_id, text) {
        (Some(id_str), _) => {
            // 以文搜文：使用已有资产的 embedding
            let id: i64 = id_str.parse().map_err(|e| format!("无效ID: {}", e))?;
            let asset = crate::service::knowledge_asset_service::get_knowledge_asset(id).await?;
            asset.content.unwrap_or(asset.title)
        }
        (None, Some(t)) => t,
        (None, None) => return Err("请提供 asset_id 或 text".into()),
    };

    let embeddings = provider.embed(&[query_text]).await?;
    if embeddings.is_empty() {
        return Err("生成查询向量失败".into());
    }

    let results = vec_service::search_similar_by_vector(
        &embeddings[0],
        limit.unwrap_or(10),
        max_distance.unwrap_or(0.8),
        okf_type.as_deref(),
    ).await?;

    Ok(results)
}
```

---

## 七、前端新增组件

### 7.1 目录结构

```
src/
└── components/
    ├── LLMSettings/              # [新增] LLM 提供商配置
    │   ├── index.tsx             # 提供商选择 + API Key 配置
    │   ├── ProviderSelect.tsx    # 提供商下拉
    │   └── ModelSelect.tsx       # 模型选择
    │
    ├── RAGChat/                  # [新增] RAG 对话组件
    │   ├── index.tsx             # 对话界面
    │   ├── ChatMessage.tsx       # 消息气泡
    │   ├── SourcePanel.tsx       # 知识来源面板
    │   └── types.ts              # 类型定义
    │
    └── pages/
        └── RAGPage.tsx           # [新增] RAG 对话主页面
```

### 7.2 前端类型定义

```typescript
// types/llm.ts

export interface ProviderDefinition {
  id: string;
  name: string;
  keyName: string;
  defaultBaseUrl: string;
  supportsEmbedding: boolean;
  embeddingModels: string[];
}

export interface ProviderConfig {
  id: string;
  apiKey: string;
  baseUrl: string;
  chatModel: string;
  embeddingModel: string;
}

export interface ModelConfig {
  id: string;
  name: string;
  provider: string;
  contextWindow: number;
}

export interface ChatMessage {
  role: 'system' | 'user' | 'assistant';
  content: string;
}

export interface RAGQueryResult {
  answer: string;
  sources: RAGSource[];
  model: string;
  usage?: {
    promptTokens: number;
    completionTokens: number;
    totalTokens: number;
  };
}

export interface RAGSource {
  assetId: string;
  title: string;
  okfType: string;
  summary: string | null;
  distance: number;
}
```

---

## 八、Skill 引擎集成

现有 Skill Registry 中的 RAG 相关 Skill 可以通过 LLMProvider 获取真实能力：

```rust
// engine/skill_context.rs — 扩展执行上下文

pub struct SkillExecutionContext {
    // 现有字段...
    
    // [新增] LLM 提供商
    pub llm_provider: Option<Box<dyn LLMProvider>>,
}

impl SkillExecutionContext {
    /// 执行 RAG 查询（供 Python Skill 脚本调用）
    pub async fn rag_query(&self, question: &str) -> Result<String, String> {
        let provider = self.llm_provider.as_ref()
            .ok_or("LLM 提供商未配置")?;
        
        // 获取当前知识树选中的节点上下文
        let context_assets = /* 从选中节点获取关联知识资产 */;
        
        // 构建 RAG prompt
        let prompt = format!("基于以下知识回答问题：\n\n{}\n\n问题：{}", 
            context_assets.iter()
                .map(|a| a.content.as_deref().unwrap_or(""))
                .collect::<Vec<_>>()
                .join("\n"),
            question
        );
        
        let response = provider.chat(ChatRequest {
            messages: vec![ChatMessage {
                role: "user".into(),
                content: prompt,
            }],
            temperature: Some(0.3),
            max_tokens: Some(2048),
            stream: false,
        }).await?;
        
        Ok(response.content)
    }
}
```

---

## 九、配置管理

### 9.1 配置文件格式

```toml
# .env.toml 新增 LLM 配置

[llm]
# 当前激活的提供商
active_provider = "openai"

[llm.openai]
api_key = "sk-xxx"
base_url = "https://api.openai.com/v1"
chat_model = "gpt-5-mini"
embedding_model = "text-embedding-3-small"

[llm.alibaba]
api_key = "sk-xxx"
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
chat_model = "qwen3-max"
embedding_model = "text-embedding-v3"

[llm.ollama]
api_key = ""
base_url = "http://localhost:11434"
chat_model = "qwen2.5:7b"
embedding_model = "nomic-embed-text"
```

### 9.2 Rust 配置加载

```rust
// config/llm_config.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMGlobalConfig {
    pub active_provider: String,
    openai: Option<ProviderConfig>,
    alibaba: Option<ProviderConfig>,
    zhipu: Option<ProviderConfig>,
    moonshot: Option<ProviderConfig>,
    ollama: Option<ProviderConfig>,
}

impl LLMGlobalConfig {
    /// 获取当前激活的提供商配置
    pub fn active_config(&self) -> Result<ProviderConfig, String> {
        match self.active_provider.as_str() {
            "openai" => self.openai.clone().ok_or("OpenAI 未配置".into()),
            "alibaba" => self.alibaba.clone().ok_or("阿里云未配置".into()),
            "zhipu" => self.zhipu.clone().ok_or("智谱未配置".into()),
            "moonshot" => self.moonshot.clone().ok_or("Kimi未配置".into()),
            "ollama" => self.ollama.clone().ok_or("Ollama 未配置".into()),
            other => Err(format!("不支持的提供商: {}", other)),
        }
    }
}
```

---

## 十、实施路线图

| 阶段 | 内容 | 工期 | 前置依赖 |
|------|------|------|---------|
| **Phase 1** | LLM 模块骨架 + Provider trait + OpenAI 实现 | 3天 | 无 |
| **Phase 2** | Ollama 本地提供商 + Provider Factory | 1天 | Phase 1 |
| **Phase 3** | `knowledge_asset` 增加 embedding 字段 + pgvector 搜素 | 2天 | Phase 1, 数据库迁移 |
| **Phase 4** | 文档上传后自动向量化流水线 | 2天 | Phase 2, Phase 3 |
| **Phase 5** | RAG 问答 Service + Tauri Command | 2天 | Phase 3, Phase 4 |
| **Phase 6** | 前端 LLM 设置页 + RAG 对话页 | 3天 | Phase 5 |
| **Phase 7** | Skill 引擎集成 LLMProvider | 1天 | Phase 1, Phase 5 |
| **Phase 8** | 测试 + 联调 + 多提供商验证 | 2天 | 全部前置 |

**总计：约 16 天**

---

## 十一、关键设计决策

| 决策 | 原因 |
|------|------|
| 使用 `LLMProvider` trait 统一接口 | 多提供商可切换，方便扩展新模型 |
| pgvector 向量维度固定为 1536 | text-embedding-3-small 是最通用的嵌入模型 |
| 用 raw SQL 而非 ORM 管理 pgvector | sqlx 不支持 pgvector 原生类型，raw query 更灵活 |
| 文档向量化采用平均池化 | 简单高效，对大部分 RAG 场景足够 |
| 配置存储为 TOML 文件 + 前端持久化 | 支持后端默认配置 + 用户自定义覆盖 |
| 流式响应暂不实现 | MVP 先用非流式，后续迭代添加 SSE 支持 |
| 文本分块策略为 512 tokens / overlap 64 | 与 OpenKnowledge 的文档处理器对齐 |

---

## 十二、与 OpenKnowledge 的映射关系总表

| OpenKnowledge | assets-plateform 实现 | 说明 |
|--------------|----------------------|------|
| `litellm` LLM 网关 | `LLMProvider` trait | 统一接口，消除对 litellm 的依赖 |
| `PROVIDERS` 常量 | `all_provider_definitions()` | 配置化定义，新增 Provider 只需加一行 |
| `embedding_service.py` | `providers/openai.rs::embed()` + `embedding.rs` | 直接映射 |
| `rag_service.py` | `service/rag_service.rs` | 架构一致，增加源追踪 |
| `document_processor.py` | `chunk_service.rs` + 异步任务 | 简化实现，去除了 PDF 分页解析 |
| `settings store` | `ProviderConfig` + TOML 配置 | 前后端分离存储 |
| `pgvector` 模型 | `Vector(1536)` + IVFFlat 索引 | 完全一致 |
| `memories` | 暂不实现 | Phase 2 规划 |
| `conversations + messages` | 暂不实现 | 对话管理由前端 Zustand 管理，不落盘 |