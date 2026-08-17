//! LLM 厂商/模型 Tauri Command

use assets_database::models::{LlmModel, LlmProvider, UserLLmSetting};
use assets_service::llm_provider_service;

/// 获取厂商列表
#[tauri::command]
pub async fn get_llm_providers() -> Result<Vec<LlmProvider>, String> {
    llm_provider_service::get_providers().await
}

/// 获取单个厂商
#[tauri::command]
pub async fn get_llm_provider(id: String) -> Result<LlmProvider, String> {
    let pid: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    llm_provider_service::get_provider(pid).await
}

/// 创建厂商
#[tauri::command]
pub async fn create_llm_provider(
    providerCode: String,
    providerName: String,
    baseUrl: Option<String>,
    apiKey: Option<String>,
    weight: Option<i32>,
    isLocal: Option<bool>,
    currentUserId: Option<String>,
) -> Result<LlmProvider, String> {
    // 加密 API Key
    let encrypted_key = match &apiKey {
        Some(key) if !key.is_empty() => match assets_utils::crypto::encrypt_api_key(key) {
            Ok(ek) => Some(ek),
            Err(_) => apiKey.clone(),
        },
        _ => apiKey.clone(),
    };

    let user_id = currentUserId.and_then(|id| id.parse().ok());

    let provider = LlmProvider {
        id: 0,
        provider_code: providerCode,
        provider_name: providerName,
        base_url: baseUrl,
        api_key: encrypted_key,
        secret_key: None,
        extra_config: None,
        weight,
        is_local: isLocal.unwrap_or(false),
        enable: true,
        created_by: user_id,
        created_at: None,
        updated_at: None,
        deleted: 0,
    };

    llm_provider_service::create_provider(&provider).await
}

/// 更新厂商
#[tauri::command]
pub async fn update_llm_provider(
    id: String,
    providerCode: Option<String>,
    providerName: Option<String>,
    baseUrl: Option<String>,
    apiKey: Option<String>,
    weight: Option<i32>,
    enable: Option<bool>,
) -> Result<LlmProvider, String> {
    let pid: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    let existing = llm_provider_service::get_provider(pid).await?;

    let code = providerCode.unwrap_or(existing.provider_code);
    let name = providerName.unwrap_or(existing.provider_name);

    // 加密新 Key
    let key = match &apiKey {
        Some(k) if !k.is_empty() => match assets_utils::crypto::encrypt_api_key(k) {
            Ok(ek) => Some(ek),
            Err(_) => Some(k.clone()),
        },
        _ => None,
    };

    llm_provider_service::update_provider(
        pid,
        &code,
        &name,
        baseUrl.as_deref(),
        key.as_deref(),
        weight,
        enable,
    )
    .await
}

/// 删除厂商
#[tauri::command]
pub async fn delete_llm_provider(id: String) -> Result<(), String> {
    let pid: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    llm_provider_service::delete_provider(pid).await
}

/// 获取模型列表
#[tauri::command]
pub async fn get_llm_models(providerId: Option<String>) -> Result<Vec<LlmModel>, String> {
    let pid = match providerId {
        Some(ref id) if !id.is_empty() => {
            Some(id.parse::<i64>().map_err(|e| format!("无效的ID: {}", e))?)
        }
        _ => None,
    };
    llm_provider_service::get_models(pid).await
}

/// 创建模型
#[tauri::command]
pub async fn create_llm_model(
    providerId: String,
    modelCode: String,
    modelName: String,
    modelType: String,
    contextWindow: Option<i32>,
    temperatureDefault: Option<f64>,
    maxTokensDefault: Option<i32>,
    enable: Option<bool>,
) -> Result<LlmModel, String> {
    let pid: i64 = providerId
        .parse()
        .map_err(|e| format!("无效的厂商ID: {}", e))?;
    llm_provider_service::create_model(
        pid,
        &modelCode,
        &modelName,
        &modelType,
        contextWindow,
        temperatureDefault,
        maxTokensDefault,
        enable,
    )
    .await
}

/// 更新模型
#[tauri::command]
pub async fn update_llm_model(
    id: String,
    modelCode: Option<String>,
    modelName: Option<String>,
    modelType: Option<String>,
    contextWindow: Option<i32>,
    temperatureDefault: Option<f64>,
    maxTokensDefault: Option<i32>,
    enable: Option<bool>,
) -> Result<LlmModel, String> {
    let mid: i64 = id.parse().map_err(|e| format!("无效的模型ID: {}", e))?;
    llm_provider_service::update_model(
        mid,
        modelCode.as_deref(),
        modelName.as_deref(),
        modelType.as_deref(),
        contextWindow,
        temperatureDefault,
        maxTokensDefault,
        enable,
    )
    .await
}

/// 删除模型
#[tauri::command]
pub async fn delete_llm_model(id: String) -> Result<(), String> {
    let mid: i64 = id.parse().map_err(|e| format!("无效的模型ID: {}", e))?;
    llm_provider_service::delete_model(mid).await
}

/// 从 OpenAI 兼容接口拉取模型列表
#[tauri::command]
pub async fn fetch_llm_models(providerId: String) -> Result<Vec<LlmModel>, String> {
    let pid: i64 = providerId
        .parse()
        .map_err(|e| format!("无效的厂商ID: {}", e))?;
    llm_provider_service::fetch_models_from_api(pid).await
}

/// 获取用户模型偏好
#[tauri::command]
pub async fn get_user_llm_setting(userId: String) -> Result<Option<UserLLmSetting>, String> {
    let user_id: i64 = userId.parse().map_err(|e| format!("无效的用户ID: {}", e))?;
    llm_provider_service::get_user_llm_setting(user_id).await
}

/// 保存用户模型偏好
#[tauri::command]
pub async fn save_user_llm_setting(
    userId: String,
    defaultProviderId: Option<String>,
    defaultChatModelId: Option<String>,
    defaultEmbedModelId: Option<String>,
    customTemp: Option<f64>,
    customMaxToken: Option<i32>,
) -> Result<UserLLmSetting, String> {
    let user_id: i64 = userId.parse().map_err(|e| format!("无效的用户ID: {}", e))?;
    let dpid = match defaultProviderId {
        Some(ref id) if !id.is_empty() => Some(
            id.parse::<i64>()
                .map_err(|e| format!("无效的厂商ID: {}", e))?,
        ),
        _ => None,
    };
    let dcmid = match defaultChatModelId {
        Some(ref id) if !id.is_empty() => Some(
            id.parse::<i64>()
                .map_err(|e| format!("无效的模型ID: {}", e))?,
        ),
        _ => None,
    };
    let demid = match defaultEmbedModelId {
        Some(ref id) if !id.is_empty() => Some(
            id.parse::<i64>()
                .map_err(|e| format!("无效的模型ID: {}", e))?,
        ),
        _ => None,
    };

    let setting = UserLLmSetting {
        id: 0,
        user_id,
        default_provider_id: dpid,
        default_chat_model_id: dcmid,
        default_embed_model_id: demid,
        custom_temp: customTemp,
        custom_max_token: customMaxToken,
        created_at: None,
        updated_at: None,
        deleted: 0,
    };

    llm_provider_service::upsert_user_llm_setting(&setting).await
}
