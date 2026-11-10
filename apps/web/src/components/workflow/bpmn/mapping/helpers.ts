/**
 * BPMN 映射层的纯函数工具。
 *
 * 注意：本文件不得导入 bpmn-js / bpmn-moddle，保证可以在 Jest(jsdom, CJS)
 * 环境下直接测试（bpmn-moddle@10 为 ESM-only）。
 */

import type { WorkflowDefinition, WorkflowNode, WorkflowNodeType } from '@/services/workflowService';
import type { EngineFields, ElementKind, ValidationIssue } from '../types';

// ======================== 常量 ========================

/** 执行器支持的 IR 节点类型 */
export const IR_NODE_TYPES: WorkflowNodeType[] = [
    'trigger',
    'skill',
    'llm',
    'condition',
    'code',
    'workflow',
    'output',
];

/** 后端仍为占位实现的 IR 节点类型 */
const BACKEND_PLACEHOLDER_TYPES: WorkflowNodeType[] = ['skill', 'workflow'];

export const CONDITION_OPERATORS = [
    '>',
    '<',
    '>=',
    '<=',
    '==',
    '!=',
    'contains',
    'is_empty',
] as const;

export type ConditionOperator = (typeof CONDITION_OPERATORS)[number];

// ======================== 安全取值 ========================

export function asRecord(value: unknown): Record<string, unknown> {
    return value && typeof value === 'object' && !Array.isArray(value)
        ? (value as Record<string, unknown>)
        : {};
}

export function asString(value: unknown, fallback = ''): string {
    return typeof value === 'string' ? value : value === undefined || value === null ? fallback : String(value);
}

export function asNumber(value: unknown): number | undefined {
    if (typeof value === 'number' && Number.isFinite(value)) return value;
    if (typeof value === 'string' && value.trim() !== '') {
        const parsed = Number(value);
        if (Number.isFinite(parsed)) return parsed;
    }
    return undefined;
}

export function parseJsonObject(value: unknown): Record<string, unknown> {
    if (typeof value === 'object' && value !== null && !Array.isArray(value)) {
        return value as Record<string, unknown>;
    }
    if (typeof value === 'string' && value.trim()) {
        try {
            const parsed: unknown = JSON.parse(value);
            return asRecord(parsed);
        } catch {
            return {};
        }
    }
    return {};
}

// ======================== 条件表达式 ========================

const OPERATOR_PATTERN = '>=|<=|==|!=|>|<';

/** 解析字面量：数字 / 布尔 / null / 引号字符串 / JSON / 普通字符串 */
export function parseLiteral(raw: string): unknown {
    const text = raw.trim();
    if (!text) return '';

    if (
        (text.startsWith("'") && text.endsWith("'")) ||
        (text.startsWith('"') && text.endsWith('"'))
    ) {
        return text.slice(1, -1);
    }
    if (text === 'true') return true;
    if (text === 'false') return false;
    if (text === 'null') return null;
    if (/^-?\d+(\.\d+)?$/.test(text)) return Number(text);

    if (
        (text.startsWith('{') && text.endsWith('}')) ||
        (text.startsWith('[') && text.endsWith(']'))
    ) {
        try {
            return JSON.parse(text);
        } catch {
            return text;
        }
    }
    return text;
}

/** 把任意值序列化成条件表达式里的字面量 */
export function formatLiteral(value: unknown): string {
    if (value === null) return 'null';
    if (value === undefined) return "''";
    if (typeof value === 'number' || typeof value === 'boolean') return String(value);
    if (typeof value === 'object') return JSON.stringify(value);
    return `'${String(value).replace(/'/g, "\\'")}'`;
}


export interface ParsedCondition {
    field: string;
    operator: ConditionOperator;
    value: string;
}

/**
 * 解析常见的 BPMN conditionExpression 写法：
 *  - ${amount > 100}
 *  - ${status == 'done'}
 *  - ${customer.contains('vip')}
 *  - ${contains(customer, 'vip')}
 *  - ${isEmpty(amount)}
 *  - amount >= 10
 *
 * 解析失败返回 null（调用方保留原始表达式）。
 */
export function parseConditionExpression(expression: string | undefined | null): ParsedCondition | null {
    if (!expression) return null;

    let body = expression.trim();
    const wrapped = body.match(/^\$\{(.*)\}$/s);
    if (wrapped) body = wrapped[1].trim();
    if (!body) return null;

    const isEmpty = body.match(/^isEmpty\(\s*([A-Za-z_$][\w$.[\]]*)\s*\)$/);
    if (isEmpty) {
        return { field: isEmpty[1], operator: 'is_empty', value: '' };
    }

    const containsFn = body.match(/^contains\(\s*([A-Za-z_$][\w$.[\]]*)\s*,\s*(.+)\s*\)$/);
    if (containsFn) {
        return {
            field: containsFn[1],
            operator: 'contains',
            value: asString(parseLiteral(containsFn[2])),
        };
    }

    const containsMethod = body.match(/^([A-Za-z_$][\w$.[\]]*)\.(contains|includes)\(\s*(.+)\s*\)$/);
    if (containsMethod) {
        return {
            field: containsMethod[1],
            operator: 'contains',
            value: asString(parseLiteral(containsMethod[3])),
        };
    }

    const comparison = body.match(
        new RegExp(`^([A-Za-z_$][\\w$.[\\]]*)\\s*(${OPERATOR_PATTERN})\\s*(.+)$`, 's')
    );
    if (comparison) {
        return {
            field: comparison[1],
            operator: comparison[2] as ConditionOperator,
            value: asString(parseLiteral(comparison[3])),
        };
    }

    return null;
}

/** 根据条件参数生成标准 BPMN conditionExpression（xsi:type="bpmn:tFormalExpression"） */
export function formatConditionExpression(field: string, operator: string, value: unknown): string {
    const target = field.trim();
    if (!target) return '';

    if (operator === 'is_empty') return `\${isEmpty(${target})}`;
    if (operator === 'contains') return `\${contains(${target}, ${formatLiteral(value)})}`;

    return `\${${target} ${operator} ${formatLiteral(value)}}`;
}

// ======================== IR 工具 ========================

/** 收集用于工作流列表展示的 node_types（skill:xxx / workflow:xxx） */
export function collectNodeTypes(definition: WorkflowDefinition): string[] {
    const types = definition.nodes.map((node) => {
        const config = asRecord(node.config);
        if (node.type === 'skill') {
            const skillId = asString(config.skill_id);
            return skillId ? `skill:${skillId}` : 'skill';
        }
        if (node.type === 'workflow') {
            const wfType = asString(config.wf_type);
            return wfType ? `workflow:${wfType}` : 'workflow';
        }
        return node.type;
    });
    return Array.from(new Set(types));
}

/** 判断 IR 节点是否属于“可执行子集” */
export function isExecutableKind(kind: ElementKind): boolean {
    return (
        kind === 'trigger' ||
        kind === 'agent' ||
        kind === 'approval' ||
        kind === 'skill' ||
        kind === 'llm' ||
        kind === 'condition' ||
        kind === 'code' ||
        kind === 'workflow' ||
        kind === 'output'
    );
}

/** 将引擎参数对象转换为 IR config 所需的键名（snake_case） */
export function engineToNodeConfig(kind: ElementKind, engine: EngineFields): Record<string, unknown> {
    switch (kind) {
        case 'trigger':
            return {
                trigger_type: engine.triggerType || 'manual',
                ...(engine.accept ? { accept: engine.accept } : {}),
                ...(engine.maxSizeMb !== undefined ? { max_size_mb: engine.maxSizeMb } : {}),
            };
        case 'skill':
            return {
                ...parseJsonObject(engine.params),
                skill_id: engine.skillId || '',
                ...(engine.skillName ? { skill_name: engine.skillName } : {}),
            };
        case 'llm':
            return {
                prompt: engine.prompt || '',
                ...(engine.model ? { model: engine.model } : {}),
                ...(engine.temperature !== undefined ? { temperature: engine.temperature } : {}),
                ...(engine.maxTokens !== undefined ? { max_tokens: engine.maxTokens } : {}),
                ...(parseJsonObject(engine.outputSchema)
                    ? { output_schema: parseJsonObject(engine.outputSchema) }
                    : {}),
            };
        case 'condition':
            return {
                field: engine.field || '',
                operator: engine.operator || '>',
                value: parseLiteral(engine.value ?? ''),
                ...(engine.yesLabel ? { yes_label: engine.yesLabel } : {}),
                ...(engine.noLabel ? { no_label: engine.noLabel } : {}),
            };
        case 'code':
            return { language: 'python', code: '' };
        case 'workflow':
            return { wf_type: engine.wfType || '' };
        case 'agent':
            return {
                kind: 'agent',
                prompt: engine.prompt || '',
                ...(engine.tools ? { tools: engine.tools } : {}),
                ...(engine.maxIterations !== undefined ? { max_iterations: engine.maxIterations } : {}),
                ...(engine.interruptPolicy ? { interrupt_policy: engine.interruptPolicy } : {}),
                ...(engine.model ? { model: engine.model } : {}),
                ...(engine.temperature !== undefined ? { temperature: engine.temperature } : {}),
                ...(engine.maxTokens !== undefined ? { max_tokens: engine.maxTokens } : {}),
            };
        case 'approval':
            return {
                kind: 'approval',
                assignee: engine.assignee || '',
                ...(engine.approvalTitle ? { title: engine.approvalTitle } : {}),
                ...(engine.approvalDescription ? { description: engine.approvalDescription } : {}),
                ...(engine.approvalVariable ? { variable: engine.approvalVariable } : {}),
            };
        default:
            return {};
    }
}

/** 拓扑排序：优先从 trigger 节点出发，保证 nodes[0] 是入口 */
export function orderNodesForExecution(nodes: WorkflowNode[], edges: WorkflowDefinition['edges']): WorkflowNode[] {
    const byId = new Map(nodes.map((node) => [node.id, node]));
    const outgoing = new Map<string, string[]>();
    const incomingCount = new Map<string, number>();

    nodes.forEach((node) => {
        outgoing.set(node.id, []);
        incomingCount.set(node.id, 0);
    });

    edges.forEach((edge) => {
        if (!byId.has(edge.source) || !byId.has(edge.target)) return;
        outgoing.get(edge.source)!.push(edge.target);
        incomingCount.set(edge.target, (incomingCount.get(edge.target) || 0) + 1);
    });

    const ordered: WorkflowNode[] = [];
    const visited = new Set<string>();

    const triggers = nodes.filter((node) => node.type === 'trigger');
    const queue: string[] =
        triggers.length > 0
            ? triggers.map((node) => node.id)
            : nodes.filter((node) => (incomingCount.get(node.id) || 0) === 0).map((node) => node.id);
    if (queue.length === 0 && nodes.length > 0) queue.push(nodes[0].id);

    while (queue.length > 0) {
        const id = queue.shift()!;
        if (!id || visited.has(id)) continue;
        visited.add(id);
        const node = byId.get(id);
        if (node) ordered.push(node);
        (outgoing.get(id) || []).forEach((target) => {
            if (!visited.has(target)) queue.push(target);
        });
    }

    // 处理环 / 孤立节点
    nodes.forEach((node) => {
        if (!visited.has(node.id)) ordered.push(node);
    });

    return ordered;
}

// ======================== 校验 ========================

/**
 * 对 IR 做执行前校验（与 BPMN 结构校验互补）。
 *
 * terminalNodeIds：通过“结束事件”终止的节点，避免误报“没有出线”。
 */
export function validateDefinition(
    definition: WorkflowDefinition,
    options: { terminalNodeIds?: string[] } = {}
): ValidationIssue[] {
    const issues: ValidationIssue[] = [];
    const terminalNodeIds = new Set(options.terminalNodeIds || []);

    if (!definition.name || !definition.name.trim()) {
        issues.push({ level: 'warning', message: '工作流名称为空，建议填写名称后再保存。' });
    }
    if (definition.nodes.length === 0) {
        issues.push({ level: 'error', message: '流程中没有任何可执行节点（至少需要一个开始事件）。' });
        return issues;
    }

    const nodeIds = new Set(definition.nodes.map((node) => node.id));
    const triggers = definition.nodes.filter((node) => node.type === 'trigger');

    if (triggers.length === 0) {
        issues.push({ level: 'error', message: '缺少开始事件（trigger），执行器无法确定入口节点。' });
    } else if (triggers.length > 1) {
        issues.push({ level: 'warning', message: `存在 ${triggers.length} 个开始事件，执行器只使用第一个。` });
    }

    definition.edges.forEach((edge) => {
        if (!nodeIds.has(edge.source)) {
            issues.push({ level: 'error', elementId: edge.source, message: `连线 ${edge.id} 的起点节点不存在。` });
        }
        if (!nodeIds.has(edge.target)) {
            issues.push({ level: 'error', elementId: edge.target, message: `连线 ${edge.id} 的终点节点不存在。` });
        }
    });

    definition.nodes.forEach((node) => {
        const config = asRecord(node.config);
        const outgoing = definition.edges.filter((edge) => edge.source === node.id);

        switch (node.type) {
            case 'trigger':
                break;
            case 'skill':
                if (!asString(config.skill_id)) {
                    issues.push({ level: 'error', elementId: node.id, message: `Skill 节点「${node.label}」未选择 Skill。` });
                }
                break;
            case 'llm':
                if (!asString(config.prompt)) {
                    issues.push({ level: 'error', elementId: node.id, message: `LLM 节点「${node.label}」未填写 Prompt。` });
                }
                break;
            case 'agent':
                if (!asString(config.prompt)) {
                    issues.push({
                        level: 'error',
                        elementId: node.id,
                        message: `AI Agent 节点「${node.label}」未填写 Prompt（系统指令）。`,
                    });
                }
                if (outgoing.length === 0) {
                    issues.push({
                        level: 'warning',
                        elementId: node.id,
                        message: `AI Agent 节点「${node.label}」没有出线，执行到此不会继续。`,
                    });
                }
                break;
            case 'approval':
                if (!asString(config.assignee)) {
                    issues.push({
                        level: 'warning',
                        elementId: node.id,
                        message: `人工审批「${node.label}」未设置审批人，任务将进入未分配队列。`,
                    });
                }
                if (outgoing.length === 0) {
                    issues.push({
                        level: 'warning',
                        elementId: node.id,
                        message: `人工审批「${node.label}」没有出线，审批通过后流程不会继续。`,
                    });
                }
                break;
            case 'code':
                if (!asString(config.code)) {
                    issues.push({ level: 'warning', elementId: node.id, message: `代码节点「${node.label}」代码为空。` });
                }
                break;
            case 'condition':
                if (!asString(config.field)) {
                    issues.push({ level: 'warning', elementId: node.id, message: `条件分支「${node.label}」未设置判断字段。` });
                }
                if (outgoing.length < 2) {
                    issues.push({
                        level: 'error',
                        elementId: node.id,
                        message: `条件分支「${node.label}」需要至少两条出线（yes / no）。`,
                    });
                }
                if (outgoing.length > 0 && !outgoing.some((edge) => edge.label === 'yes')) {
                    issues.push({
                        level: 'warning',
                        elementId: node.id,
                        message: `条件分支「${node.label}」的出线缺少 name="yes" 标签，执行器将无法进入“是”分支。`,
                    });
                }
                break;
            case 'workflow':
                if (!asString(config.wf_type)) {
                    issues.push({ level: 'error', elementId: node.id, message: `子工作流节点「${node.label}」未设置类型。` });
                }
                break;
            case 'output':
                break;
            default:
                issues.push({
                    level: 'error',
                    elementId: node.id,
                    message: `节点「${node.label}」类型 ${String(node.type)} 不受执行器支持。`,
                });
        }

        if (BACKEND_PLACEHOLDER_TYPES.includes(node.type)) {
            issues.push({
                level: 'warning',
                elementId: node.id,
                message: `节点「${node.label}」：后端 workflow_executor 目前为占位实现。`,
            });
        }

        if (
            node.type !== 'trigger' &&
            node.type !== 'output' &&
            outgoing.length === 0 &&
            !terminalNodeIds.has(node.id)
        ) {
            issues.push({
                level: 'warning',
                elementId: node.id,
                message: `节点「${node.label}」没有出线，执行到此处会提前结束。`,
            });
        }
    });

    return issues;
}

/** 去重（同一元素同一消息只保留一次） */
export function dedupeIssues(issues: ValidationIssue[]): ValidationIssue[] {
    const seen = new Set<string>();
    return issues.filter((issue) => {
        const key = `${issue.level}|${issue.elementId ?? ''}|${issue.message}`;
        if (seen.has(key)) return false;
        seen.add(key);
        return true;
    });
}

// ======================== BPMN 业务对象读取（仅做属性访问） ========================

function readModdleValue(source: any, key: string): unknown {
    if (!source) return undefined;
    const direct = source[key];
    if (direct !== undefined) return direct;
    if (typeof source.get === 'function') return source.get(key);
    return undefined;
}

/** 从任意 BPMN 业务对象中读取 assets:EngineConfig */
export function findEngineConfig(businessObject: any): any | undefined {
    const values = readModdleValue(businessObject, 'extensionElements');
    const list = readModdleValue(values, 'values');
    if (!Array.isArray(list)) return undefined;
    return list.find((item: any) => item?.$type === 'assets:EngineConfig');
}

/** 把 assets:EngineConfig 读成 EngineFields */
export function readEngineFields(businessObject: any): EngineFields {
    const config = findEngineConfig(businessObject);
    if (!config) return {};

    return {
        kind: asString(readModdleValue(config, 'kind')) || undefined,
        skillId: asString(readModdleValue(config, 'skillId')) || undefined,
        skillName: asString(readModdleValue(config, 'skillName')) || undefined,
        triggerType: asString(readModdleValue(config, 'triggerType')) || undefined,
        accept: asString(readModdleValue(config, 'accept')) || undefined,
        maxSizeMb: asNumber(readModdleValue(config, 'maxSizeMb')),
        model: asString(readModdleValue(config, 'model')) || undefined,
        temperature: asNumber(readModdleValue(config, 'temperature')),
        maxTokens: asNumber(readModdleValue(config, 'maxTokens')),
        field: asString(readModdleValue(config, 'field')) || undefined,
        operator: asString(readModdleValue(config, 'operator')) || undefined,
        value: asString(readModdleValue(config, 'value')),
        yesLabel: asString(readModdleValue(config, 'yesLabel')) || undefined,
        noLabel: asString(readModdleValue(config, 'noLabel')) || undefined,
        wfType: asString(readModdleValue(config, 'wfType')) || undefined,
        prompt: asString(readModdleValue(config, 'prompt')) || undefined,
        outputSchema: asString(readModdleValue(config, 'outputSchema')) || undefined,
        params: asString(readModdleValue(config, 'params')) || undefined,
        tools: asString(readModdleValue(config, 'tools')) || undefined,
        maxIterations: asNumber(readModdleValue(config, 'maxIterations')),
        interruptPolicy: asString(readModdleValue(config, 'interruptPolicy')) || undefined,
        assignee: asString(readModdleValue(config, 'assignee')) || undefined,
        approvalTitle: asString(readModdleValue(config, 'approvalTitle')) || undefined,
        approvalDescription: asString(readModdleValue(config, 'approvalDescription')) || undefined,
        approvalVariable: asString(readModdleValue(config, 'approvalVariable')) || undefined,
    };
}

/**
 * 读取 bpmn:UserTask 的审批人。
 *
 * 标准 BPMN 2.0 没有 assignee 属性，bpmn-moddle 会把未知属性放进 `$attrs`：
 * - `data-assignee`：本平台与 bpmninja bpmn-parser 的约定
 * - `camunda:assignee`：Camunda Modeler 导出的流程
 */
export function readUserTaskAssignee(businessObject: any): string {
    const attrs = readModdleValue(businessObject, '$attrs') as Record<string, unknown> | undefined;
    const fromAttrs = attrs
        ? asString(attrs['data-assignee']) ||
          asString(attrs['camunda:assignee']) ||
          asString(attrs['assignee'])
        : '';
    return fromAttrs || asString(readModdleValue(businessObject, 'assignee'));
}

/** 业务对象上的 BPMN documentation 文本 */
export function readDocumentation(businessObject: any): string {
    const documentation = readModdleValue(businessObject, 'documentation');
    if (!Array.isArray(documentation) || documentation.length === 0) return '';
    return asString(readModdleValue(documentation[0], 'text'));
}

/** 名称：BPMN name 优先，否则回退到元素 id */
export function readElementName(businessObject: any): string {
    return asString(readModdleValue(businessObject, 'name')) || asString(readModdleValue(businessObject, 'id'));
}




/**
 * 把 BPMN 元素的执行参数投影为 IR 节点。
 *
 * 仅在 kind 属于可执行子集时调用（other 会回退为 Skill，调用方应过滤）。
 */
export function engineToIrNode(params: {
    id: string;
    kind: ElementKind;
    label: string;
    position: { x: number; y: number };
    engine: EngineFields;
    script?: string;
    scriptFormat?: string;
    eventDefinition?: string;
}): WorkflowNode {
    const { id, kind, label, position, engine, script, scriptFormat, eventDefinition } = params;

    if (kind === 'trigger') {
        let triggerType = engine.triggerType;
        if (!triggerType || triggerType === 'manual') {
            if (eventDefinition === 'timer') triggerType = 'scheduled';
            else if (eventDefinition === 'message') triggerType = 'webhook';
            else triggerType = 'manual';
        }
        return {
            id,
            type: 'trigger',
            label,
            position,
            config: engineToNodeConfig('trigger', { ...engine, triggerType }),
        };
    }

    if (kind === 'code') {
        return {
            id,
            type: 'code',
            label,
            position,
            config: {
                language: scriptFormat === 'javascript' ? 'javascript' : 'python',
                code: script || '',
            },
        };
    }

    const nodeType: WorkflowNode['type'] =
        kind === 'output' ? 'output' : kind === 'other' ? 'skill' : kind;

    return {
        id,
        type: nodeType,
        label,
        position,
        config: engineToNodeConfig(kind, engine),
    };
}

