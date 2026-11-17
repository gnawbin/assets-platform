/**
 * Zen Engine Skill API 服务
 *
 * 封装所有与 Skill 管理相关的 Tauri 命令调用。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

/** Skill 类型 */
export type SkillType = 'Builtin' | 'Custom';

/** Skill 元数据 */
export interface SkillMeta {
    id: string;
    name: string;
    description: string;
    icon: string;
    version: string;
    author: string;
    skill_type: SkillType;
    config_schema: Record<string, unknown>;
    file_path: string;
}

/** Skill 执行上下文 */
export interface SkillContext {
    input_text: string;
    config: Record<string, unknown>;
    user_id: string;
    tenant_id: string;
    document_id?: string;
    cursor_position?: number;
}

/** Skill 执行结果 */
export interface SkillResult {
    output: string;
    output_type: string;
    position: string;
    metadata?: Record<string, string>;
}

// ======================== API 方法 ========================

/** 获取所有 Skill 列表 */
export function listSkills() {
    return api.get<SkillMeta[]>('list_skills');
}

/** 根据 ID 获取 Skill 详情（路径占位符为 {skillId}） */
export function getSkill(skillId: string) {
    return api.get<SkillMeta>('get_skill', { skillId });
}

/**
 * 执行 Skill
 *
 * 后端 `ExecuteSkillRequest` 为扁平 snake_case；`user_id` / `tenant_id`
 * 是雪花 ID，按字符串传输（后端 i64_from_string 兼容）。
 */
export function executeSkill(params: {
    skill_id: string;
    input_text: string;
    config?: Record<string, unknown>;
    user_id: string;
    tenant_id: string;
}) {
    return api.post<SkillResult>('execute_skill', {
        skill_id: params.skill_id,
        input_text: params.input_text,
        config: params.config ?? {},
        user_id: params.user_id,
        tenant_id: params.tenant_id,
    });
}

/** 注册自定义 Skill（后端直接反序列化 SkillMeta，不再包一层 skillMeta） */
export function registerCustomSkill(skillMeta: SkillMeta) {
    return api.post<null>('register_custom_skill', { ...skillMeta });
}

/** 移除 Skill（路径占位符为 {skillId}） */
export function unregisterSkill(skillId: string) {
    return api.delete<boolean>('unregister_skill', { skillId });
}

/** 获取 Skill 数量 */
export function getSkillCount() {
    return api.get<number>('get_skill_count');
}