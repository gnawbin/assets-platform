/**
 * 业务模板（基于标准 BPMN 元素，不是私有节点类型）。
 *
 * 左侧面板的 Skill / LLM / 条件 等条目只是“预填标准元素属性的快捷方式”，
 * 画布中看到的始终是 bpmn-js 默认渲染的标准 BPMN 图形。
 */

import type { EngineFields, ElementKind } from '../types';

export interface NodePreset {
    id: string;
    label: string;
    description: string;
    bpmnType: string;
    kind: ElementKind;
    icon: string;
    color: string;
    /** bpmn-font 图标类名（复用标准图标，避免额外资源） */
    paletteIcon: string;
}

export const NODE_PRESETS: NodePreset[] = [
    {
        id: 'trigger',
        label: '开始事件',
        description: '工作流入口（手动 / 文件 / 定时 / Webhook）',
        bpmnType: 'bpmn:StartEvent',
        kind: 'trigger',
        icon: '🚀',
        color: '#2b8a3e',
        paletteIcon: 'bpmn-icon-start-event-none',
    },
    {
        id: 'skill',
        label: 'Skill 任务',
        description: '标准 ServiceTask，implementation="##Skill"',
        bpmnType: 'bpmn:ServiceTask',
        kind: 'skill',
        icon: '🧩',
        color: '#1971c2',
        paletteIcon: 'bpmn-icon-service-task',
    },
    {
        id: 'llm',
        label: 'LLM 任务',
        description: '标准 ServiceTask，implementation="##LLM"',
        bpmnType: 'bpmn:ServiceTask',
        kind: 'llm',
        icon: '🧠',
        color: '#9c36b5',
        paletteIcon: 'bpmn-icon-business-rule-task',
    },
    {
        id: 'condition',
        label: '条件分支',
        description: '标准 ExclusiveGateway + conditionExpression',
        bpmnType: 'bpmn:ExclusiveGateway',
        kind: 'condition',
        icon: '🔀',
        color: '#e8590c',
        paletteIcon: 'bpmn-icon-gateway-xor',
    },
    {
        id: 'code',
        label: '脚本任务',
        description: '标准 ScriptTask（python / javascript）',
        bpmnType: 'bpmn:ScriptTask',
        kind: 'code',
        icon: '💻',
        color: '#495057',
        paletteIcon: 'bpmn-icon-script-task',
    },
    {
        id: 'workflow',
        label: '子工作流',
        description: '标准 CallActivity（calledElement）',
        bpmnType: 'bpmn:CallActivity',
        kind: 'workflow',
        icon: '🗂️',
        color: '#0c8599',
        paletteIcon: 'bpmn-icon-call-activity',
    },
    {
        id: 'output',
        label: '结束事件',
        description: '工作流终点事件',
        bpmnType: 'bpmn:EndEvent',
        kind: 'output',
        icon: '🏁',
        color: '#c92a2a',
        paletteIcon: 'bpmn-icon-end-event-none',
    },
];

export function getPreset(presetId: string): NodePreset | undefined {
    return NODE_PRESETS.find((preset) => preset.id === presetId);
}

/** 当前执行器支持投影为 IR 的标准 BPMN 元素 */
export const EXECUTABLE_BPMN_TYPES = [
    'bpmn:StartEvent',
    'bpmn:ServiceTask',
    'bpmn:ScriptTask',
    'bpmn:ExclusiveGateway',
    'bpmn:CallActivity',
    'bpmn:EndEvent',
] as const;

export function isExecutableBpmnType(type: string | undefined): boolean {
    return !!type && (EXECUTABLE_BPMN_TYPES as readonly string[]).includes(type);
}

/** 可切换的标准任务类型（属性面板） */
export const REPLACEABLE_ACTIVITY_TYPES = [
    { value: 'bpmn:Task', label: 'Task（抽象任务）' },
    { value: 'bpmn:UserTask', label: 'User Task（人工任务）' },
    { value: 'bpmn:ServiceTask', label: 'Service Task（服务任务）' },
    { value: 'bpmn:ScriptTask', label: 'Script Task（脚本任务）' },
    { value: 'bpmn:ManualTask', label: 'Manual Task（手动任务）' },
    { value: 'bpmn:BusinessRuleTask', label: 'Business Rule Task（规则任务）' },
    { value: 'bpmn:SendTask', label: 'Send Task（发送任务）' },
    { value: 'bpmn:ReceiveTask', label: 'Receive Task（接收任务）' },
    { value: 'bpmn:CallActivity', label: 'Call Activity（调用活动）' },
];

/** 可切换的标准网关类型 */
export const REPLACEABLE_GATEWAY_TYPES = [
    { value: 'bpmn:ExclusiveGateway', label: 'Exclusive Gateway（排他）' },
    { value: 'bpmn:ParallelGateway', label: 'Parallel Gateway（并行）' },
    { value: 'bpmn:InclusiveGateway', label: 'Inclusive Gateway（包容）' },
    { value: 'bpmn:EventBasedGateway', label: 'Event-Based Gateway（事件）' },
];

export function isActivityType(type: string | undefined): boolean {
    if (!type) return false;
    return (
        type === 'bpmn:Task' ||
        type === 'bpmn:UserTask' ||
        type === 'bpmn:ServiceTask' ||
        type === 'bpmn:ScriptTask' ||
        type === 'bpmn:ManualTask' ||
        type === 'bpmn:BusinessRuleTask' ||
        type === 'bpmn:SendTask' ||
        type === 'bpmn:ReceiveTask' ||
        type === 'bpmn:CallActivity' ||
        type === 'bpmn:SubProcess'
    );
}

export function isGatewayType(type: string | undefined): boolean {
    return !!type && type.endsWith('Gateway');
}

/** 由标准元素属性推导执行语义（元素类型优先，ServiceTask 再参考 assets 扩展） */
export function classifyBpmnElement(
    type: string,
    implementation: string | undefined,
    engineKind: string | undefined
): ElementKind {
    switch (type) {
        case 'bpmn:StartEvent':
            return 'trigger';
        case 'bpmn:EndEvent':
            return 'output';
        case 'bpmn:ScriptTask':
            return 'code';
        case 'bpmn:ExclusiveGateway':
            return 'condition';
        case 'bpmn:CallActivity':
            return 'workflow';
        case 'bpmn:ServiceTask':
            if (engineKind === 'skill' || engineKind === 'llm') return engineKind;
            if (implementation?.toUpperCase().includes('LLM')) return 'llm';
            return 'skill';
        default:
            return 'other';
    }
}

export const KIND_LABELS: Record<ElementKind, string> = {
    trigger: '开始事件',
    skill: 'Skill 任务',
    llm: 'LLM 任务',
    condition: '条件分支',
    code: '脚本任务',
    workflow: '子工作流',
    output: '结束事件',
    other: '其他 BPMN 元素',
};

/** 业务模板插入时的默认执行参数 */
export function defaultEngineForPreset(preset: NodePreset): Partial<EngineFields> {
    switch (preset.kind) {
        case 'trigger':
            return { triggerType: 'manual' };
        case 'condition':
            return { operator: '>', yesLabel: 'yes', noLabel: 'no' };
        default:
            return {};
    }
}
