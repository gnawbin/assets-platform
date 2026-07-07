//! LLM 厂商/模型/偏好 CRUD Service
//!
//! 提供厂商、模型、用户偏好的增删改查操作。

use crate::database;
use crate::database::models::{LLmCallRecord, LlmModel, LlmProvider, UserLLmSetting};

// ======================== 厂商 CRUD ========================

pub async fn get_providers() -> Result<Vec<LlmProvider>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let list = sqlx::query_as::<_, LlmProvider>(
        "SELECT id, provider_code, provider_name, base_url, api_key, secret_key, \
         extra_config, weight, is_local, enable, created_by, created_at, updated_at, deleted \
         FROM public.llm_provider WHERE deleted = 0 ORDER BY weight DESC",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("查询厂商列表失败: {}", e))?;
    Ok(list)
}

pub async fn get_provider(id: i64) -> Result<LlmProvider, String> {
    let pool = database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let item = sqlx::query_as::<_, LlmProvider>(
        "SELECT id, provider_code, provider_name, base_url, api_key, secret_key, \
         extra_config, weight, is_local, enable, created_by, created_at, updated_at, deleted \
         FROM public.llm_provider WHERE id = $1 AND deleted = 0",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .map_err(|e| format!("查询厂商失败: {}", e))?;
    Ok(item)
}

pub async fn create_provider(p: &LlmProvider) -> Result<LlmProvider, String> {
    let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let inserted = sqlx::query_as::<_, LlmProvider>(
        "INSERT INTO public.llm_provider \
         (provider_code, provider_name, base_url, api_key, secret_key, extra_config, weight, is_local, enable, created_by, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NOW()) \
         RETURNING id, provider_code, provider_name, base_url, api_key, secret_key, extra_config, weight, is_local, enable, created_by, created_at, updated_at, deleted",
    )
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
    let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let existing = get_provider(id).await?;

    let new_api_key = api_key.unwrap_or(existing.api_key.as_deref().unwrap_or(""));
    let new_base_url = base_url.unwrap_or(existing.base_url.as_deref().unwrap_or(""));
    let new_weight = weight.unwrap_or(existing.weight.unwrap_or(10));
    let new_enable = enable.unwrap_or(existing.enable);

    let updated = sqlx::query_as::<_, LlmProvider>(
        "UPDATE public.llm_provider SET provider_code=$1, provider_name=$2, base_url=$3, api_key=$4, \
         weight=$5, enable=$6, updated_at=NOW() WHERE id=$7 \
         RETURNING id, provider_code, provider_name, base_url, api_key, secret_key, extra_config, weight, is_local, enable, created_by, created_at, updated_at, deleted",
    )
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
    let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    sqlx::query("UPDATE public.llm_provider SET deleted=1 WHERE id=$1")
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| format!("删除厂商失败: {}", e))?;
    Ok(())
}

// ======================== 模型 CRUD ========================

pub async fn get_models(provider_id: Option<i64>) -> Result<Vec<LlmModel>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    let sql = if let Some(pid) = provider_id {
        format!(
            "SELECT id, provider_id, model_code, model_name, model_type, context_window, \
             temperature_default, max_tokens_default, price_input, price_output, enable, \
             created_by, created_at, updated_at, deleted FROM public.llm_model \
             WHERE provider_id = {} AND deleted = 0 ORDER BY model_type, model_code",
            pid
        )
    } else {
        "SELECT id, provider_id, model_code, model_name, model_type, context_window, \
         temperature_default, max_tokens_default, price_input, price_output, enable, \
         created_by, created_at, updated_at, deleted FROM public.llm_model \
         WHERE deleted = 0 ORDER BY provider_id, model_type"
            .to_string()
    };

    let list = sqlx::query_as::<_, LlmModel>(sqlx::AssertSqlSafe(sql))
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("查询模型列表失败: {}", e))?;
    Ok(list)
}

// ======================== 用户偏好 CRUD ========================

pub async fn get_user_llm_setting(user_id: i64) -> Result<Option<UserLLmSetting>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let item = sqlx::query_as::<_, UserLLmSetting>(
        "SELECT id, user_id, default_provider_id, default_chat_model_id, default_embed_model_id, \
         custom_temp, custom_max_token, created_at, updated_at, deleted \
         FROM public.user_llm_setting WHERE user_id = $1 AND deleted = 0",
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("查询用户偏好失败: {}", e))?;
    Ok(item)
}

pub async fn upsert_user_llm_setting(setting: &UserLLmSetting) -> Result<UserLLmSetting, String> {
    let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let inserted = sqlx::query_as::<_, UserLLmSetting>(
        "INSERT INTO public.user_llm_setting \
         (user_id, default_provider_id, default_chat_model_id, default_embed_model_id, custom_temp, custom_max_token, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW()) \
         ON CONFLICT (user_id) DO UPDATE SET \
         default_provider_id=$2, default_chat_model_id=$3, default_embed_model_id=$4, custom_temp=$5, custom_max_token=$6, updated_at=NOW() \
         RETURNING id, user_id, default_provider_id, default_chat_model_id, default_embed_model_id, custom_temp, custom_max_token, created_at, updated_at, deleted",
    )
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
