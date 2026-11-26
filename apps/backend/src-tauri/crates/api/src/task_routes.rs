//! 异步任务 HTTP 路由（P3 + 任务中心页）
//!
//! | 路由 | 作用 |
//! |---|---|
//! | `POST /api/tasks/parse` | 解析任务入队，返回 `202 { taskId }` |
//! | `GET /api/tasks` | 任务列表（按创建时间倒序，支持 `status` / `limit`） |
//! | `GET /api/tasks/{taskId}` | 查询任务进度（读租户 `task_ticket`） |
//!
//! 约定（与 `knowledge_asset_routes` 一致）：
//! - 租户 schema **只从鉴权上下文取**（`database::current_schema_name()`），绝不采信请求体；
//! - 路径占位符沿用命令参数名（`{taskId}`），前端 `extractParams` 才能剥离；
//! - 入队失败按错误类型映射：参数非法 → 400，队列不可用 → 503（让前端可重试）；
//! - **大整数一律字符串化**（`assetId`）：前端 JS 不能安全处理 >2^53 的 i64，
//!   参见 `docs/前端大整数精度丢失问题(Snowflake ID).md`。

use axum::Extension;
use axum::extract::{Json, Path, Query};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::info;

use task_queue::ParseTask;
use task_queue::TaskError;
use task_queue::ticket::TaskTicket;

use super::auth::UserContext;
use super::response::{ApiError, ApiResponse};

/// 最长 task id（UUID 36 字符 + 余量），超长直接 400，避免无谓查询
const MAX_TASK_ID_LEN: usize = 64;

/// 任务列表默认条数
const DEFAULT_TASK_LIST_LIMIT: i64 = 50;

/// `POST /api/tasks/parse` 请求体（字段名与前端命令参数保持一致）
#[derive(Debug, Deserialize)]
pub struct PushParseRequest {
    /// 关联的知识资产 id（字符串，避免 JS 大整数精度丢失）
    #[serde(rename = "assetId")]
    pub asset_id: String,
    /// 对象存储引用：`s3://bucket/key`（桌面调试允许 `file://` / 绝对路径）
    #[serde(rename = "fileUrl")]
    pub file_url: String,
    /// 原文件名（决定 `doc_type` 与队列路由）
    #[serde(rename = "fileName")]
    pub file_name: String,
    /// 资产标题（向量化时的 title）
    #[serde(default)]
    pub title: Option<String>,
    /// OKF 类型
    #[serde(rename = "okfType", default)]
    pub okf_type: Option<String>,
    /// 标签
    #[serde(default)]
    pub tags: Vec<String>,
    /// 知识树节点 id
    #[serde(rename = "treeNodeId", default)]
    pub tree_node_id: Option<i64>,
    /// 解析选项（`skip_index` / `frame_interval` 等，语义同 doc-parser `/parse`）
    #[serde(default)]
    pub options: serde_json::Value,
    /// 文件 md5（用于幂等键）
    #[serde(rename = "fileMd5", default)]
    pub file_md5: Option<String>,
    /// 调用链追踪 id
    #[serde(rename = "traceId", default)]
    pub trace_id: Option<String>,
}

/// 入队响应
#[derive(Debug, Serialize)]
pub struct PushParseResponse {
    /// 任务 id（UUID，用于后续进度查询）
    #[serde(rename = "taskId")]
    pub task_id: String,
    /// 关联资产 id（字符串，与前端 `KnowledgeAsset.id` 对齐）
    #[serde(rename = "assetId")]
    pub asset_id: String,
    /// 初始状态（固定 `pending`）
    pub status: String,
}

/// 任务视图（对外 DTO）
///
/// 为什么不直接返回 `ticket::TaskTicket`：
/// 1. `asset_id: Option<i64>` / `created_by: Option<i64>` 直出会让前端丢精度；
/// 2. 时间字段统一转 RFC3339 字符串，前端 `new Date(...)` 即可。
#[derive(Debug, Serialize)]
pub struct TaskItem {
    /// 任务 id
    #[serde(rename = "taskId")]
    pub task_id: String,
    /// 任务类型：parse / vectorize
    #[serde(rename = "taskType")]
    pub task_type: String,
    /// 队列名
    pub queue: String,
    /// 关联资产 id（字符串）
    #[serde(rename = "assetId")]
    pub asset_id: Option<String>,
    /// 状态：pending / running / completed / failed
    pub status: String,
    /// 阶段：queued / downloading / parsing / chunking / done
    pub stage: Option<String>,
    /// 进度 0~100
    pub progress: i16,
    /// 已尝试次数
    pub attempts: i32,
    /// 最近一次失败原因
    pub error: Option<String>,
    /// 调用链追踪 id
    #[serde(rename = "traceId")]
    pub trace_id: Option<String>,
    /// 发起人 id（字符串）
    #[serde(rename = "createdBy")]
    pub created_by: Option<String>,
    /// 入队时间（RFC3339）
    #[serde(rename = "createdAt")]
    pub created_at: String,
    /// 开始时间（RFC3339）
    #[serde(rename = "startedAt")]
    pub started_at: Option<String>,
    /// 结束时间（RFC3339）
    #[serde(rename = "finishedAt")]
    pub finished_at: Option<String>,
    /// 最近更新时间（RFC3339）
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

impl From<TaskTicket> for TaskItem {
    fn from(ticket: TaskTicket) -> Self {
        Self {
            task_id: ticket.id,
            task_type: ticket.task_type,
            queue: ticket.queue,
            asset_id: ticket.asset_id.map(|id| id.to_string()),
            status: ticket.status,
            stage: ticket.stage,
            progress: ticket.progress,
            attempts: ticket.attempts,
            error: ticket.error,
            trace_id: ticket.trace_id,
            created_by: ticket.created_by.map(|id| id.to_string()),
            created_at: ticket.created_at.to_rfc3339(),
            started_at: ticket.started_at.map(|t| t.to_rfc3339()),
            finished_at: ticket.finished_at.map(|t| t.to_rfc3339()),
            updated_at: ticket.updated_at.to_rfc3339(),
        }
    }
}

/// `GET /api/tasks` 查询参数
#[derive(Debug, Deserialize)]
pub struct ListTasksQuery {
    /// 状态过滤（缺省返回全部）
    #[serde(default)]
    pub status: Option<String>,
    /// 返回条数（缺省 50，夹到 1..=200）
    #[serde(default)]
    pub limit: Option<i64>,
}

/// `POST /api/tasks/parse` - 解析任务入队（`202 Accepted`）
pub async fn push_parse(
    user_ctx: Option<Extension<UserContext>>,
    Json(req): Json<PushParseRequest>,
) -> Result<(StatusCode, Json<ApiResponse<PushParseResponse>>), ApiError> {
    let user_id = require_user(&user_ctx)?;
    // 租户只能来自令牌上下文（auth_middleware 已注入 CURRENT_SCHEMA）
    let tenant = database::current_schema_name();
    // 资产 id 走字符串入参（前端大整数精度），这里再转回 i64
    let asset_id = req
        .asset_id
        .trim()
        .parse::<i64>()
        .map_err(|e| ApiError::bad_request(format!("无效的资产 ID: {e}")))?;

    let mut task = ParseTask::new(tenant, asset_id, req.file_url, req.file_name);
    task.title = req.title.unwrap_or_default();
    task.okf_type = req.okf_type.unwrap_or_default();
    task.tags = req.tags;
    task.tree_node_id = req.tree_node_id;
    task.options = req.options;
    task.file_md5 = req.file_md5;
    task.trace_id = req.trace_id;

    let asset_id = task.asset_id;
    let cfg = task_queue::TaskQueueConfig::from_env();
    let pool = database::get_write_pool()
        .map_err(|e| ApiError::internal_error(format!("数据库连接失败: {e}")))?;

    let task_id = task_queue::push_parse(&pool, task, &cfg, Some(user_id))
        .await
        .map_err(map_task_error)?;

    info!(
        "[HTTP] POST /api/tasks/parse user_id={} asset_id={} task_id={}",
        user_id, asset_id, task_id
    );
    Ok((
        StatusCode::ACCEPTED,
        Json(ApiResponse::success(PushParseResponse {
            task_id,
            asset_id: asset_id.to_string(),
            status: task_queue::ticket::STATUS_PENDING.to_string(),
        })),
    ))
}

/// `GET /api/tasks` - 任务列表（当前租户，按创建时间倒序）
pub async fn list_tasks(
    Query(q): Query<ListTasksQuery>,
    user_ctx: Option<Extension<UserContext>>,
) -> Result<Json<ApiResponse<Vec<TaskItem>>>, ApiError> {
    let user_id = require_user(&user_ctx)?;
    let schema = database::current_schema_name();
    let pool = database::get_write_pool()
        .map_err(|e| ApiError::internal_error(format!("数据库连接失败: {e}")))?;

    let limit = q.limit.unwrap_or(DEFAULT_TASK_LIST_LIMIT);
    let tickets = task_queue::ticket::list(&pool, &schema, q.status.as_deref(), limit)
        .await
        .map_err(map_task_error)?;

    info!(
        "[HTTP] GET /api/tasks user_id={} status={:?} count={}",
        user_id,
        q.status,
        tickets.len()
    );
    Ok(Json(ApiResponse::success(
        tickets.into_iter().map(TaskItem::from).collect(),
    )))
}

/// `GET /api/tasks/{taskId}` - 查询任务进度
pub async fn get_task_status(
    Path(taskId): Path<String>,
    user_ctx: Option<Extension<UserContext>>,
) -> Result<Json<ApiResponse<TaskItem>>, ApiError> {
    let user_id = require_user(&user_ctx)?;
    let task_id = taskId.trim();
    if task_id.is_empty() || task_id.len() > MAX_TASK_ID_LEN {
        return Err(ApiError::bad_request("无效的任务 ID".to_string()));
    }

    let schema = database::current_schema_name();
    let pool = database::get_write_pool()
        .map_err(|e| ApiError::internal_error(format!("数据库连接失败: {e}")))?;

    let ticket = task_queue::ticket::get(&pool, &schema, task_id)
        .await
        .map_err(map_task_error)?
        .ok_or_else(|| ApiError::not_found("任务不存在或已过期".to_string()))?;

    info!(
        "[HTTP] GET /api/tasks/{} user_id={} status={}",
        task_id, user_id, ticket.status
    );
    Ok(Json(ApiResponse::success(TaskItem::from(ticket))))
}

/// 取令牌主体（缺失说明路由挂在认证中间件之外）
fn require_user(user_ctx: &Option<Extension<UserContext>>) -> Result<i64, ApiError> {
    user_ctx
        .as_ref()
        .map(|Extension(ctx)| ctx.user_id)
        .ok_or_else(|| ApiError::unauthorized("未认证"))
}

/// `TaskError` → HTTP 语义
fn map_task_error(err: TaskError) -> ApiError {
    match err {
        TaskError::Invalid(message) => ApiError::bad_request(message),
        // 队列/数据库暂不可用：让前端可重试，而不是 500（500 通常会被前端当成 bug）
        TaskError::Transient(message) => ApiError::service_unavailable(message),
        TaskError::Fatal(message) => ApiError::internal_error(message),
    }
}
