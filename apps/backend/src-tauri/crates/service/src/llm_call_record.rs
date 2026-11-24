//! `llm_call_record` 计量（沙箱内 Agent 调用平台的模型网关）
//!
//! 表在**租户 schema** 下（`{schema}.llm_call_record`，见 `tenant_tables.sql`），
//! 依赖调用方已通过 `auth_middleware` 设置 task-local schema。
//!
//! 约定：
//! 1. 单条失败只记 WARN，不阻塞调用方；
//! 2. `total_cost` 暂不写入（缺 `llm_model` 价格联动，列默认 0）；
//! 3. 失败调用同样记录（`provider_id`/`model_id` 未知写 0），便于排查。

use sqlx::PgPool;

/// 一条调用记录
#[derive(Debug, Clone)]
pub struct CallRecord<'a> {
    /// 业务用户（服务身份调用时为 `None`）
    pub user_id: Option<i64>,
    /// 会话 ID（工作流场景为 `None`）
    pub conv_id: Option<i64>,
    /// 实际使用的 Provider（失败时为 0）
    pub provider_id: i64,
    /// 实际使用的模型（失败时为 0）
    pub model_id: i64,
    /// 调用类型（沙箱内统一 `sandbox_chat`）
    pub call_type: &'a str,
    /// 输入 token
    pub input_tokens: i32,
    /// 输出 token
    pub output_tokens: i32,
    /// 耗时（毫秒）
    pub duration_ms: i32,
    /// `success` / `error`
    pub status: &'a str,
    /// 失败原因（截断后写入）
    pub error_msg: Option<&'a str>,
    /// 平台 LLM 网关返回的 request_id
    pub request_id: Option<&'a str>,
}

/// 写入一条调用记录
///
/// # Errors
/// 表不存在 / 连接失败等 DB 错误时返回错误（调用方应只记 WARN）。
pub async fn record(pool: &PgPool, record: &CallRecord<'_>) -> Result<(), String> {
    let prefix = database::schema_prefix();
    let sql = format!(
        "INSERT INTO {prefix}llm_call_record \
         (user_id, conv_id, provider_id, model_id, call_type, input_tokens, output_tokens, \
          duration_ms, status, error_msg, request_id, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NOW())"
    );

    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(record.user_id)
        .bind(record.conv_id)
        .bind(record.provider_id)
        .bind(record.model_id)
        .bind(record.call_type)
        .bind(record.input_tokens)
        .bind(record.output_tokens)
        .bind(record.duration_ms)
        .bind(record.status)
        .bind(record.error_msg.map(|message| truncate(message, 500)))
        .bind(record.request_id)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_message_is_truncated_by_chars() {
        let text = "错".repeat(600);
        assert_eq!(truncate(&text, 500).chars().count(), 500);
        assert_eq!(truncate("short", 500), "short");
    }
}