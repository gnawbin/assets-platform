/**
 * 单据编号规则 API 服务
 *
 * 封装所有与编号规则相关的 Tauri 命令调用。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

export interface NumberingRule {
    id: string;
    biz_type: string;
    biz_name: string;
    prefix: string | null;
    date_format: string | null;
    date_position: string | null;
    serial_length: number;
    separator: string | null;
    reset_mode: string | null;
    sample_output: string | null;
    is_active: boolean;
}

export interface NumberingRuleInput {
    biz_type: string;
    biz_name: string;
    prefix: string | null;
    date_format: string | null;
    date_position: string | null;
    serial_length: number;
    separator: string | null;
    reset_mode: string | null;
    is_active: boolean;
}

// ======================== 服务方法 ========================

/** 获取所有编号规则 */
export function getRules() {
    return api.get<NumberingRule[]>('get_numbering_rules');
}

/** 根据业务类型获取单条规则 */
export function getRule(bizType: string) {
    return api.get<NumberingRule>('get_numbering_rule', { bizType });
}

/** 保存编号规则（新增或更新） */
export function saveRule(params: {
    id?: string;
    input: NumberingRuleInput;
}) {
    return api.post<NumberingRule>('save_numbering_rule', params);
}

/** 重置流水号 */
export function resetSequence(params: {
    bizType: string;
    resetKey: string;
}) {
    return api.post<void>('reset_numbering_sequence', params);
}