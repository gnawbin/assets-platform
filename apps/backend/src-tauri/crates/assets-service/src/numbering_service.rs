//! 单据编号规则管理 Service
//!
//! 提供单据编号规则的 CRUD 操作以及统一编号生成功能。
//! 支持前缀 + 日期 + 流水号的灵活组合，按年/按月/永不重置流水号。

use assets_database;
use assets_database::models::DocNumberingRule;
use assets_database::{get_read_pool, get_write_pool};
use assets_utils::snowflake::next_id;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

/// 编号规则响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumberingRuleResponse {
    #[serde(serialize_with = "assets_database::models::i64_to_string")]
    pub id: i64,
    pub biz_type: String,
    pub biz_name: String,
    pub prefix: Option<String>,
    pub date_format: Option<String>,
    pub date_position: Option<String>,
    pub serial_length: i32,
    pub separator: Option<String>,
    pub reset_mode: Option<String>,
    pub sample_output: Option<String>,
    pub is_active: bool,
}

impl From<DocNumberingRule> for NumberingRuleResponse {
    fn from(r: DocNumberingRule) -> Self {
        Self {
            id: r.id,
            biz_type: r.biz_type,
            biz_name: r.biz_name,
            prefix: r.prefix,
            date_format: r.date_format,
            date_position: r.date_position,
            serial_length: r.serial_length,
            separator: r.separator,
            reset_mode: r.reset_mode,
            sample_output: r.sample_output,
            is_active: r.is_active,
        }
    }
}

/// 编号规则创建/更新请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumberingRuleInput {
    pub biz_type: String,
    pub biz_name: String,
    pub prefix: Option<String>,
    pub date_format: Option<String>,
    pub date_position: Option<String>,
    pub serial_length: i32,
    pub separator: Option<String>,
    pub reset_mode: Option<String>,
    pub is_active: bool,
}

fn rule_table() -> String {
    format!("{}doc_numbering_rule", assets_database::schema_prefix())
}

fn seq_table() -> String {
    format!("{}doc_numbering_sequence", assets_database::schema_prefix())
}

/// 获取所有编号规则（租户级）
pub async fn get_rules() -> Result<Vec<NumberingRuleResponse>, String> {
    let pool = get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let table = rule_table();

    let sql = format!(
        "SELECT id, biz_type, biz_name, prefix, date_format, date_position, \
         serial_length, separator, reset_mode, sample_output, is_active, \
         created_by, created_at, updated_by, updated_at, deleted \
         FROM {} WHERE deleted = 0 ORDER BY id ASC",
        table
    );

    let rules = sqlx::query_as::<_, DocNumberingRule>(sqlx::AssertSqlSafe(sql))
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询编号规则列表失败: {}", e);
            format!("查询编号规则列表失败: {}", e)
        })?;

    Ok(rules.into_iter().map(|r| r.into()).collect())
}

/// 根据业务类型获取单条规则
pub async fn get_rule(biz_type: &str) -> Result<NumberingRuleResponse, String> {
    let pool = get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let table = rule_table();

    let sql = format!(
        "SELECT id, biz_type, biz_name, prefix, date_format, date_position, \
         serial_length, separator, reset_mode, sample_output, is_active, \
         created_by, created_at, updated_by, updated_at, deleted \
         FROM {} WHERE biz_type = $1 AND deleted = 0",
        table
    );

    let rule = sqlx::query_as::<_, DocNumberingRule>(sqlx::AssertSqlSafe(sql))
        .bind(biz_type)
        .fetch_optional(&pool)
        .await
        .map_err(|e| {
            error!("查询编号规则失败: biz_type={}, error={}", biz_type, e);
            format!("查询编号规则失败: {}", e)
        })?
        .ok_or_else(|| format!("未找到业务类型 '{}' 的编号规则", biz_type))?;

    Ok(rule.into())
}

/// 保存编号规则（新增或更新）
pub async fn save_rule(
    id: Option<i64>,
    input: NumberingRuleInput,
    current_user_id: Option<i64>,
) -> Result<NumberingRuleResponse, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let table = rule_table();

    // 计算示例输出
    let sample = compute_sample(&input);

    if let Some(rule_id) = id {
        // 更新
        let sql = format!(
            r#"
            UPDATE {}
            SET biz_name = $2, prefix = $3, date_format = $4, date_position = $5,
                serial_length = $6, separator = $7, reset_mode = $8, is_active = $9,
                sample_output = $10, updated_by = $11, updated_at = NOW()
            WHERE id = $1 AND deleted = 0
            RETURNING id, biz_type, biz_name, prefix, date_format, date_position,
                      serial_length, separator, reset_mode, sample_output, is_active,
                      created_by, created_at, updated_by, updated_at, deleted
            "#,
            table
        );

        let rule = sqlx::query_as::<_, DocNumberingRule>(sqlx::AssertSqlSafe(sql))
            .bind(rule_id)
            .bind(&input.biz_name)
            .bind(&input.prefix)
            .bind(&input.date_format)
            .bind(&input.date_position)
            .bind(input.serial_length)
            .bind(&input.separator)
            .bind(&input.reset_mode)
            .bind(input.is_active)
            .bind(&sample)
            .bind(current_user_id)
            .fetch_one(&pool)
            .await
            .map_err(|e| {
                error!("更新编号规则失败: id={}, error={}", rule_id, e);
                format!("更新编号规则失败: {}", e)
            })?;

        info!(
            "更新编号规则成功: id={}, biz_type={}",
            rule_id, rule.biz_type
        );
        Ok(rule.into())
    } else {
        // 新增
        let new_id = next_id() as i64;

        // 检查 biz_type 是否已存在
        let check_sql = format!(
            "SELECT EXISTS(SELECT 1 FROM {} WHERE biz_type = $1 AND deleted = 0)",
            table
        );

        let exists: bool = sqlx::query_scalar::<_, bool>(sqlx::AssertSqlSafe(check_sql))
            .bind(&input.biz_type)
            .fetch_one(&pool)
            .await
            .map_err(|e| {
                error!("检查编号规则是否已存在失败: {}", e);
                format!("检查编号规则失败: {}", e)
            })?;

        if exists {
            return Err(format!("业务类型 '{}' 的编号规则已存在", input.biz_type));
        }

        let insert_sql = format!(
            r#"
            INSERT INTO {}
                (id, biz_type, biz_name, prefix, date_format, date_position,
                 serial_length, separator, reset_mode, sample_output, is_active,
                 created_by, created_at, updated_by, updated_at, deleted)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, NOW(), $12, NOW(), 0)
            RETURNING id, biz_type, biz_name, prefix, date_format, date_position,
                      serial_length, separator, reset_mode, sample_output, is_active,
                      created_by, created_at, updated_by, updated_at, deleted
            "#,
            table
        );

        let rule = sqlx::query_as::<_, DocNumberingRule>(sqlx::AssertSqlSafe(insert_sql))
            .bind(new_id)
            .bind(&input.biz_type)
            .bind(&input.biz_name)
            .bind(&input.prefix)
            .bind(&input.date_format)
            .bind(&input.date_position)
            .bind(input.serial_length)
            .bind(&input.separator)
            .bind(&input.reset_mode)
            .bind(&sample)
            .bind(input.is_active)
            .bind(current_user_id)
            .fetch_one(&pool)
            .await
            .map_err(|e| {
                error!("新增编号规则失败: biz_type={}, error={}", input.biz_type, e);
                format!("新增编号规则失败: {}", e)
            })?;

        info!(
            "新增编号规则成功: id={}, biz_type={}",
            new_id, rule.biz_type
        );
        Ok(rule.into())
    }
}

/// 生成下一个编号（核心方法）
///
/// 1. 根据 biz_type 获取规则配置
/// 2. 计算当前重置键（如 "2026" 或 "202607"）
/// 3. 获取/创建流水号记录（UPSERT 原子操作）
/// 4. 流水号 +1
/// 5. 拼接输出
pub async fn generate_number(biz_type: &str) -> Result<String, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let rule_t = rule_table();
    let seq_t = seq_table();

    // 1. 获取规则
    let rule_sql = format!(
        "SELECT id, biz_type, biz_name, prefix, date_format, date_position, \
         serial_length, separator, reset_mode, sample_output, is_active, \
         created_by, created_at, updated_by, updated_at, deleted \
         FROM {} WHERE biz_type = $1 AND deleted = 0 AND is_active = true",
        rule_t
    );

    let rule = sqlx::query_as::<_, DocNumberingRule>(sqlx::AssertSqlSafe(rule_sql))
        .bind(biz_type)
        .fetch_optional(&pool)
        .await
        .map_err(|e| {
            error!("查询编号规则失败: biz_type={}, error={}", biz_type, e);
            format!("查询编号规则失败: {}", e)
        })?
        .ok_or_else(|| format!("未找到或未启用业务类型 '{}' 的编号规则", biz_type))?;

    // 2. 计算重置键
    let now = Utc::now();
    let reset_key = compute_reset_key(&rule, now);

    // 3. 使用 UPSERT 原子获取/创建序列
    let seq_id = next_id() as i64;

    let upsert_sql = format!(
        r#"
        INSERT INTO {} (id, biz_type, reset_key, current_seq, updated_at)
        VALUES ($1, $2, $3, 1, NOW())
        ON CONFLICT (biz_type, reset_key) DO UPDATE
            SET current_seq = doc_numbering_sequence.current_seq + 1,
                updated_at = NOW()
            WHERE doc_numbering_sequence.biz_type = $2
              AND doc_numbering_sequence.reset_key = $3
        RETURNING current_seq
        "#,
        seq_t
    );

    let result = sqlx::query_as::<_, (i32,)>(sqlx::AssertSqlSafe(upsert_sql))
        .bind(seq_id)
        .bind(&rule.biz_type)
        .bind(&reset_key)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!(
                "获取流水号失败: biz_type={}, reset_key={}, error={}",
                biz_type, reset_key, e
            );
            format!("获取流水号失败: {}", e)
        })?;

    let serial = result.0;

    // 5. 拼接编号
    let number = format_number(&rule, &reset_key, serial);

    info!("生成编号成功: biz_type={}, number={}", biz_type, number);
    Ok(number)
}

/// 重置指定业务类型的流水号
pub async fn reset_sequence(biz_type: &str, reset_key: &str) -> Result<(), String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let table = seq_table();

    let sql = format!(
        "UPDATE {} SET current_seq = 0, updated_at = NOW() \
         WHERE biz_type = $1 AND reset_key = $2",
        table
    );

    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(biz_type)
        .bind(reset_key)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!(
                "重置流水号失败: biz_type={}, reset_key={}, error={}",
                biz_type, reset_key, e
            );
            format!("重置流水号失败: {}", e)
        })?;

    info!(
        "重置流水号成功: biz_type={}, reset_key={}",
        biz_type, reset_key
    );
    Ok(())
}

// ======================== 辅助函数 ========================

/// 根据规则和当前时间计算重置键
fn compute_reset_key(rule: &DocNumberingRule, now: chrono::DateTime<Utc>) -> String {
    match rule.reset_mode.as_deref() {
        Some("yearly") => now.format("%Y").to_string(),
        Some("monthly") => now.format("%Y%m").to_string(),
        _ => String::new(), // never 或其他：空字符串作为全局序列
    }
}

/// 根据日期格式格式化日期部分
fn format_date_part(date_format: &Option<String>, now: chrono::DateTime<Utc>) -> String {
    match date_format.as_deref() {
        Some("yyyyMMdd") => now.format("%Y%m%d").to_string(),
        Some("yyMMdd") => now.format("%y%m%d").to_string(),
        Some("yyyyMM") => now.format("%Y%m").to_string(),
        Some("yyyy") => now.format("%Y").to_string(),
        _ => String::new(), // 无日期
    }
}

/// 拼接完整编号
fn format_number(rule: &DocNumberingRule, _reset_key: &str, serial: i32) -> String {
    let sep = rule.separator.as_deref().unwrap_or("-");
    let prefix = rule.prefix.as_deref().unwrap_or("");
    let date_str = format_date_part(&rule.date_format, Utc::now());
    let serial_str = format!("{:0>width$}", serial, width = rule.serial_length as usize);

    let mut parts: Vec<String> = Vec::new();

    if !prefix.is_empty() {
        parts.push(prefix.to_string());
    }

    // 根据 date_position 决定日期位置
    match rule.date_position.as_deref() {
        Some("before_serial") => {
            if !date_str.is_empty() {
                parts.push(date_str);
            }
            parts.push(serial_str);
        }
        _ => {
            // after_prefix（默认）
            if !date_str.is_empty() {
                parts.push(date_str);
            }
            parts.push(serial_str);
        }
    }

    parts.join(sep)
}

/// 计算示例输出
fn compute_sample(input: &NumberingRuleInput) -> String {
    let now = Utc::now();

    let date_format = &input.date_format;
    let date_str = format_date_part(date_format, now);

    let sep = input.separator.as_deref().unwrap_or("-");
    let prefix = input.prefix.as_deref().unwrap_or("");
    let serial_str = format!("{:0>width$}", 1, width = input.serial_length as usize);

    let mut parts: Vec<String> = Vec::new();
    if !prefix.is_empty() {
        parts.push(prefix.to_string());
    }
    match input.date_position.as_deref() {
        Some("before_serial") => {
            if !date_str.is_empty() {
                parts.push(date_str);
            }
            parts.push(serial_str);
        }
        _ => {
            if !date_str.is_empty() {
                parts.push(date_str);
            }
            parts.push(serial_str);
        }
    }

    parts.join(sep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_number_with_date() {
        let rule = DocNumberingRule {
            id: 1,
            biz_type: "asset".to_string(),
            biz_name: "资产编号".to_string(),
            prefix: Some("ZC".to_string()),
            date_format: Some("yyyyMMdd".to_string()),
            date_position: Some("after_prefix".to_string()),
            serial_length: 4,
            separator: Some("-".to_string()),
            reset_mode: Some("yearly".to_string()),
            sample_output: None,
            is_active: true,
            created_by: None,
            created_at: None,
            updated_by: None,
            updated_at: None,
            deleted: 0,
        };
        let number = format_number(&rule, "2026", 1);
        assert!(number.starts_with("ZC"));
        assert!(number.ends_with("0001"));
        assert_eq!(number.chars().filter(|&c| c == '-').count(), 2);
    }

    #[test]
    fn test_format_number_no_prefix() {
        let rule = DocNumberingRule {
            id: 1,
            biz_type: "test".to_string(),
            biz_name: "测试".to_string(),
            prefix: None,
            date_format: Some("yyyyMMdd".to_string()),
            date_position: Some("after_prefix".to_string()),
            serial_length: 4,
            separator: Some("-".to_string()),
            reset_mode: Some("never".to_string()),
            sample_output: None,
            is_active: true,
            created_by: None,
            created_at: None,
            updated_by: None,
            updated_at: None,
            deleted: 0,
        };
        let number = format_number(&rule, "", 42);
        // 无前缀，格式为 "日期-0042"
        assert!(number.ends_with("0042"));
    }

    #[test]
    fn test_compute_sample() {
        let input = NumberingRuleInput {
            biz_type: "asset".to_string(),
            biz_name: "资产编号".to_string(),
            prefix: Some("ZC".to_string()),
            date_format: Some("yyyyMMdd".to_string()),
            date_position: Some("after_prefix".to_string()),
            serial_length: 4,
            separator: Some("-".to_string()),
            reset_mode: Some("yearly".to_string()),
            is_active: true,
        };
        let sample = compute_sample(&input);
        assert!(sample.starts_with("ZC"));
        assert!(sample.ends_with("0001"));
    }
}
