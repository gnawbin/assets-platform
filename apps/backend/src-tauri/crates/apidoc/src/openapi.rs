//! OpenAPI 3.1 文档定义
//!
//! `#[derive(OpenApi)]` 的 `paths(...)` 只登记**已经**在 `crates/api` 打了
//! `#[utoipa::path]` 的 handler；新增接口时同步把函数路径加进来即可（漏登记不会
//! 报错，但接口不会出现在文档里，`spec_is_serializable_and_documented` 测试会兜底
//! 检查数量与安全方案）。

use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

/// 资产平台 HTTP API 文档
#[derive(OpenApi)]
#[openapi(
    info(
        title = "资产平台 HTTP API",
        version = "0.1.0",
        description = "资产平台（固定资产 / 无形资产 / 知识库 / 流程 / 运维采集）的 RESTful HTTP 接口。\n\n\
**统一响应结构**：`{ \"code\": 0, \"message\": \"success\", \"data\": ... }`；失败时 `code` 为 HTTP 语义码\n\
（400/401/404/500），响应体为 `{ \"code\": 400, \"message\": \"...\" }`。\n\n\
**调度方式**：除 `POST /api/auth/login`、`POST /api/auth/register` 外，其余接口都需要在请求头\n\
携带 `Authorization: Bearer <token>`；点击右上角 Authorize 填入 token 后即可在本页调试。\n\n\
**ID 约定**：雪花 ID 在响应 JSON 中统一序列化为字符串（避免 JS 精度丢失），请求侧同时接受\n\
字符串与数字。\n\n\
**令牌生命周期**：`POST /api/auth/login` 同时返回 `token`（访问令牌，默认 7 天，可用\n\
`JWT_ACCESS_TTL_SECS` 调整）与 `refresh_token`（刷新令牌，默认 30 天，可用 `JWT_REFRESH_TTL_SECS`\n\
调整）。访问令牌过期后调用 `POST /api/auth/refresh` 换取新的一对令牌（旧 refresh 立即失效，\n\
采用轮换机制）；若旧 refresh 被重复使用，该次登录派生的整条令牌链都会被吊销。\n\
`POST /api/auth/logout` 撤销当前 refresh；用户被禁用、改密或删除时，其全部 refresh 立即吊销。"
    ),
    servers((url = "/", description = "当前服务")),
    tags(
        (name = "认证", description = "登录 / 注册申请（公开接口）"),
        (name = "用户", description = "用户管理与个人信息"),
        (name = "注册审核", description = "注册申请的审核与驳回"),
        (name = "资产分类", description = "资产分类树维护"),
        (name = "固定资产", description = "固定资产台账"),
        (name = "无形资产", description = "无形资产台账"),
        (name = "部门", description = "组织架构维护"),
        (name = "角色", description = "角色与用户角色绑定"),
        (name = "菜单", description = "菜单权限与侧边栏"),
        (name = "租户", description = "租户（schema）管理与切换"),
        (name = "知识库", description = "知识树与知识条目"),
        (name = "流程管理-领用", description = "资产领用流程"),
        (name = "流程管理-归还", description = "资产归还流程"),
        (name = "流程管理-调拨", description = "资产调拨流程"),
        (name = "流程管理-维修", description = "资产维修流程"),
        (name = "流程管理-报废", description = "资产报废流程"),
        (name = "流程管理-采购", description = "资产采购流程"),
        (name = "运维采集", description = "Docker / Swarm / Kubernetes 只读采集"),
        (name = "大文件上传", description = "S3 分片上传与断点续传"),
        (name = "智能问答", description = "RAG + LLM 的 SSE 流式问答"),
        (name = "技能", description = "Agent Skill 注册与执行"),
        (name = "API Key", description = "外部系统 API Key 管理（JWT 管理员专用，API Key 自身禁止管理）"),
    ),
    // 只登记已标注 #[utoipa::path] 的 handler；新增接口时同步追加。
    paths(
        // 认证 / 用户
        api::user_routes::login,
        api::user_routes::refresh,
        api::user_routes::logout,
        api::user_routes::get_current_user,
        api::user_routes::get_users,
        api::user_routes::insert_user,
        api::user_routes::update_user,
        api::user_routes::reset_password,
        api::user_routes::delete_user,
        // 注册审核
        api::register_routes::register,
        api::register_routes::get_registrations,
        api::register_routes::approve_registration,
        api::register_routes::reject_registration,
        // 资产分类
        api::category_routes::get_categories,
        api::category_routes::get_categories_parents,
        api::category_routes::insert_category,
        api::category_routes::update_category,
        api::category_routes::delete_category,
        // 固定资产 / 无形资产
        api::asset_routes::get_hardware_assets,
        api::asset_routes::insert_hardware_asset,
        api::asset_routes::update_hardware_asset,
        api::asset_routes::delete_hardware_asset,
        api::asset_routes::get_intangible_assets,
        api::asset_routes::insert_intangible_asset,
        api::asset_routes::update_intangible_asset,
        api::asset_routes::delete_intangible_asset,
        // 部门
        api::department_routes::get_departments,
        api::department_routes::insert_department,
        api::department_routes::update_department,
        api::department_routes::delete_department,
        // 角色 / 菜单
        api::role_routes::get_roles,
        api::role_routes::insert_role,
        api::role_routes::delete_role,
        api::role_routes::get_user_role_ids,
        api::role_routes::assign_user_roles,
        api::role_routes::get_role_menu_ids,
        api::role_routes::assign_role_menus,
        api::role_routes::get_all_menus_tree,
        api::role_routes::get_user_menus,
        // 租户
        api::tenant_routes::get_tenants,
        api::tenant_routes::insert_tenant,
        api::tenant_routes::update_tenant,
        api::tenant_routes::delete_tenant,
        api::tenant_routes::switch_tenant,
        api::tenant_routes::assign_tenants,
        api::tenant_routes::get_user_tenants,
        // 知识库
        api::knowledge_routes::get_tree,
        api::knowledge_routes::insert_node,
        api::knowledge_routes::update_node,
        api::knowledge_routes::delete_node,
        api::knowledge_routes::move_node,
        api::knowledge_routes::get_list,
        api::knowledge_routes::get_by_id,
        api::knowledge_routes::insert_knowledge,
        api::knowledge_routes::update_knowledge,
        api::knowledge_routes::delete_knowledge,
        // 流程管理：领用 / 归还 / 调拨 / 维修 / 报废 / 采购
        api::process_routes::get_receives,
        api::process_routes::insert_receive,
        api::process_routes::update_receive,
        api::process_routes::delete_receive,
        api::process_routes::get_returns,
        api::process_routes::insert_return,
        api::process_routes::update_return,
        api::process_routes::delete_return,
        api::process_routes::get_transfers,
        api::process_routes::insert_transfer,
        api::process_routes::update_transfer,
        api::process_routes::delete_transfer,
        api::process_routes::get_repairs,
        api::process_routes::insert_repair,
        api::process_routes::update_repair,
        api::process_routes::delete_repair,
        api::process_routes::get_scraps,
        api::process_routes::insert_scrap,
        api::process_routes::update_scrap,
        api::process_routes::delete_scrap,
        api::process_routes::get_purchases,
        api::process_routes::insert_purchase,
        api::process_routes::update_purchase,
        api::process_routes::delete_purchase,
        // 运维采集
        api::ops_routes::providers,
        api::ops_routes::list_connections,
        api::ops_routes::get_connection,
        api::ops_routes::create_connection,
        api::ops_routes::update_connection,
        api::ops_routes::delete_connection,
        api::ops_routes::test_draft,
        api::ops_routes::test_connection,
        api::ops_routes::collect_now,
        api::ops_routes::list_links,
        api::ops_routes::bind_link,
        // 大文件上传
        api::upload_routes::init_upload,
        api::upload_routes::start_upload,
        api::upload_routes::commit_upload,
        api::upload_routes::report_chunk,
        api::upload_routes::get_progress,
        api::upload_routes::complete_upload,
        api::upload_routes::abort_upload,
        // 智能问答 / 技能
        api::chat_routes::chat_stream,
        api::skill_routes::list_skills_handler,
        api::skill_routes::get_skill_handler,
        api::skill_routes::execute_skill_handler,
        api::skill_routes::register_skill_handler,
        api::skill_routes::unregister_skill_handler,
        api::skill_routes::get_skill_count_handler,
        // API Key 管理（JWT 管理员专用）
        api::api_key_routes::list_api_keys,
        api::api_key_routes::create_api_key,
        api::api_key_routes::rotate_api_key,
        api::api_key_routes::revoke_api_key,
    ),
    modifiers(&BearerAuthAddon)
)]
pub struct ApiDoc;
/// 注册 JWT 安全方案（`bearer_auth`）
///
/// 采用 [`Modify`] 而不是 `components(security_schemes(...))` 属性，避免属性宏里写
/// 复杂表达式的语法版本差异。
struct BearerAuthAddon;

impl Modify for BearerAuthAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let scheme = SecurityScheme::Http(
            HttpBuilder::new()
                .scheme(HttpAuthScheme::Bearer)
                .bearer_format("JWT")
                .description(Some(
                    "在 /api/auth/login 获取 token 后，以 `Bearer <token>` 形式放入 Authorization 请求头",
                ))
                .build(),
        );

        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme("bearer_auth", scheme);
        }
    }
}
