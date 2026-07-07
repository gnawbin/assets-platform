//! 对话会话 Service
//!
//! 提供多轮对话的创建、消息发送、历史获取等能力。

use crate::database;
use crate::database::models::RetrieveParams;
use crate::database::models::{ChunkResult, Conversation, Message};
use crate::service::rag_service::RAGRetriever;
use tracing::{error, info};

/// 对话响应
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConversationResponse {
    pub conv_id: String,
    pub answer: String,
    pub cited_assets: Vec<AssetInfo>,
    pub usage: TokenUsageInfo,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetInfo {
    pub id: String,
    pub title: String,
    pub okf_type: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TokenUsageInfo {
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub total_tokens: i32,
    pub cost: f64,
}

/// 对话系统 Service
pub struct ConversationService;

impl ConversationService {
    /// 创建新会话并回答
    pub async fn create_conversation_and_answer(
        user_id: i64,
        question: &str,
        bind_tree_node_id: Option<i64>,
    ) -> Result<ConversationResponse, String> {
        // 1. 创建会话
        let title = Self::generate_title(question);
        let conv = Self::insert_conversation(user_id, &title, bind_tree_node_id).await?;

        // 2. 保存用户消息
        Self::insert_message(conv.id, "user", question, None, None, 0, 0).await?;

        // 3. 执行 RAG 检索
        let params = RetrieveParams {
            question: question.to_string(),
            bind_tree_node_id,
            top_k: 5,
            max_tokens: 2000,
            okf_type_filter: None,
            min_similarity: 0.0,
        };
        let chunks = RAGRetriever::retrieve(&params).await?;

        // 4. 构建简单回答（RAG 模式：直接拼接检索结果）
        let answer = Self::build_rag_answer(question, &chunks);

        // 5. 提取引用
        let cited_ids: Vec<i64> = chunks.iter().map(|c| c.asset_id).collect();
        let cited_assets = Self::get_cited_asset_info(&cited_ids).await?;

        // 6. 保存 AI 消息
        let cited_str = cited_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let input_tokens = question.len() as i32;
        let output_tokens = answer.len() as i32;

        Self::insert_message(
            conv.id,
            "assistant",
            &answer,
            if cited_ids.is_empty() {
                None
            } else {
                Some(&cited_str)
            },
            None,
            input_tokens,
            output_tokens,
        )
        .await?;

        Ok(ConversationResponse {
            conv_id: conv.id.to_string(),
            answer,
            cited_assets,
            usage: TokenUsageInfo {
                input_tokens,
                output_tokens,
                total_tokens: input_tokens + output_tokens,
                cost: 0.0,
            },
        })
    }

    /// 构建基于检索结果的简单回答
    fn build_rag_answer(_question: &str, chunks: &[ChunkResult]) -> String {
        if chunks.is_empty() {
            return "未找到相关的知识内容。请尝试调整问题或检查知识库中是否包含相关信息。"
                .to_string();
        }

        let mut answer = String::from("根据知识库中的相关资料，以下是与您问题相关的内容：\n\n");
        for (i, chunk) in chunks.iter().enumerate() {
            answer.push_str(&format!(
                "**[来源{}]** 《{}》\n{}\n\n",
                i + 1,
                chunk.title,
                chunk.chunk_text
            ));
        }
        answer.push_str("---\n> 📎 以上内容来源于知识库，请点击来源标题查看完整文档。");
        answer
    }

    /// 获取引用资产信息
    async fn get_cited_asset_info(asset_ids: &[i64]) -> Result<Vec<AssetInfo>, String> {
        if asset_ids.is_empty() {
            return Ok(Vec::new());
        }
        let pool = database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
        let prefix = database::schema_prefix();

        // 去重
        let mut unique_ids: Vec<i64> = asset_ids.to_vec();
        unique_ids.sort();
        unique_ids.dedup();

        let mut result = Vec::new();
        for id in unique_ids {
            let sql = format!(
                "SELECT id, title, okf_type FROM {}knowledge_asset WHERE id = $1 AND deleted = 0",
                prefix
            );
            let row = sqlx::query_as::<_, (i64, String, String)>(sqlx::AssertSqlSafe(sql))
                .bind(id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| format!("查询资产失败: {}", e))?;

            if let Some((aid, title, okf_type)) = row {
                result.push(AssetInfo {
                    id: aid.to_string(),
                    title,
                    okf_type,
                });
            }
        }
        Ok(result)
    }

    /// 继续已有会话
    pub async fn continue_conversation(
        conv_id: i64,
        user_id: i64,
        question: &str,
    ) -> Result<ConversationResponse, String> {
        // 验证会话所有权
        let conv = Self::get_conversation_by_id(conv_id).await?;
        if conv.user_id != user_id {
            return Err("无权访问此会话".to_string());
        }

        // 保存用户消息
        Self::insert_message(conv_id, "user", question, None, None, 0, 0).await?;

        // 复用创建逻辑
        let bind_tree_node_id = conv.bind_knowledge_tree_id;
        let params = RetrieveParams {
            question: question.to_string(),
            bind_tree_node_id,
            top_k: 5,
            max_tokens: 2000,
            okf_type_filter: None,
            min_similarity: 0.0,
        };
        let chunks = RAGRetriever::retrieve(&params).await?;
        let answer = Self::build_rag_answer(question, &chunks);

        let cited_ids: Vec<i64> = chunks.iter().map(|c| c.asset_id).collect();
        let cited_assets = Self::get_cited_asset_info(&cited_ids).await?;

        let cited_str = cited_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let input_tokens = question.len() as i32;
        let output_tokens = answer.len() as i32;

        Self::insert_message(
            conv_id,
            "assistant",
            &answer,
            if cited_ids.is_empty() {
                None
            } else {
                Some(&cited_str)
            },
            None,
            input_tokens,
            output_tokens,
        )
        .await?;

        Ok(ConversationResponse {
            conv_id: conv_id.to_string(),
            answer,
            cited_assets,
            usage: TokenUsageInfo {
                input_tokens,
                output_tokens,
                total_tokens: input_tokens + output_tokens,
                cost: 0.0,
            },
        })
    }

    /// 获取会话列表
    pub async fn get_conversations(
        user_id: i64,
        page: i32,
        page_size: i32,
    ) -> Result<(Vec<Conversation>, i64), String> {
        let pool = database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
        let prefix = database::schema_prefix();

        let count_sql = format!(
            "SELECT COUNT(*) FROM {}conversation WHERE user_id = $1 AND deleted = 0",
            prefix
        );
        let total: (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(count_sql))
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .map_err(|e| format!("查询会话总数失败: {}", e))?;

        let offset = (page - 1) * page_size;
        let sql = format!(
            "SELECT id, user_id, title, bind_knowledge_tree_id, created_at, updated_at, deleted \
             FROM {}conversation WHERE user_id = $1 AND deleted = 0 \
             ORDER BY updated_at DESC LIMIT $2 OFFSET $3",
            prefix
        );
        let list = sqlx::query_as::<_, Conversation>(sqlx::AssertSqlSafe(sql))
            .bind(user_id)
            .bind(page_size)
            .bind(offset)
            .fetch_all(&pool)
            .await
            .map_err(|e| format!("查询会话列表失败: {}", e))?;

        Ok((list, total.0))
    }

    /// 获取会话消息
    pub async fn get_conversation_messages(
        conv_id: i64,
        page: i32,
        page_size: i32,
    ) -> Result<Vec<Message>, String> {
        let pool = database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
        let prefix = database::schema_prefix();

        let offset = (page - 1) * page_size;
        let sql = format!(
            "SELECT id, conv_id, role, content, audio_url, reference_asset_ids, reference_text, \
             metadata, input_tokens, output_tokens, created_at, deleted \
             FROM {}message WHERE conv_id = $1 AND deleted = 0 \
             ORDER BY created_at ASC LIMIT $2 OFFSET $3",
            prefix
        );
        let messages = sqlx::query_as::<_, Message>(sqlx::AssertSqlSafe(sql))
            .bind(conv_id)
            .bind(page_size)
            .bind(offset)
            .fetch_all(&pool)
            .await
            .map_err(|e| format!("查询消息失败: {}", e))?;

        Ok(messages)
    }

    /// 插入会话
    async fn insert_conversation(
        user_id: i64,
        title: &str,
        bind_tree_node_id: Option<i64>,
    ) -> Result<Conversation, String> {
        let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
        let prefix = database::schema_prefix();

        let sql = format!(
            "INSERT INTO {}conversation (user_id, title, bind_knowledge_tree_id, created_at, updated_at) \
             VALUES ($1, $2, $3, NOW(), NOW()) \
             RETURNING id, user_id, title, bind_knowledge_tree_id, created_at, updated_at, deleted",
            prefix
        );
        let conv = sqlx::query_as::<_, Conversation>(sqlx::AssertSqlSafe(sql))
            .bind(user_id)
            .bind(title)
            .bind(bind_tree_node_id)
            .fetch_one(&pool)
            .await
            .map_err(|e| format!("创建会话失败: {}", e))?;

        info!("创建新会话: id={}, title={}", conv.id, title);
        Ok(conv)
    }

    /// 插入消息
    async fn insert_message(
        conv_id: i64,
        role: &str,
        content: &str,
        reference_asset_ids: Option<&str>,
        metadata: Option<&str>,
        input_tokens: i32,
        output_tokens: i32,
    ) -> Result<i64, String> {
        let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
        let prefix = database::schema_prefix();

        // 解析引用ID字符串为数组
        let ref_ids: Option<Vec<i64>> = reference_asset_ids.map(|s| {
            s.split(',')
                .filter_map(|id| id.trim().parse::<i64>().ok())
                .collect()
        });

        // 解析 metadata
        let meta: Option<serde_json::Value> = metadata.and_then(|m| serde_json::from_str(m).ok());

        let sql = format!(
            "INSERT INTO {}message (conv_id, role, content, reference_asset_ids, metadata, input_tokens, output_tokens, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, NOW()) \
             RETURNING id",
            prefix
        );
        let row: (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(conv_id)
            .bind(role)
            .bind(content)
            .bind(&ref_ids)
            .bind(&meta)
            .bind(input_tokens)
            .bind(output_tokens)
            .fetch_one(&pool)
            .await
            .map_err(|e| format!("插入消息失败: {}", e))?;

        Ok(row.0)
    }

    /// 获取单条会话
    async fn get_conversation_by_id(conv_id: i64) -> Result<Conversation, String> {
        let pool = database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
        let prefix = database::schema_prefix();

        let sql = format!(
            "SELECT id, user_id, title, bind_knowledge_tree_id, created_at, updated_at, deleted \
             FROM {}conversation WHERE id = $1 AND deleted = 0",
            prefix
        );
        let conv = sqlx::query_as::<_, Conversation>(sqlx::AssertSqlSafe(sql))
            .bind(conv_id)
            .fetch_one(&pool)
            .await
            .map_err(|e| format!("查询会话失败: {}", e))?;

        Ok(conv)
    }

    /// 生成会话标题
    fn generate_title(question: &str) -> String {
        let trimmed = question.trim();
        let max_len = 30;
        if trimmed.chars().count() <= max_len {
            return trimmed.to_string();
        }
        let title: String = trimmed.chars().take(max_len).collect();
        format!("{}...", title)
    }

    /// 删除会话
    pub async fn delete_conversation(conv_id: i64) -> Result<(), String> {
        let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
        let prefix = database::schema_prefix();

        let sql = format!(
            "UPDATE {}conversation SET deleted = 1, updated_at = NOW() WHERE id = $1",
            prefix
        );
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(conv_id)
            .execute(&pool)
            .await
            .map_err(|e| format!("删除会话失败: {}", e))?;

        Ok(())
    }

    /// 更新会话标题
    pub async fn update_conversation_title(conv_id: i64, title: &str) -> Result<(), String> {
        let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
        let prefix = database::schema_prefix();

        let sql = format!(
            "UPDATE {}conversation SET title = $1, updated_at = NOW() WHERE id = $2",
            prefix
        );
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(title)
            .bind(conv_id)
            .execute(&pool)
            .await
            .map_err(|e| format!("更新会话标题失败: {}", e))?;

        Ok(())
    }

    /// 更新会话绑定目录
    pub async fn update_conversation_bind_tree(
        conv_id: i64,
        bind_tree_node_id: Option<i64>,
    ) -> Result<(), String> {
        let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
        let prefix = database::schema_prefix();

        let sql = format!(
            "UPDATE {}conversation SET bind_knowledge_tree_id = $1, updated_at = NOW() WHERE id = $2",
            prefix
        );
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(bind_tree_node_id)
            .bind(conv_id)
            .execute(&pool)
            .await
            .map_err(|e| format!("更新会话绑定目录失败: {}", e))?;

        Ok(())
    }
}
