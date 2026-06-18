use crate::engine::skill_context::{SkillContext, SkillResult};
use crate::engine::skill_registry::{SkillMeta, SkillRegistry};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

/// 全局 Skill 注册表状态
pub struct SkillRegistryState(pub Arc<Mutex<SkillRegistry>>);

/// 获取所有 Skill 列表
#[tauri::command]
pub async fn list_skills(
    registry: State<'_, SkillRegistryState>,
) -> Result<Vec<SkillMeta>, String> {
    let registry = registry.0.lock().await;
    let skills: Vec<SkillMeta> = registry.list_skills().into_iter().cloned().collect();
    Ok(skills)
}

/// 根据 ID 获取 Skill 详情
#[tauri::command]
pub async fn get_skill(
    registry: State<'_, SkillRegistryState>,
    skill_id: String,
) -> Result<SkillMeta, String> {
    let registry = registry.0.lock().await;
    registry
        .get_skill(&skill_id)
        .cloned()
        .ok_or_else(|| format!("Skill '{}' 未找到", skill_id))
}

/// 执行 Skill
#[tauri::command]
pub async fn execute_skill(
    registry: State<'_, SkillRegistryState>,
    skill_id: String,
    input_text: String,
    config: HashMap<String, serde_json::Value>,
    user_id: i64,
    tenant_id: i64,
) -> Result<SkillResult, String> {
    let registry = registry.0.lock().await;
    let skill = registry
        .get_skill(&skill_id)
        .ok_or_else(|| format!("Skill '{}' 未找到", skill_id))?;

    // 验证配置参数
    validate_config(&skill.config_schema, &config)?;

    // 创建执行上下文
    let ctx = SkillContext::new(input_text, config, user_id, tenant_id);

    // TODO: 实际调用 Python 运行时执行 Skill
    // 当前返回模拟结果
    let result = SkillResult::new(
        format!("> 🤖 **{} 执行结果**\n>\n> Skill '{}' 已执行完成。\n>\n> *Python 运行时尚未就绪，当前为模拟输出。*", 
                skill.name, skill.name),
        "markdown",
        "after_selection",
    );

    Ok(result)
}

/// 注册自定义 Skill
#[tauri::command]
pub async fn register_custom_skill(
    registry: State<'_, SkillRegistryState>,
    skill_meta: SkillMeta,
) -> Result<(), String> {
    let mut registry = registry.0.lock().await;
    registry.register_custom_skill(skill_meta);
    Ok(())
}

/// 移除 Skill
#[tauri::command]
pub async fn unregister_skill(
    registry: State<'_, SkillRegistryState>,
    skill_id: String,
) -> Result<bool, String> {
    let mut registry = registry.0.lock().await;
    Ok(registry.unregister_skill(&skill_id))
}

/// 获取 Skill 数量
#[tauri::command]
pub async fn get_skill_count(registry: State<'_, SkillRegistryState>) -> Result<usize, String> {
    let registry = registry.0.lock().await;
    Ok(registry.skill_count())
}

/// 验证配置参数是否符合 schema
fn validate_config(
    schema: &serde_json::Value,
    config: &HashMap<String, serde_json::Value>,
) -> Result<(), String> {
    if let serde_json::Value::Object(fields) = schema {
        for (key, field_schema) in fields {
            if let serde_json::Value::Object(field) = field_schema {
                let field_type = field
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("string");

                if let Some(value) = config.get(key) {
                    // 验证类型
                    match field_type {
                        "int" => {
                            if !value.is_number() {
                                return Err(format!("配置项 '{}' 应为整数", key));
                            }
                        }
                        "float" => {
                            if !value.is_number() {
                                return Err(format!("配置项 '{}' 应为浮点数", key));
                            }
                        }
                        "bool" => {
                            if !value.is_boolean() {
                                return Err(format!("配置项 '{}' 应为布尔值", key));
                            }
                        }
                        "string" => {
                            if !value.is_string() {
                                return Err(format!("配置项 '{}' 应为字符串", key));
                            }
                        }
                        "array" => {
                            if !value.is_array() {
                                return Err(format!("配置项 '{}' 应为数组", key));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(())
}
