//! 单据编号规则 HTTP API 路由
//!
//! 对应原 Tauri 命令：`get_numbering_rules` / `get_numbering_rule` /
//! `save_numbering_rule` / `reset_numbering_sequence`。
//!
//! # 请求形状约定（全 HTTP 化统一原则）
//!
//! 前端 `httpAdapter.extractParams()` 只把**路径占位符**对应的参数从请求体里剥掉，
//! 其余参数原样作为 JSON body 发送。因此路由的 body 形状必须与命令参数形状逐字一致：
//!
//! | 命令 | 路由 | body |
//! | --- | --- | --- |
//! | `get_numbering_rules()` | `GET /api/numbering/rules` | 无 |
//! | `get_numbering_rule(bizType)` | `GET /api/numbering/rules/{bizType}` | 无 |
//! | `save_numbering_rule(id, input)` | `POST /api/numbering/rules` | `{"id":"…","input":{…}}` |
//! | `reset_numbering_sequence(bizType, resetKey)` | `POST /api/numbering/rules/{bizType}/reset` | `{"resetKey":"…"}` |
//!
//! 若路径占位符与命令参数名不一致，`extractParams` 就匹配不到，参数会残留在 body 里
//! 导致反序列化失败——这是新增路由最容易踩的坑。

use axum::{extract::Path, Extension, Json};
use serde::Deserialize;
use tracing::info;

use service::numbering_service::{self, NumberingRuleInput, NumberingRuleResponse};

use super::auth::UserContext;
use super::response::{ApiError, ApiResponse};

// ======================== 请求 DTO ========================

/// 编号规则内容（与 `NumberingRuleInput` 字段一一对应）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NumberingRulePayload {
    /// 业务类型（asset / receive / …）
    pub biz_type: String,
    /// 业务名称
    pub biz_name: String,
    /// 前缀（如 `ZC`）
    pub prefix: Option<String>,
    /// 日期格式（如 `yyyyMMdd`）
    pub date_format: Option<String>,
    /// 日期位置：prefix / suffix
    pub date_position: Option<String>,
    /// 流水号长度
    pub serial_length: i32,
    /// 分隔符
    pub separator: Option<String>,
    /// 重置方式：never / yearly / monthly
    pub reset_mode: Option<String>,
    /// 是否启用
    pub is_active: bool,
}

impl From<NumberingRulePayload> for NumberingRuleInput {
    fn from(p: NumberingRulePayload) -> Self {
        Self {
            biz_type: p.biz_type,
            biz_name: p.biz_name,
            prefix: p.prefix,
            date_format: p.date_format,
            date_position: p.date_position,
            serial_length: p.serial_length,
            separator: p.separator,
            reset_mode: p.reset_mode,
            is_active: p.is_active,
        }
    }
}

/// 保存规则请求：`{ id?, input }`（与命令参数同名，前端无需转换）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SaveRuleRequest {
    /// 规则 ID（字符串形式的 i64），为空表示新增
    #[serde(default)]
    pub id: Option<String>,
    /// 规则内容
    pub input: NumberingRulePayload,
}

/// 重置流水号请求：`{ resetKey }`
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ResetSequenceRequest {
    /// 重置键（如年份 `2026`，由前端按 reset_mode 计算）
    #[serde(rename = "resetKey")]
    pub reset_key: String,
}

// ======================== 错误映射 ========================

/// 把 service 层的 `Result<_, String>` 映射为 HTTP 语义
///
/// 约定：`未找到` → 404；其余按 500 处理（service 层不区分错误类别，
/// 不在 HTTP 层臆造 `400`，避免把内部错误误报成参数问题）。
fn map_service_error(message: String) -> ApiError {
    if message.contains("未找到") {
        ApiError::not_found(message)
    } else {
        ApiError::internal_error(message)
    }
}

/// 解析可选的字符串 ID（空串视为 None）
fn parse_opt_id(raw: Option<String>) -> Result<Option<i64>, ApiError> {
    match raw {
        Some(s) if !s.trim().is_empty() => s
            .trim()
            .parse::<i64>()
            .map(Some)
            .map_err(|e| ApiError::bad_request(format!("无效的规则ID: {}", e))),
        _ => Ok(None),
    }
}

// ======================== 路由 handler ========================

/// GET /api/numbering/rules - 获取所有编号规则
#[utoipa::path(
    get,
    path = "/api/numbering/rules",
    tag = "单据编号",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "编号规则列表（`data` 为 `NumberingRuleResponse[]`）"),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn get_rules() -> Result<Json<ApiResponse<Vec<NumberingRuleResponse>>>, ApiError> {
    info!("[HTTP] GET /api/numbering/rules");

    let rules = numbering_service::get_rules()
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(rules)))
}

/// GET /api/numbering/rules/{bizType} - 按业务类型获取单条规则
#[utoipa::path(
    get,
    path = "/api/numbering/rules/{bizType}",
    tag = "单据编号",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "编号规则（`data` 为 `NumberingRuleResponse`）"),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "业务类型未配置编号规则", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn get_rule(
    Path(biz_type): Path<String>,
) -> Result<Json<ApiResponse<NumberingRuleResponse>>, ApiError> {
    info!("[HTTP] GET /api/numbering/rules/{}", biz_type);

    let rule = numbering_service::get_rule(&biz_type)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(rule)))
}

/// POST /api/numbering/rules - 保存编号规则（新增或更新）
///
/// body: `{ "id": "1" | null, "input": { … } }`；`id` 有值即更新。
#[utoipa::path(
    post,
    path = "/api/numbering/rules",
    tag = "单据编号",
    security(("bearer_auth" = [])),
    request_body = SaveRuleRequest,
    responses(
        (status = 200, description = "保存后的编号规则"),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn save_rule(
    user_ctx: Option<Extension<UserContext>>,
    Json(req): Json<SaveRuleRequest>,
) -> Result<Json<ApiResponse<NumberingRuleResponse>>, ApiError> {
    let rule_id = parse_opt_id(req.id)?;
    info!(
        "[HTTP] POST /api/numbering/rules biz_type={} id={:?}",
        req.input.biz_type, rule_id
    );

    // 审计字段取令牌身份；无令牌（如服务身份）时留空，由 DB 默认值兜底
    let current_user_id = user_ctx.map(|Extension(ctx)| ctx.user_id);

    let rule = numbering_service::save_rule(rule_id, req.input.into(), current_user_id)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(rule)))
}

/// POST /api/numbering/rules/{bizType}/reset - 重置流水号
///
/// body: `{ "resetKey": "2026" }`
#[utoipa::path(
    post,
    path = "/api/numbering/rules/{bizType}/reset",
    tag = "单据编号",
    security(("bearer_auth" = [])),
    request_body = ResetSequenceRequest,
    responses(
        (status = 200, description = "重置成功（`data` 为 null）"),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn reset_sequence(
    Path(biz_type): Path<String>,
    Json(req): Json<ResetSequenceRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    info!("[HTTP] POST /api/numbering/rules/{}/reset", biz_type);

    numbering_service::reset_sequence(&biz_type, &req.reset_key)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_id_is_treated_as_create() {
        assert_eq!(parse_opt_id(None).expect("None 应合法"), None);
        assert_eq!(
            parse_opt_id(Some("  ".to_string())).expect("空白应合法"),
            None
        );
    }

    #[test]
    fn numeric_id_is_parsed() {
        assert_eq!(
            parse_opt_id(Some("123".to_string())).expect("数字应合法"),
            Some(123)
        );
    }

    #[test]
    fn invalid_id_becomes_400() {
        let err = parse_opt_id(Some("abc".to_string())).expect_err("非数字应报错");
        assert_eq!(err.code, 400);
    }

    #[test]
    fn not_found_message_maps_to_404() {
        assert_eq!(
            map_service_error("未找到业务类型 'x' 的编号规则".to_string()).code,
            404
        );
        assert_eq!(map_service_error("数据库连接失败".to_string()).code, 500);
    }

    #[test]
    fn reset_body_uses_camel_case_key() {
        let req: ResetSequenceRequest =
            serde_json::from_str(r#"{"resetKey":"2026"}"#).expect("应可反序列化");
        assert_eq!(req.reset_key, "2026");
    }

    #[test]
    fn save_body_matches_command_arguments() {
        let req: SaveRuleRequest = serde_json::from_str(
            r#"{"id":"9","input":{"biz_type":"asset","biz_name":"资产编号",
                "prefix":"ZC","date_format":"yyyyMMdd","date_position":"prefix",
                "serial_length":4,"separator":"-","reset_mode":"never","is_active":true}}"#,
        )
        .expect("应可反序列化");
        assert_eq!(req.id.as_deref(), Some("9"));
        let input: NumberingRuleInput = req.input.into();
        assert_eq!(input.biz_type, "asset");
        assert_eq!(input.serial_length, 4);
        assert!(input.is_active);
    }
}
