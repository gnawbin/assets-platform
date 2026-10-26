/**
 * BPMN 工作流编辑器的前端类型定义。
 *
 * 设计原则：
 * - 画布只承载标准 BPMN 2.0 元素；
 * - 执行器需要的业务参数放在 `assets:EngineConfig` 扩展里（BPMN 标准扩展点），
 *   不新增私有节点类型；
 * - IR（WorkflowDefinition.nodes/edges）仅作为后端执行契约，由映射层生成。
 */

import type { WorkflowDefinition } from '@/services/workflowService';

/** 可执行语义分类（同时用于 IR 投影与属性面板） */
export type ElementKind =
    | 'trigger'
    | 'skill'
    | 'llm'
    | 'condition'
    | 'code'
    | 'workflow'
    | 'output'
    | 'other';

/** `assets:EngineConfig` 中的执行参数 */
export interface EngineFields {
    kind?: string;
    skillId?: string;
    skillName?: string;
    triggerType?: string;
    accept?: string;
    maxSizeMb?: number;
    model?: string;
    temperature?: number;
    maxTokens?: number;
    field?: string;
    operator?: string;
    value?: string;
    yesLabel?: string;
    noLabel?: string;
    wfType?: string;
    prompt?: string;
    outputSchema?: string;
    params?: string;
}

/** 连线信息（用于条件分支展示 / 投影） */
export interface ElementFlowInfo {
    id: string;
    name: string;
    condition?: string;
}

/** 画布选中元素在属性面板中的视图模型 */
export interface SelectedBpmnElement {
    id: string;
    /** BPMN 元素类型，如 bpmn:ServiceTask */
    type: string;
    kind: ElementKind;
    name: string;
    documentation: string;
    /** 是否属于当前执行器支持的可执行子集 */
    executable: boolean;
    implementation?: string;
    calledElement?: string;
    scriptFormat?: string;
    script?: string;
    /** none / timer / message / signal / error ... */
    eventDefinition?: string;
    incoming: string[];
    outgoing: ElementFlowInfo[];
    engine: EngineFields;
}

/** 条件分支参数（同时写入 assets 扩展与标准 conditionExpression） */
export interface ConditionFields {
    field: string;
    operator: string;
    value: string;
    yesLabel: string;
    noLabel: string;
}

/** 属性面板对选中元素的修改指令 */
export type ElementPatch =
    | { type: 'name'; value: string }
    | { type: 'documentation'; value: string }
    | { type: 'replace'; value: string }
    | { type: 'implementation'; value: string }
    | { type: 'calledElement'; value: string }
    | { type: 'script'; scriptFormat: string; script: string }
    | { type: 'triggerEvent'; value: 'none' | 'timer' | 'message' }
    | { type: 'engine'; value: Partial<EngineFields> }
    | { type: 'condition'; value: ConditionFields };

/** 校验问题级别：error 阻断执行，warning 仅提示 */
export interface ValidationIssue {
    level: 'error' | 'warning';
    elementId?: string;
    message: string;
}

/** BPMN → IR 投影结果 */
export interface BpmnProjection {
    definition: WorkflowDefinition;
    issues: ValidationIssue[];
}

/** 画布暴露给外层的命令式 API */
export interface BpmnCanvasApi {
    saveXml(): Promise<string>;
    importXml(xml: string): Promise<void>;
    buildDefinition(meta: {
        name: string;
        description?: string;
        version?: string;
    }): Promise<BpmnProjection>;
    updateProcessMeta(meta: { name?: string; documentation?: string }): void;
    getProcessMeta(): { id: string; name: string; documentation: string };
    undo(): void;
    redo(): void;
    canUndo(): boolean;
    canRedo(): boolean;
    zoomIn(): void;
    zoomOut(): void;
    fitView(): void;
    insertPreset(presetId: string, extra?: { engine?: Partial<EngineFields>; name?: string }): void;
    insertPresetAt(
        presetId: string,
        position: { x: number; y: number },
        extra?: { engine?: Partial<EngineFields>; name?: string }
    ): void;
    updateSelected(patch: ElementPatch): void;
    getSelected(): SelectedBpmnElement | null;
    selectElement(id: string): void;
    applyExecutionState(results: { node_id: string; status: string }[]): void;
    clearExecutionState(): void;
    getModeler(): unknown;
}

/** 左侧面板拖拽时写入 dataTransfer 的负载 */
export interface AssetsDragPayload {
    presetId: string;
    name?: string;
    engine?: Partial<EngineFields>;
}
