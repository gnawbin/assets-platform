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
    user_id: number;
    tenant_id: number;
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

/** 根据 ID 获取 Skill 详情 */
export function getSkill(skillId: string) {
    return api.get<SkillMeta>('get_skill', { skillId });
}

/** 执行 Skill */
export function executeSkill(params: {
    skill_id: string;
    input_text: string;
    config?: Record<string, unknown>;
    user_id: number;
    tenant_id: number;
}) {
    return api.post<SkillResult>('execute_skill', {
        skillId: params.skill_id,
        inputText: params.input_text,
        config: params.config ?? {},
        userId: params.user_id,
        tenantId: params.tenant_id,
    });
}

/** 注册自定义 Skill */
export function registerCustomSkill(skillMeta: SkillMeta) {
    return api.post<null>('register_custom_skill', { skillMeta });
}

/** 移除 Skill */
export function unregisterSkill(skillId: string) {
    return api.delete<boolean>('unregister_skill', { skillId });
}

/** 获取 Skill 数量 */
export function getSkillCount() {
    return api.get<number>('get_skill_count');
}