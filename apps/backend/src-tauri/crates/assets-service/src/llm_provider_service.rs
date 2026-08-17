//! LLM 厂商/模型/偏好 CRUD Service
//!
//! 提供厂商、模型、用户偏好的增删改查操作。

use assets_database;
use assets_database::models::{LlmModel, LlmProvider, UserLLmSetting};
use crate::llm_gateway_service::adapters::OpenAIAdapter;
use crate::llm_gateway_service::LLMProviderAdapter;

// ======================== 厂商 CRUD ========================

pub async fn get_providers() -> Result<Vec<LlmProvider>, String> {
    let pool = assets_database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();
    let sql = format!(
        "SELECT id, provider_code, provider_name, base_url, api_key, secret_key, \
         extra_config, weight, is_local, enable, created_by, created_at, updated_at, deleted \
         FROM {}llm_provider WHERE deleted = 0 ORDER BY weight DESC",
        prefix
    );
    let list = sqlx::query_as::<_, LlmProvider>(sqlx::AssertSqlSafe(sql))
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("查询厂商列表失败: {}", e))?;
    Ok(list)
}

pub async fn get_provider(id: i64) -> Result<LlmProvider, String> {
    let pool = assets_database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();
    let sql = format!(
        "SELECT id, provider_code, provider_name, base_url, api_key, secret_key, \
         extra_config, weight, is_local, enable, created_by, created_at, updated_at, deleted \
         FROM {}llm_provider WHERE id = $1 AND deleted = 0",
        prefix
    );
    let item = sqlx::query_as::<_, LlmProvider>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("查询厂商失败: {}", e))?;
    Ok(item)
}

pub async fn create_provider(p: &LlmProvider) -> Result<LlmProvider, String> {
    let pool = assets_database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();
    let sql = format!(
        "INSERT INTO {}llm_provider \
         (provider_code, provider_name, base_url, api_key, secret_key, extra_config, weight, is_local, enable, created_by, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NOW()) \
         RETURNING id, provider_code, provider_name, base_url, api_key, secret_key, extra_config, weight, is_local, enable, created_by, created_at, updated_at, deleted",
        prefix
    );
    let inserted = sqlx::query_as::<_, LlmProvider>(sqlx::AssertSqlSafe(sql))
        .bind(&p.provider_code)
        .bind(&p.provider_name)
        .bind(&p.base_url)
        .bind(&p.api_key)
        .bind(&p.secret_key)
        .bind(&p.extra_config)
        .bind(p.weight)
        .bind(p.is_local)
        .bind(p.enable)
        .bind(p.created_by)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("创建厂商失败: {}", e))?;
    Ok(inserted)
}

pub async fn update_provider(
    id: i64,
    provider_code: &str,
    provider_name: &str,
    base_url: Option<&str>,
    api_key: Option<&str>,
    weight: Option<i32>,
    enable: Option<bool>,
) -> Result<LlmProvider, String> {
    let pool = assets_database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();
    let existing = get_provider(id).await?;

    let new_api_key = api_key.unwrap_or(existing.api_key.as_deref().unwrap_or(""));
    let new_base_url = base_url.unwrap_or(existing.base_url.as_deref().unwrap_or(""));
    let new_weight = weight.unwrap_or(existing.weight.unwrap_or(10));
    let new_enable = enable.unwrap_or(existing.enable);

    let sql = format!(
        "UPDATE {}llm_provider SET provider_code=$1, provider_name=$2, base_url=$3, api_key=$4, \
         weight=$5, enable=$6, updated_at=NOW() WHERE id=$7 \
         RETURNING id, provider_code, provider_name, base_url, api_key, secret_key, extra_config, weight, is_local, enable, created_by, created_at, updated_at, deleted",
        prefix
    );
    let updated = sqlx::query_as::<_, LlmProvider>(sqlx::AssertSqlSafe(sql))
        .bind(provider_code)
        .bind(provider_name)
        .bind(new_base_url)
        .bind(new_api_key)
        .bind(new_weight)
        .bind(new_enable)
        .bind(id)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("更新厂商失败: {}", e))?;
    Ok(updated)
}

pub async fn delete_provider(id: i64) -> Result<(), String> {
    let pool = assets_database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();
    let sql = format!("UPDATE {}llm_provider SET deleted=1 WHERE id=$1", prefix);
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| format!("删除厂商失败: {}", e))?;
    Ok(())
}

// ======================== 模型 CRUD ========================

pub async fn get_models(provider_id: Option<i64>) -> Result<Vec<LlmModel>, String> {
    let pool = assets_database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();

    let sql = if let Some(pid) = provider_id {
        format!(
            "SELECT id, provider_id, model_code, model_name, model_type, context_window, \
             temperature_default, max_tokens_default, price_input, price_output, enable, \
             created_by, created_at, updated_at, deleted FROM {}llm_model \
             WHERE provider_id = {} AND deleted = 0 ORDER BY model_type, model_code",
            prefix, pid
        )
    } else {
        format!(
            "SELECT id, provider_id, model_code, model_name, model_type, context_window, \
             temperature_default, max_tokens_default, price_input, price_output, enable, \
             created_by, created_at, updated_at, deleted FROM {}llm_model \
             WHERE deleted = 0 ORDER BY provider_id, model_type",
            prefix
        )
    };

    let list = sqlx::query_as::<_, LlmModel>(sqlx::AssertSqlSafe(sql))
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("查询模型列表失败: {}", e))?;
    Ok(list)
}

pub async fn create_model(
    provider_id: i64,
    model_code: &str,
    model_name: &str,
    model_type: &str,
    context_window: Option<i32>,
    temperature_default: Option<f64>,
    max_tokens_default: Option<i32>,
    enable: Option<bool>,
) -> Result<LlmModel, String> {
    let pool = assets_database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();
    let enable_val = enable.unwrap_or(true);
    let sql = format!(
        "INSERT INTO {}llm_model (provider_id, model_code, model_name, model_type, context_window, \
         temperature_default, max_tokens_default, enable, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW()) \
         ON CONFLICT (provider_id, model_code) DO UPDATE SET \
         model_name=$3, model_type=$4, context_window=$5, temperature_default=$6, max_tokens_default=$7, enable=$8, updated_at=NOW() \
         RETURNING id, provider_id, model_code, model_name, model_type, context_window, temperature_default, max_tokens_default, price_input, price_output, enable, created_by, created_at, updated_at, deleted",
        prefix
    );
    let inserted = sqlx::query_as::<_, LlmModel>(sqlx::AssertSqlSafe(sql))
        .bind(provider_id)
        .bind(model_code)
        .bind(model_name)
        .bind(model_type)
        .bind(context_window)
        .bind(temperature_default)
        .bind(max_tokens_default)
        .bind(enable_val)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("创建模型失败: {}", e))?;
    Ok(inserted)
}

pub async fn update_model(
    id: i64,
    model_code: Option<&str>,
    model_name: Option<&str>,
    model_type: Option<&str>,
    context_window: Option<i32>,
    temperature_default: Option<f64>,
    max_tokens_default: Option<i32>,
    enable: Option<bool>,
) -> Result<LlmModel, String> {
    let pool = assets_database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();
    let existing = sqlx::query_as::<_, LlmModel>(sqlx::AssertSqlSafe(format!(
        "SELECT id, provider_id, model_code, model_name, model_type, context_window, \
         temperature_default, max_tokens_default, price_input, price_output, enable, \
         created_by, created_at, updated_at, deleted FROM {}llm_model WHERE id = $1 AND deleted = 0",
        prefix
    )))
    .bind(id)
    .fetch_one(&pool)
    .await
    .map_err(|e| format!("查询模型失败: {}", e))?;

    let new_code = model_code.unwrap_or(&existing.model_code);
    let new_name = model_name.unwrap_or(&existing.model_name);
    let new_type = model_type.unwrap_or(&existing.model_type);
    let new_ctx = context_window.or(existing.context_window);
    let new_temp = temperature_default.or(existing.temperature_default);
    let new_max = max_tokens_default.or(existing.max_tokens_default);
    let new_enable = enable.unwrap_or(existing.enable);

    let sql = format!(
        "UPDATE {}llm_model SET model_code=$1, model_name=$2, model_type=$3, context_window=$4, \
         temperature_default=$5, max_tokens_default=$6, enable=$7, updated_at=NOW() WHERE id=$8 \
         RETURNING id, provider_id, model_code, model_name, model_type, context_window, temperature_default, max_tokens_default, price_input, price_output, enable, created_by, created_at, updated_at, deleted",
        prefix
    );
    let updated = sqlx::query_as::<_, LlmModel>(sqlx::AssertSqlSafe(sql))
        .bind(new_code)
        .bind(new_name)
        .bind(new_type)
        .bind(new_ctx)
        .bind(new_temp)
        .bind(new_max)
        .bind(new_enable)
        .bind(id)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("更新模型失败: {}", e))?;
    Ok(updated)
}

pub async fn delete_model(id: i64) -> Result<(), String> {
    let pool = assets_database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();
    let sql = format!("UPDATE {}llm_model SET deleted=1 WHERE id=$1", prefix);
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| format!("删除模型失败: {}", e))?;
    Ok(())
}

/// 从 OpenAI 兼容接口拉取模型列表并批量保存
pub async fn fetch_models_from_api(provider_id: i64) -> Result<Vec<LlmModel>, String> {
    let provider = get_provider(provider_id).await?;

    // 解密 API Key
    let api_key = match &provider.api_key {
        Some(key) if !key.is_empty() && key.len() > 20 => {
            match assets_utils::crypto::decrypt_api_key(key) {
                Ok(k) => k,
                Err(_) => key.clone(),
            }
        }
        Some(key) => key.clone(),
        None => return Err("厂商未配置 API Key".to_string()),
    };

    let base_url = provider.base_url.clone().unwrap_or_else(|| {
        crate::llm_gateway_service::get_default_base_url(&provider.provider_code)
    });

    // 创建适配器并调用 list_models
    let adapter = OpenAIAdapter::new(&api_key, &base_url, &provider.provider_code, "");
    let model_names = adapter.list_models().await?;

    if model_names.is_empty() {
        return Err("API 返回的模型列表为空".to_string());
    }

    // 分类并批量保存
    let mut saved = Vec::new();
    for name in &model_names {
        let model_type = classify_model_name(name);
        let context_window = estimate_context_window(name);

        let model = create_model(
            provider_id,
            name,
            name,
            model_type,
            context_window,
            None, // temperature_default
            None, // max_tokens_default
            Some(true),
        )
        .await?;
        saved.push(model);
    }

    Ok(saved)
}

/// 根据模型名称猜测类型（chat / embedding）
fn classify_model_name(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    if lower.contains("embedding") || lower.contains("ada") {
        "embedding"
    } else {
        "chat"
    }
}

/// 根据常见模型名估算上下文窗口
fn estimate_context_window(name: &str) -> Option<i32> {
    let lower = name.to_lowercase();
    if lower.contains("gpt-4") || lower.contains("gpt4") {
        if lower.contains("128k") || lower.contains("turbo") {
            Some(128000)
        } else {
            Some(8192)
        }
    } else if lower.contains("gpt-3.5") || lower.contains("gpt35") {
        Some(16384)
    } else if lower.contains("claude") {
        if lower.contains("3.5") || lower.contains("4") {
            Some(200000)
        } else {
            Some(100000)
        }
    } else if lower.contains("deepseek") {
        Some(65536)
    } else if lower.contains("qwen") {
        Some(32768)
    } else if lower.contains("doubao") {
        Some(128000)
    } else if lower.contains("hunyuan") {
        Some(32768)
    } else if lower.contains("llama") {
        Some(8192)
    } else {
        Some(4096)
    }
}

// ======================== 用户偏好 CRUD ========================

pub async fn get_user_llm_setting(user_id: i64) -> Result<Option<UserLLmSetting>, String> {
    let pool = assets_database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();
    let sql = format!(
        "SELECT id, user_id, default_provider_id, default_chat_model_id, default_embed_model_id, \
         custom_temp, custom_max_token, created_at, updated_at, deleted \
         FROM {}user_llm_setting WHERE user_id = $1 AND deleted = 0",
        prefix
    );
    let item = sqlx::query_as::<_, UserLLmSetting>(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| format!("查询用户偏好失败: {}", e))?;
    Ok(item)
}

pub async fn upsert_user_llm_setting(setting: &UserLLmSetting) -> Result<UserLLmSetting, String> {
    let pool = assets_database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();
    let sql = format!(
        "INSERT INTO {}user_llm_setting \
         (user_id, default_provider_id, default_chat_model_id, default_embed_model_id, custom_temp, custom_max_token, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW()) \
         ON CONFLICT (user_id) DO UPDATE SET \
         default_provider_id=$2, default_chat_model_id=$3, default_embed_model_id=$4, custom_temp=$5, custom_max_token=$6, updated_at=NOW() \
         RETURNING id, user_id, default_provider_id, default_chat_model_id, default_embed_model_id, custom_temp, custom_max_token, created_at, updated_at, deleted",
        prefix
    );
    let inserted = sqlx::query_as::<_, UserLLmSetting>(sqlx::AssertSqlSafe(sql))
        .bind(setting.user_id)
        .bind(setting.default_provider_id)
        .bind(setting.default_chat_model_id)
        .bind(setting.default_embed_model_id)
        .bind(setting.custom_temp)
        .bind(setting.custom_max_token)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("保存用户偏好失败: {}", e))?;
    Ok(inserted)
}
