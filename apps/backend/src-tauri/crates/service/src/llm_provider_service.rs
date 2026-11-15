//! LLM 厂商/模型/偏好 CRUD Service
//!
//! 提供厂商、模型、用户偏好的增删改查操作。

use database;
use database::models::{LlmModel, LlmProvider, UserLLmSetting};

use crate::llm_factory::{create_provider as build_provider, resolve_api_key};

/// LLM 厂商不存在（含已软删除）
///
/// 文案必须带「未找到」——`crates/api` 的 `map_service_error` 依赖该关键字映射为 **404**。
pub const LLM_PROVIDER_NOT_FOUND: &str = "LLM 厂商未找到（不存在或已删除）";

/// LLM 模型不存在（含已软删除）
pub const LLM_MODEL_NOT_FOUND: &str = "LLM 模型未找到（不存在或已删除）";

/// 响应里 API Key 的脱敏占位符
///
/// 厂商列表/详情对外一律返回该常量（不返回密文，更不返回明文）；
/// 前端回传该值时视为「未修改」，不会被当成新 Key 保存。
pub const API_KEY_MASK: &str = "******";

/// 构造厂商实体（含 API Key 加密）
///
/// 桌面命令 `create_llm_provider` 与 HTTP 路由 `POST /api/llm/providers` 共用，
/// 保证默认值一致：`enable = true`、`is_local` 缺省 `false`、`secret_key`/`extra_config` 留空。
pub fn build_provider_entity(
    provider_code: String,
    provider_name: String,
    base_url: Option<String>,
    api_key: Option<String>,
    weight: Option<i32>,
    is_local: Option<bool>,
    created_by: Option<i64>,
) -> LlmProvider {
    // 加密 API Key（加密失败时保留原值，与桌面命令行为一致）
    let encrypted_key = match &api_key {
        Some(key) if !key.is_empty() => match utils::crypto::encrypt_api_key(key) {
            Ok(ek) => Some(ek),
            Err(_) => api_key.clone(),
        },
        _ => api_key.clone(),
    };

    LlmProvider {
        id: 0,
        provider_code,
        provider_name,
        base_url,
        api_key: encrypted_key,
        secret_key: None,
        extra_config: None,
        weight,
        is_local: is_local.unwrap_or(false),
        enable: true,
        created_by,
        created_at: None,
        updated_at: None,
        deleted: 0,
    }
}


// ======================== 厂商 CRUD ========================

pub async fn get_providers() -> Result<Vec<LlmProvider>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();
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
    let pool = database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();
    let sql = format!(
        "SELECT id, provider_code, provider_name, base_url, api_key, secret_key, \
         extra_config, weight, is_local, enable, created_by, created_at, updated_at, deleted \
         FROM {}llm_provider WHERE id = $1 AND deleted = 0",
        prefix
    );
    let item = sqlx::query_as::<_, LlmProvider>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| format!("查询厂商失败: {}", e))?;
    item.ok_or_else(|| LLM_PROVIDER_NOT_FOUND.to_string())
}

pub async fn create_provider(p: &LlmProvider) -> Result<LlmProvider, String> {
    let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();
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
    let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();
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
    let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();
    let sql = format!(
        "UPDATE {}llm_provider SET deleted=1 WHERE id=$1 AND deleted=0",
        prefix
    );
    let result = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| format!("删除厂商失败: {}", e))?;
    if result.rows_affected() == 0 {
        return Err(LLM_PROVIDER_NOT_FOUND.to_string());
    }
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
    let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();
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
    let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();
    let existing = sqlx::query_as::<_, LlmModel>(sqlx::AssertSqlSafe(format!(
        "SELECT id, provider_id, model_code, model_name, model_type, context_window, \
         temperature_default, max_tokens_default, price_input, price_output, enable, \
         created_by, created_at, updated_at, deleted FROM {}llm_model WHERE id = $1 AND deleted = 0",
        prefix
    )))
    .bind(id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("查询模型失败: {}", e))?
    .ok_or_else(|| LLM_MODEL_NOT_FOUND.to_string())?;

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
    let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();
    let sql = format!(
        "UPDATE {}llm_model SET deleted=1 WHERE id=$1 AND deleted=0",
        prefix
    );
    let result = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| format!("删除模型失败: {}", e))?;
    if result.rows_affected() == 0 {
        return Err(LLM_MODEL_NOT_FOUND.to_string());
    }
    Ok(())
}

/// 从厂商接口拉取模型列表并批量保存
///
/// 走 [`build_provider`] 构造的协议实现：OpenAI 兼容厂商用 `/models`，
/// Anthropic 用 `/v1/models`（端点不存在时回落到默认模型）。
pub async fn fetch_models_from_api(provider_id: i64) -> Result<Vec<LlmModel>, String> {
    let provider = get_provider(provider_id).await?;

    if resolve_api_key(provider.api_key.as_ref()).is_empty() {
        return Err("厂商未配置 API Key".to_string());
    }

    let implementation = build_provider(&provider, None)
        .map_err(|error| format!("{}: {}", error.kind(), error))?;
    let models = implementation
        .list_models()
        .await
        .map_err(|error| format!("{}: {}", error.kind(), error))?;

    if models.is_empty() {
        return Err("API 返回的模型列表为空".to_string());
    }

    // 分类并批量保存
    let mut saved = Vec::new();
    for info in &models {
        let model_type = classify_model_name(&info.id);
        let context_window = estimate_context_window(&info.id);

        let model = create_model(
            provider_id,
            &info.id,
            &info.id,
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

/// 根据模型名称猜测类型（chat / embedding / vision）
fn classify_model_name(name: &str) -> &'static str {
    let lower = name.to_lowercase();

    let is_embedding = lower.contains("embedding")
        || lower.contains("ada")
        || lower.contains("bge")
        || lower.contains("m3e")
        || lower.contains("text-embedding");
    if is_embedding {
        return "embedding";
    }

    let is_vision = lower.contains("vision")
        || lower.contains("-vl")
        || lower.contains("vl-")
        || lower.contains("omni")
        || lower.contains("llava")
        || lower.contains("glm-4v")
        || lower.ends_with("-v");
    if is_vision {
        return "vision";
    }

    "chat"
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
    let pool = database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();
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
    let pool = database::get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 新建厂商：默认值 + API Key 加密入库（明文不得直存）
    #[test]
    fn build_provider_encrypts_api_key() {
        // SAFETY: edition 2024 起 set_var 为 unsafe；本测试独占该变量。
        unsafe { std::env::set_var("LLM_KEY_ENCRYPT_KEY", "abcdefghijklmnopqrstuvwxyz123456") };

        let p = build_provider_entity(
            "openai".to_string(),
            "OpenAI".to_string(),
            Some("https://api.openai.com/v1".to_string()),
            Some("sk-test-api-key-12345".to_string()),
            Some(10),
            None,
            Some(7),
        );

        assert_eq!(p.id, 0, "自增列占位为 0");
        assert!(p.enable, "新建厂商默认启用");
        assert!(!p.is_local, "is_local 缺省 false");
        assert_eq!(p.weight, Some(10));
        assert_eq!(p.created_by, Some(7));
        assert_eq!(p.deleted, 0);
        assert!(p.secret_key.is_none());
        assert!(p.extra_config.is_none());

        let stored = p.api_key.expect("应写入密钥");
        assert_ne!(stored, "sk-test-api-key-12345", "明文不得直接入库");
        assert_eq!(
            utils::crypto::decrypt_api_key(&stored).expect("密文应可解密"),
            "sk-test-api-key-12345"
        );

        // SAFETY: 同上，清理本测试设置的变量。
        unsafe { std::env::remove_var("LLM_KEY_ENCRYPT_KEY") };
    }

    /// 空串 / 缺省密钥原样保留（与桌面命令一致），空串不会被加密成密文
    #[test]
    fn build_provider_keeps_blank_key() {
        let p = build_provider_entity(
            "local".to_string(),
            "本地模型".to_string(),
            None,
            Some(String::new()),
            None,
            Some(true),
            None,
        );
        assert_eq!(p.api_key.as_deref(), Some(""));
        assert!(p.is_local);
        assert!(p.created_by.is_none());

        let p = build_provider_entity(
            "local".to_string(),
            "本地模型".to_string(),
            None,
            None,
            None,
            None,
            None,
        );
        assert!(p.api_key.is_none());
    }

    /// 文案必须保留「未找到」，否则 api 层无法映射成 404
    #[test]
    fn not_found_messages_map_to_404() {
        assert!(LLM_PROVIDER_NOT_FOUND.contains("未找到"));
        assert!(LLM_MODEL_NOT_FOUND.contains("未找到"));
        assert!(!API_KEY_MASK.contains("sk-"), "脱敏占位符不得泄露真实前缀");
    }
}

