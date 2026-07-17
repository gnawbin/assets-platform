use axum::{
    extract::State,
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::engine::skill_context::SkillResult;
use crate::engine::skill_registry::{SkillMeta, SkillRegistry};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// 共享 Skill 注册表状态
pub struct SkillRouterState {
    pub registry: Arc<Mutex<SkillRegistry>>,
}

/// 构建 Skill 路由
pub fn skill_routes() -> Router<Arc<SkillRouterState>> {
    Router::new()
        .route("/api/skills", get(list_skills_handler))
        .route("/api/skills/{skill_id}", get(get_skill_handler))
        .route("/api/skills/execute", post(execute_skill_handler))
        .route("/api/skills/register", post(register_skill_handler))
        .route("/api/skills/{skill_id}", delete(unregister_skill_handler))
        .route("/api/skills/count", get(get_skill_count_handler))
}

/// 获取所有 Skill 列表
async fn list_skills_handler(State(state): State<Arc<SkillRouterState>>) -> Json<Vec<SkillMeta>> {
    let registry = state.registry.lock().await;
    let skills: Vec<SkillMeta> = registry.list_skills().into_iter().cloned().collect();
    Json(skills)
}

/// 获取单个 Skill
async fn get_skill_handler(
    State(state): State<Arc<SkillRouterState>>,
    axum::extract::Path(skill_id): axum::extract::Path<String>,
) -> Result<Json<SkillMeta>, String> {
    let registry = state.registry.lock().await;
    registry
        .get_skill(&skill_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| format!("Skill '{}' 未找到", skill_id))
}

/// 执行 Skill 请求体
#[derive(Debug, Deserialize)]
pub struct ExecuteSkillRequest {
    pub skill_id: String,
    pub input_text: String,
    pub config: HashMap<String, serde_json::Value>,
    pub user_id: i64,
    pub tenant_id: i64,
}

/// 执行 Skill
async fn execute_skill_handler(
    State(state): State<Arc<SkillRouterState>>,
    Json(req): Json<ExecuteSkillRequest>,
) -> Result<Json<SkillResult>, String> {
    let registry = state.registry.lock().await;
    let skill = registry
        .get_skill(&req.skill_id)
        .ok_or_else(|| format!("Skill '{}' 未找到", req.skill_id))?;

    // TODO: 实际调用 Python 运行时
    let result = SkillResult::new(
        format!(
            "> 🤖 **{} 执行结果**\n>\n> Skill '{}' 已执行完成。\n>\n> *Python 运行时尚未就绪，当前为模拟输出。*",
            skill.name, skill.name
        ),
        "markdown",
        "after_selection",
    );

    Ok(Json(result))
}

/// 注册自定义 Skill
async fn register_skill_handler(
    State(state): State<Arc<SkillRouterState>>,
    Json(skill_meta): Json<SkillMeta>,
) -> Json<()> {
    let mut registry = state.registry.lock().await;
    registry.register_custom_skill(skill_meta);
    Json(())
}

/// 移除 Skill
async fn unregister_skill_handler(
    State(state): State<Arc<SkillRouterState>>,
    axum::extract::Path(skill_id): axum::extract::Path<String>,
) -> Json<bool> {
    let mut registry = state.registry.lock().await;
    Json(registry.unregister_skill(&skill_id))
}

/// 获取 Skill 数量
async fn get_skill_count_handler(State(state): State<Arc<SkillRouterState>>) -> Json<usize> {
    let registry = state.registry.lock().await;
    Json(registry.skill_count())
}
