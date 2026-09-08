/**
 * httpAdapter 映射表完整性测试
 *
 * 验证 COMMAND_ROUTE_MAP 与 TAURI_ONLY_COMMANDS 覆盖了后端
 * apps/backend/src-tauri/src/lib.rs invoke_handler 中注册的全部 Tauri 命令，
 * 且映射表自身格式合法（路径以 /api/ 开头、方法合法、占位符格式正确）。
 *
 * 当后端新增/移除 Tauri 命令时，需同步更新本文件的 REGISTERED_COMMANDS 列表。
 */

import {
    COMMAND_ROUTE_MAP,
    TAURI_ONLY_COMMANDS,
} from '@/utils/adapters/httpAdapter';

/**
 * 后端 lib.rs invoke_handler 注册的 Tauri 命令全集（共 123 个）
 *
 * 与 apps/backend/src-tauri/src/lib.rs 的 invoke_handler 保持一致，
 * 按模块分组，方便对照。
 */
const REGISTERED_COMMANDS: string[] = [
    // 资产（固定资产 + 无形资产）
    'get_hardware_assets',
    'insert_hardware_asset',
    'update_hardware_asset',
    'delete_hardware_asset',
    'get_intangible_assets',
    'insert_intangible_asset',
    'update_intangible_asset',
    'delete_intangible_asset',

    // 资产分类
    'get_categories',
    'get_categories_parents',
    'insert_category',
    'update_category',
    'delete_category',

    // 部门
    'get_departments',
    'insert_department',
    'update_department',
    'delete_department',

    // 租户
    'get_tenants',
    'insert_tenant',
    'update_tenant',
    'delete_tenant',
    'switch_tenant',
    'assign_user_tenants',
    'get_user_tenants',

    // 用户
    'login',
    'get_users',
    'insert_user',
    'update_user',
    'delete_user',
    'reset_password',
    'get_current_user',

    // 角色权限
    'get_roles',
    'insert_role',
    'delete_role',
    'get_user_role_ids',
    'assign_user_roles',
    'get_role_menu_ids',
    'assign_role_menus',
    'get_all_menus_tree',
    'get_user_menus',

    // 注册申请
    'register',
    'get_registrations',
    'approve_registration',
    'reject_registration',

    // 知识库（知识树节点 + 知识条目）
    'get_knowledge_tree',
    'insert_knowledge_node',
    'update_knowledge_node',
    'delete_knowledge_node',
    'move_knowledge_node',
    'get_knowledge_list',
    'get_knowledge_by_id',
    'insert_knowledge',
    'update_knowledge',
    'delete_knowledge',

    // 知识资产（OKF 新表）
    'get_knowledge_asset_by_tree_node',
    'get_knowledge_asset',
    'list_knowledge_assets',
    'create_knowledge_asset',
    'update_knowledge_asset',
    'delete_knowledge_asset',
    'attach_file_to_knowledge',

    // 流程管理（领用/归还/调拨/维修/报废/采购）
    'get_receives',
    'insert_receive',
    'update_receive',
    'delete_receive',
    'get_returns',
    'insert_return',
    'update_return',
    'delete_return',
    'get_transfers',
    'insert_transfer',
    'update_transfer',
    'delete_transfer',
    'get_repairs',
    'insert_repair',
    'update_repair',
    'delete_repair',
    'get_scraps',
    'insert_scrap',
    'update_scrap',
    'delete_scrap',
    'get_purchases',
    'insert_purchase',
    'update_purchase',
    'delete_purchase',

    // LLM 厂商/模型
    'get_llm_providers',
    'get_llm_provider',
    'create_llm_provider',
    'update_llm_provider',
    'delete_llm_provider',
    'get_llm_models',
    'create_llm_model',
    'update_llm_model',
    'delete_llm_model',
    'fetch_llm_models',
    'get_user_llm_setting',
    'save_user_llm_setting',

    // Skill 管理
    'list_skills',
    'get_skill',
    'execute_skill',
    'register_custom_skill',
    'unregister_skill',
    'get_skill_count',

    // 编号规则
    'get_numbering_rules',
    'get_numbering_rule',
    'save_numbering_rule',
    'reset_numbering_sequence',

    // RAG
    'chunk_and_vectorize',
    'test_rag_retrieval',

    // 智能问答对话
    'create_conversation',
    'send_message',
    'get_conversations',
    'get_conversation_messages',
    'update_conversation_title',
    'delete_conversation',

    // 大文件上传
    'upload_init',
    'upload_start',
    'upload_report_chunk',
    'upload_complete',
    'upload_abort',
    'upload_get_progress',
    'upload_commit',
    'upload_get_version_history',
];

describe('httpAdapter 命令路由映射表', () => {
    it('覆盖后端注册的全部 Tauri 命令（映射表 或 Tauri-only 集合）', () => {
        const missing = REGISTERED_COMMANDS.filter(
            (command) =>
                !Object.prototype.hasOwnProperty.call(COMMAND_ROUTE_MAP, command) &&
                !TAURI_ONLY_COMMANDS.has(command),
        );

        expect(missing).toEqual([]);
    });

    it('命令不会同时出现在映射表和 Tauri-only 集合中', () => {
        const overlaps = Object.keys(COMMAND_ROUTE_MAP).filter((command) =>
            TAURI_ONLY_COMMANDS.has(command),
        );

        expect(overlaps).toEqual([]);
    });

    it('映射表路径以 /api/ 开头且 HTTP 方法合法', () => {
        const LEGAL_METHODS = ['GET', 'POST', 'PUT', 'DELETE'];

        for (const [command, route] of Object.entries(COMMAND_ROUTE_MAP)) {
            // 路径必须以 /api/ 开头
            expect(route.path).toMatch(/^\/api\//);
            // HTTP 方法必须合法
            expect(LEGAL_METHODS).toContain(route.method);
            // 占位符必须为 {word} 格式
            for (const placeholder of route.path.match(/\{(\w+)\}/g) ?? []) {
                expect(placeholder.slice(1, -1)).toMatch(/^[a-zA-Z][a-zA-Z0-9_]*$/);
            }
        }
    });

    it('映射表与 Tauri-only 集合中的命令名均为小写下划线风格', () => {
        const NAME_PATTERN = /^[a-z][a-z0-9_]*$/;

        for (const command of Object.keys(COMMAND_ROUTE_MAP)) {
            expect(command).toMatch(NAME_PATTERN);
        }

        for (const command of TAURI_ONLY_COMMANDS) {
            expect(command).toMatch(NAME_PATTERN);
        }
    });
});
