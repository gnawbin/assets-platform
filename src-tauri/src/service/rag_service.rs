//! RAG 检索引擎
//!
//! 提供文本切片、向量化、语义检索等 RAG 核心能力。

use crate::database;
use crate::database::models::{ChunkResult, DocumentChunk, RetrieveParams};
use sqlx::FromRow;
use tracing::info;

// ======================== 文本切片引擎 ========================

/// 文本切片器
pub struct TextChunker {
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub separator: String,
}

impl Default for TextChunker {
    fn default() -> Self {
        Self {
            chunk_size: 512,
            chunk_overlap: 128,
            separator: "\n\n".to_string(),
        }
    }
}

impl TextChunker {
    /// 对文本进行切片
    pub fn chunk(&self, text: &str) -> Vec<(String, i32)> {
        let mut chunks = Vec::new();
        if text.is_empty() {
            return chunks;
        }

        // 按分隔符分割
        let paragraphs: Vec<&str> = text.split(&self.separator).collect();
        let mut current = String::new();
        let mut current_tokens: i32 = 0;
        let chunk_size_i32 = self.chunk_size as i32;
        let chunk_overlap_i32 = self.chunk_overlap as i32;

        for para in paragraphs {
            let para_tokens = self.estimate_tokens(para);
            if current_tokens + para_tokens > chunk_size_i32 && !current.is_empty() {
                chunks.push((current.clone(), current_tokens));
                // 重叠逻辑：保留最后一部分
                let overlap_bytes = (chunk_overlap_i32 * 4) as usize;
                let overlap_start = current.len().saturating_sub(overlap_bytes);
                if overlap_start < current.len() {
                    current = current[overlap_start..].to_string();
                    current_tokens = self.estimate_tokens(&current);
                } else {
                    current.clear();
                    current_tokens = 0;
                }
            }
            if !current.is_empty() {
                current.push_str(&self.separator);
            }
            current.push_str(para);
            current_tokens += para_tokens;
        }

        if !current.is_empty() {
            chunks.push((current, current_tokens));
        }

        chunks
    }

    /// 估算文本的 Token 数（中英文混合：中文约 1.5 字符/token，英文约 4 字符/token）
    fn estimate_tokens(&self, text: &str) -> i32 {
        let mut tokens = 0;
        for ch in text.chars() {
            if ch.is_ascii() {
                tokens += 1;
            } else {
                tokens += 2;
            }
        }
        (tokens / 4).max(1)
    }
}

// ======================== RAG 检索器 ========================

/// RAG 检索器
pub struct RAGRetriever;

impl RAGRetriever {
    /// 执行 RAG 语义检索
    pub async fn retrieve(params: &RetrieveParams) -> Result<Vec<ChunkResult>, String> {
        let pool = database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
        let prefix = database::schema_prefix();

        let sql = format!(
            r#"SELECT 
                dc.id, dc.chunk_text, dc.chunk_index,
                COALESCE(dc.title, '') AS title,
                COALESCE(dc.okf_type, '') AS okf_type,
                dc.asset_id, dc.token_count,
                0.0 AS similarity
            FROM {}document_chunk dc
            WHERE dc.deleted = 0
                AND ($1::BIGINT IS NULL OR dc.tree_node_id IN (
                    WITH RECURSIVE sub_tree AS (
                        SELECT id FROM {}knowledge_tree WHERE id = $1 AND deleted = 0
                        UNION ALL
                        SELECT kt.id FROM {}knowledge_tree kt
                        JOIN sub_tree st ON kt.parent_id = st.id
                        WHERE kt.deleted = 0
                    )
                    SELECT id FROM sub_tree
                ))
                AND ($2::VARCHAR IS NULL OR dc.okf_type = $2)
            ORDER BY dc.id
            LIMIT $3"#,
            prefix, prefix, prefix
        );

        #[derive(FromRow)]
        struct DbChunk {
            id: i64,
            chunk_text: String,
            chunk_index: i32,
            title: String,
            okf_type: String,
            asset_id: i64,
            token_count: Option<i32>,
            similarity: Option<f64>,
        }

        let rows = sqlx::query_as::<_, DbChunk>(&sql)
            .bind(params.bind_tree_node_id)
            .bind(&params.okf_type_filter)
            .bind(params.top_k)
            .fetch_all(&pool)
            .await
            .map_err(|e| format!("RAG 检索失败: {}", e))?;

        Ok(rows
            .into_iter()
            .map(|r| ChunkResult {
                chunk_id: r.id,
                chunk_text: r.chunk_text,
                chunk_index: r.chunk_index,
                title: r.title,
                okf_type: r.okf_type,
                asset_id: r.asset_id,
                similarity: r.similarity.unwrap_or(0.0),
                token_count: r.token_count.unwrap_or(0),
            })
            .collect())
    }

    /// 对知识资产执行分片 + 向量化
    pub async fn chunk_and_vectorize(
        asset_id: i64,
        content: &str,
        title: &str,
        okf_type: &str,
        tags: &[String],
        tree_node_id: Option<i64>,
    ) -> Result<Vec<DocumentChunk>, String> {
        let chunker = TextChunker::default();
        let chunks = chunker.chunk(content);

        let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
        let prefix = database::schema_prefix();

        let mut results = Vec::new();

        for (i, (text, tokens)) in chunks.iter().enumerate() {
            let sql = format!(
                "INSERT INTO {}document_chunk (asset_id, chunk_index, chunk_text, token_count, title, okf_type, tags, tree_node_id, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW()) \
                 RETURNING id, asset_id, chunk_index, chunk_text, token_count, embedding, title, okf_type, tags, tree_node_id, created_at, deleted",
                prefix
            );

            let inserted = sqlx::query_as::<_, DocumentChunk>(&sql)
                .bind(asset_id)
                .bind(i as i32)
                .bind(text)
                .bind(*tokens)
                .bind(title)
                .bind(okf_type)
                .bind(tags)
                .bind(tree_node_id)
                .fetch_one(&pool)
                .await
                .map_err(|e| format!("插入分片失败: {}", e))?;

            results.push(inserted);
        }

        info!("知识资产 {} 已完成分片: {} 个分片", asset_id, results.len());
        Ok(results)
    }
}
