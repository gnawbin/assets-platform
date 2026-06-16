//! OpenAPI 文档定义
//!
//! 使用 utoipa 自动生成 OpenAPI 3.0 规范文档。

use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

use crate::api::category_routes::{CreateCategoryRequest, UpdateCategoryRequest};
use crate::api::department_routes::{CreateDepartmentRequest, UpdateDepartmentRequest};
use crate::api::role_routes::{
    AssignRoleMenusRequest, AssignUserRolesRequest, CreateRoleRequest, UpdateRoleRequest,
};
use crate::api::user_routes::{
    CreateUserRequest, LoginRequest, LoginResponse, ResetPasswordRequest, UpdateUserRequest,
};
use crate::database::models::{AssetCategory, Department, Role};
use crate::service::assets_service::{
    HardwareAssetInput, HardwareAssetView, IntangibleAssetInput, IntangibleAssetView,
};
use crate::service::process_service::{
    AssetPurchaseInput, AssetPurchaseUpdateInput, AssetReceiveInput, AssetReceiveUpdateInput,
    AssetRepairInput, AssetRepairUpdateInput, AssetReturnInput, AssetReturnUpdateInput,
    AssetScrapInput, AssetScrapUpdateInput, AssetTransferInput, AssetTransferUpdateInput,
};
use crate::service::user_service::UserResponse;

/// API 文档结构
#[derive(OpenApi)]
#[openapi(
    paths(
        // 认证
        crate::api::user_routes::login,
        // 资产分类
        crate::api::category_routes::get_categories,
        crate::api::category_routes::get_categories_parents,
        crate::api::category_routes::insert_category,
        crate::api::category_routes::update_category,
        crate::api::category_routes::delete_category,
        // 固定资产
        crate::api::asset_routes::get_hardware_assets,
        crate::api::asset_routes::insert_hardware_asset,
        crate::api::asset_routes::update_hardware_asset,
        crate::api::asset_routes::delete_hardware_asset,
        // 无形资产
        crate::api::asset_routes::get_intangible_assets,
        crate::api::asset_routes::insert_intangible_asset,
        crate::api::asset_routes::update_intangible_asset,
        crate::api::asset_routes::delete_intangible_asset,
        // 部门
        crate::api::department_routes::get_departments,
        crate::api::department_routes::insert_department,
        crate::api::department_routes::update_department,
        crate::api::department_routes::delete_department,
        // 用户
        crate::api::user_routes::get_users,
        crate::api::user_routes::insert_user,
        crate::api::user_routes::update_user,
        crate::api::user_routes::delete_user,
        crate::api::user_routes::reset_password,
        crate::api::user_routes::get_current_user,
        // 角色
        crate::api::role_routes::get_roles,
        crate::api::role_routes::insert_role,
        crate::api::role_routes::delete_role,
        crate::api::role_routes::get_user_role_ids,
        crate::api::role_routes::assign_user_roles,
        crate::api::role_routes::get_role_menu_ids,
        crate::api::role_routes::assign_role_menus,
        // 菜单
        crate::api::role_routes::get_all_menus_tree,
        crate::api::role_routes::get_user_menus,
    ),
    components(
        schemas(
            // 请求
            CreateCategoryRequest,
            UpdateCategoryRequest,
            CreateDepartmentRequest,
            UpdateDepartmentRequest,
            CreateUserRequest,
            UpdateUserRequest,
            LoginRequest,
            LoginResponse,
            CreateRoleRequest,
            UpdateRoleRequest,
            AssignUserRolesRequest,
            AssignRoleMenusRequest,
            ResetPasswordRequest,
            HardwareAssetInput,
            HardwareAssetView,
            IntangibleAssetInput,
            IntangibleAssetView,
            // 模型
            AssetCategory,
            Department,
            Role,
            UserResponse,
        )
    ),
    tags(
        (name = "认证", description = "用户认证相关接口"),
        (name = "资产分类", description = "资产分类管理接口"),
        (name = "固定资产", description = "固定资产管理接口"),
        (name = "无形资产", description = "无形资产管理接口"),
        (name = "部门管理", description = "部门管理接口"),
        (name = "用户管理", description = "用户管理接口"),
        (name = "角色管理", description = "角色管理接口"),
        (name = "菜单管理", description = "菜单管理接口"),
    ),
    modifiers(&SecurityAddon),
)]
pub struct ApiDoc;

/// 添加 Bearer 认证安全方案
struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.as_mut().unwrap();
        components.add_security_scheme(
            "bearer_auth",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
    }
}
