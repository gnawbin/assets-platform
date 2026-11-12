use axum::{
    extract::State,
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;

use engine::skill_context::SkillResult;
use engine::skill_registry::{SkillMeta, SkillRegistry};
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
#[utoipa::path(
    get,
    path = "/api/skills",
    tag = "技能",
    responses(
        (status = 200, description = "Skill 元数据数组（结构与 engine::skill_registry::SkillMeta 一致）", body = Vec<serde_json::Value>),
    )
)]
async fn list_skills_handler(State(state): State<Arc<SkillRouterState>>) -> Json<Vec<SkillMeta>> {
    let registry = state.registry.lock().await;
    let skills: Vec<SkillMeta> = registry.list_skills().into_iter().cloned().collect();
    Json(skills)
}

/// 获取单个 Skill
#[utoipa::path(
    get,
    path = "/api/skills/{skill_id}",
    tag = "技能",
    responses(
        (status = 200, description = "Skill 元数据", body = serde_json::Value),
        (status = 400, description = "Skill 未找到（返回纯文本错误信息）"),
    )
)]
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
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ExecuteSkillRequest {
    /// Skill 标识
    pub skill_id: String,
    /// 输入文本
    pub input_text: String,
    /// Skill 配置（自由结构）
    pub config: HashMap<String, serde_json::Value>,
    /// 用户 ID（雪花 ID）
    pub user_id: i64,
    /// 租户 ID（雪花 ID）
    pub tenant_id: i64,
}

/// 执行 Skill
#[utoipa::path(
    post,
    path = "/api/skills/execute",
    tag = "技能",
    request_body = ExecuteSkillRequest,
    responses(
        (status = 200, description = "执行结果（结构与 engine::skill_context::SkillResult 一致）", body = serde_json::Value),
        (status = 400, description = "Skill 未找到（返回纯文本错误信息）"),
    )
)]
async fn execute_skill_handler(
    State(state): State<Arc<SkillRouterState>>,
    Json(req): Json<ExecuteSkillRequest>,
) -> Result<Json<SkillResult>, String> {
    // 先在锁内取出名称，随后立即释放锁（执行可能耗时数十秒）
    let skill_name = {
        let registry = state.registry.lock().await;
        registry
            .get_skill(&req.skill_id)
            .map(|skill| skill.name.clone())
            .ok_or_else(|| format!("Skill '{}' 未找到", req.skill_id))?
    };

    // 待执行代码：优先取 config.code（显式脚本），否则把选中文本交给沙箱解释器
    let code = req
        .config
        .get("code")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| req.input_text.clone());

    let key = service::sandbox_service::SessionKey::new(
        database::current_schema_name(),
        req.user_id,
        format!("skill:{}", req.skill_id),
    );

    // 沙箱不可用时也返回 200 + 可读 markdown（保持改造前的响应契约，不抛 5xx）
    match service::sandbox_service::exec_code(key, code, None).await {
        Ok(result) => {
            let output = if result.is_ok() {
                result.combined_output()
            } else {
                let detail = result
                    .error
                    .as_ref()
                    .map_or_else(|| result.combined_output(), |failure| {
                        format!("{}: {}", failure.name, failure.value)
                    });
                format!("> ⚠️ **{} 执行失败**（exit {})\n>\n> {detail}", skill_name, result.exit_code)
            };

            let mut metadata = HashMap::new();
            metadata.insert("skill_id".to_string(), req.skill_id.clone());
            metadata.insert("exit_code".to_string(), result.exit_code.to_string());
            metadata.insert("duration_ms".to_string(), result.duration_ms.to_string());
            metadata.insert("sandbox".to_string(), "cube".to_string());

            Ok(Json(
                SkillResult::new(output, "markdown", "after_selection").with_metadata(metadata),
            ))
        }
        Err(message) => Ok(Json(SkillResult::new(
            format!("> ⚠️ **{skill_name} 未能执行**\n>\n> {message}"),
            "markdown",
            "after_selection",
        ))),
    }
}

/// 注册自定义 Skill
#[utoipa::path(
    post,
    path = "/api/skills/register",
    tag = "技能",
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "注册成功（`data` 为 null）"),
    )
)]
async fn register_skill_handler(
    State(state): State<Arc<SkillRouterState>>,
    Json(skill_meta): Json<SkillMeta>,
) -> Json<()> {
    let mut registry = state.registry.lock().await;
    registry.register_custom_skill(skill_meta);
    Json(())
}

/// 移除 Skill
#[utoipa::path(
    delete,
    path = "/api/skills/{skill_id}",
    tag = "技能",
    responses(
        (status = 200, description = "是否移除成功", body = bool),
    )
)]
async fn unregister_skill_handler(
    State(state): State<Arc<SkillRouterState>>,
    axum::extract::Path(skill_id): axum::extract::Path<String>,
) -> Json<bool> {
    let mut registry = state.registry.lock().await;
    Json(registry.unregister_skill(&skill_id))
}

/// 获取 Skill 数量
#[utoipa::path(
    get,
    path = "/api/skills/count",
    tag = "技能",
    responses(
        (status = 200, description = "已注册 Skill 数量", body = usize),
    )
)]
async fn get_skill_count_handler(State(state): State<Arc<SkillRouterState>>) -> Json<usize> {
    let registry = state.registry.lock().await;
    Json(registry.skill_count())
}
