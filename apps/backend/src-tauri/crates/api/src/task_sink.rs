//! 解析结果落库（`task_queue::ParseResultSink` 的业务实现）
//!
//! 为什么放在 `api` 而不是 `task-queue`（设计文档 §5.3）：
//! `task-queue` 必须保持「不依赖业务表」的边界，业务表结构由上层决定；
//! 而 `api` 已经依赖 `service` / `database`，是天然的业务装配层。
//!
//! 落库规则（刻意保守）：
//! 1. **只在资产正文为空时回填**（`content IS NULL OR content = ''`）——
//!    解析产物不应该覆盖用户手写的 Markdown；
//! 2. 不动标题 / OKF 类型 / 标签：这些是人工维护的元数据；
//! 3. 回填失败按「可重试」处理（`Transient`），交给 apalis 重试。

use std::future::Future;
use std::pin::Pin;

use task_queue::{ParseResultSink, ParseTask, TaskError};

/// 把解析文本回填到 `knowledge_asset.content`（仅空正文）
pub struct BusinessParseSink;

impl ParseResultSink for BusinessParseSink {
    fn on_parsed<'a>(
        &'a self,
        task: &'a ParseTask,
        result: &'a parser::ParseResult,
    ) -> Pin<Box<dyn Future<Output = Result<(), TaskError>> + Send + 'a>> {
        Box::pin(async move {
            // 调用方（worker）已把任务包在 database::with_schema 内，这里直接用 schema_prefix
            let pool = database::get_write_pool()
                .map_err(|e| TaskError::Transient(format!("获取数据库连接失败: {e}")))?;
            let prefix = database::schema_prefix();

            let sql = format!(
                "UPDATE {prefix}knowledge_asset SET content = $1, updated_at = NOW() \
                 WHERE id = $2 AND (deleted IS NULL OR deleted = 0) \
                 AND (content IS NULL OR content = '')"
            );
            let affected = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&result.raw_text)
                .bind(task.asset_id)
                .execute(&pool)
                .await
                .map_err(|e| TaskError::Transient(format!("回填资产正文失败: {e}")))?
                .rows_affected();

            tracing::info!(
                asset_id = task.asset_id,
                file_type = %result.file_type,
                text_len = result.raw_text.chars().count() as u64,
                images = result.images.len() as u64,
                backfilled = affected > 0,
                "解析结果已处理（正文为空时回填）"
            );
            Ok(())
        })
    }
}
