/**
 * 标准 BPMN 2.0 → WorkflowDefinition（IR）投影。
 *
 * 该模块通过 modeler 的公开服务读取已导入的 diagram，不直接操作 DOM。
 * 未识别的标准 BPMN 元素会保留在 XML 中，但不会进入执行 IR，并给出 warning。
 */

import type { WorkflowDefinition, WorkflowEdge, WorkflowNode } from '@/services/workflowService';
import type { BpmnProjection, EngineFields, ValidationIssue } from '../types';
import {
    asString,
    dedupeIssues,
    engineToIrNode,
    isExecutableKind,
    orderNodesForExecution,
    parseConditionExpression,
    readElementName,
    readEngineFields,
    validateDefinition,
} from './helpers';
import { classifyBpmnElement, isExecutableBpmnType } from './nodeKinds';

interface BuildMeta {
    name: string;
    description?: string;
    version?: string;
}

interface FlowInfo {
    id: string;
    source: string;
    target: string;
    name?: string;
    condition?: string;
    isDefault: boolean;
}

function bpmnTypeLabel(type: string | undefined): string {
    if (!type) return '未知元素';
    const name = type.replace(/^bpmn:/, '');
    return name.replace(/([a-z])([A-Z])/g, '$1 $2');
}

function readDocumentationText(businessObject: any): string {
    const documentation = businessObject?.documentation;
    if (!Array.isArray(documentation) || documentation.length === 0) return '';
    return asString(documentation[0]?.text);
}

function readEventDefinition(businessObject: any): string | undefined {
    const definitions = businessObject?.eventDefinitions;
    if (!Array.isArray(definitions) || definitions.length === 0) return undefined;
    const type = asString(definitions[0]?.$type);
    return type.replace(/^bpmn:/, '').replace(/EventDefinition$/, '').toLowerCase() || undefined;
}

/** 读取流程中的所有连线（含 source/target/default 信息） */
function collectFlows(flowElements: any[]): FlowInfo[] {
    return flowElements
        .filter((element) => element?.$type === 'bpmn:SequenceFlow')
        .map((flow) => ({
            id: asString(flow.id),
            source: asString(flow.sourceRef?.id),
            target: asString(flow.targetRef?.id),
            name: asString(flow.name) || undefined,
            condition: asString(flow.conditionExpression?.body) || undefined,
            isDefault: false,
        }));
}

function markDefaultFlows(flowElements: any[], flows: FlowInfo[]): void {
    flowElements
        .filter((element) => typeof element?.default?.id === 'string')
        .forEach((gateway) => {
            const defaultId = gateway.default.id;
            const flow = flows.find((item) => item.id === defaultId);
            if (flow) flow.isDefault = true;
        });
}

function normalizeConditionFlowLabels(
    sourceKind: string,
    sourceFlows: FlowInfo[]
): Map<string, string> {
    const labels = new Map<string, string>();
    if (sourceKind !== 'condition') {
        sourceFlows.forEach((flow) => {
            if (flow.name) labels.set(flow.id, flow.name);
        });
        return labels;
    }

    const nonDefault = sourceFlows.filter((flow) => !flow.isDefault);
    sourceFlows.forEach((flow, index) => {
        if (flow.name) {
            labels.set(flow.id, flow.name);
        } else if (flow.isDefault) {
            labels.set(flow.id, 'no');
        } else if (nonDefault.length > 1 && index > 0) {
            labels.set(flow.id, 'no');
        } else {
            labels.set(flow.id, 'yes');
        }
    });
    return labels;
}


/** 读取流程内的所有流节点（任务 / 事件 / 网关 / 子流程） */
function isFlowNode(element: any): boolean {
    const type = asString(element?.$type);
    if (!type || type === 'bpmn:SequenceFlow') return false;
    return (
        type.endsWith('Task') ||
        type.endsWith('Event') ||
        type.endsWith('Gateway') ||
        type === 'bpmn:SubProcess' ||
        type === 'bpmn:Transaction'
    );
}

export async function bpmnToDefinition(modeler: any, meta: BuildMeta): Promise<BpmnProjection> {
    const canvas = modeler.get('canvas');
    const elementRegistry = modeler.get('elementRegistry');
    const root = canvas.getRootElement();
    const definitions = root?.businessObject?.$parent;
    const issues: ValidationIssue[] = [];

    const definition: WorkflowDefinition = {
        name: asString(meta.name).trim() || '未命名工作流',
        description: meta.description ?? '',
        version: meta.version || '1.0.0',
        nodes: [],
        edges: [],
    };

    const rootElements: any[] = Array.isArray(definitions?.rootElements) ? definitions.rootElements : [];
    const processes = rootElements.filter((element) => element?.$type === 'bpmn:Process');

    if (rootElements.some((element) => element?.$type === 'bpmn:Collaboration')) {
        issues.push({
            level: 'warning',
            message: '文档包含协作图（Pool / Lane），执行投影只使用第一个可执行 Process。',
        });
    }
    if (processes.length === 0) {
        issues.push({ level: 'error', message: 'BPMN 文档中没有 bpmn:Process，无法生成可执行工作流。' });
        return { definition, issues: dedupeIssues(issues) };
    }
    if (processes.length > 1) {
        issues.push({
            level: 'warning',
            message: `文档包含 ${processes.length} 个 Process，执行投影只使用第一个可执行 Process。`,
        });
    }

    const process = processes.find((item) => item.isExecutable === true) ?? processes[0];
    if (!process.isExecutable) {
        issues.push({ level: 'warning', message: 'Process 未标记 isExecutable="true"，执行前建议开启。' });
    }

    const processName = asString(process.name);
    if (!asString(meta.name).trim() && processName) {
        definition.name = processName;
    }
    if (meta.description === undefined) {
        definition.description = readDocumentationText(process);
    }

    const flowElements: any[] = Array.isArray(process.flowElements) ? process.flowElements : [];
    const flows = collectFlows(flowElements);
    markDefaultFlows(flowElements, flows);

    const flowsBySource = new Map<string, FlowInfo[]>();
    flows.forEach((flow) => {
        if (!flow.source) return;
        const list = flowsBySource.get(flow.source) || [];
        list.push(flow);
        flowsBySource.set(flow.source, list);
    });

    const nodes: WorkflowNode[] = [];
    const skipped = new Map<string, string>();
    let startCount = 0;
    let endCount = 0;

    flowElements.filter(isFlowNode).forEach((businessObject, index) => {
        const id = asString(businessObject.id);
        const type = asString(businessObject.$type);
        if (!id) return;

        const engine = readEngineFields(businessObject);
        const kind = classifyBpmnElement(type, asString(businessObject.implementation), engine.kind);
        const name = readElementName(businessObject);

        if (type === 'bpmn:EndEvent') {
            endCount += 1;
            skipped.set(id, type);
            return;
        }

        if (!isExecutableBpmnType(type) || !isExecutableKind(kind) || kind === 'other') {
            skipped.set(id, type);
            issues.push({
                level: 'warning',
                elementId: id,
                message: `元素「${name}」为 ${bpmnTypeLabel(type)}，当前执行器不支持，已保留在 BPMN 中但不会执行。`,
            });
            return;
        }

        const eventDefinition = type === 'bpmn:StartEvent' ? readEventDefinition(businessObject) : undefined;
        if (type === 'bpmn:StartEvent') startCount += 1;

        if (kind === 'condition') {
            const sourceFlows = flowsBySource.get(id) || [];
            const parsed = parseConditionExpression(sourceFlows.find((flow) => flow.condition)?.condition);
            if (parsed) {
                engine.field = engine.field || parsed.field;
                engine.operator = engine.operator || parsed.operator;
                engine.value = engine.value || parsed.value;
            }
            const labels = normalizeConditionFlowLabels(kind, sourceFlows);
            const yesFlow = sourceFlows.find((flow) => labels.get(flow.id) === 'yes');
            const noFlow = sourceFlows.find((flow) => labels.get(flow.id) === 'no');
            engine.yesLabel = engine.yesLabel || (yesFlow ? labels.get(yesFlow.id) : undefined) || 'yes';
            engine.noLabel = engine.noLabel || (noFlow ? labels.get(noFlow.id) : undefined) || 'no';
        }

        // 标准 BPMN 不含 assets 扩展时，calledElement 即子流程类型
        if (kind === 'workflow') {
            engine.wfType = engine.wfType || asString(businessObject.calledElement) || undefined;
        }

        const shape = elementRegistry.get(id);
        const position = shape
            ? { x: Math.round(shape.x), y: Math.round(shape.y) }
            : { x: 180, y: 120 + index * 110 };

        const node = engineToIrNode({
            id,
            kind,
            label: name,
            position,
            engine,
            script: asString(businessObject.script),
            scriptFormat: asString(businessObject.scriptFormat),
            eventDefinition,
        });
        nodes.push(node);

        if (type === 'bpmn:ServiceTask' && !asString(businessObject.implementation)) {
            issues.push({
                level: 'warning',
                elementId: id,
                message: `服务任务「${name}」没有 implementation，默认按 Skill 任务处理（建议填写 ##Skill 或 ##LLM）。`,
            });
        }
    });



    if (startCount === 0) {
        issues.push({ level: 'error', message: '流程缺少开始事件（bpmn:StartEvent），执行器无法确定入口。' });
    } else if (startCount > 1) {
        issues.push({ level: 'warning', message: `流程包含 ${startCount} 个开始事件，执行器只使用第一个。` });
    }
    if (endCount === 0) {
        issues.push({ level: 'warning', message: '流程没有结束事件（bpmn:EndEvent），建议补充。' });
    }

    const nodeIds = new Set(nodes.map((node) => node.id));
    const nodeTypeById = new Map(nodes.map((node) => [node.id, node.type]));
    const terminalByEndEvent = new Set<string>();
    const edges: WorkflowEdge[] = [];

    flows.forEach((flow) => {
        if (!flow.source || !flow.target) {
            issues.push({
                level: 'warning',
                elementId: flow.id,
                message: `连线「${flow.name || flow.id}」缺少 source 或 target，已忽略。`,
            });
            return;
        }
        if (!nodeIds.has(flow.source)) return;

        if (!nodeIds.has(flow.target)) {
            if (skipped.get(flow.target) === 'bpmn:EndEvent') {
                terminalByEndEvent.add(flow.source);
            } else {
                issues.push({
                    level: 'warning',
                    elementId: flow.id,
                    message: `连线「${flow.name || flow.id}」指向不支持的节点，已从执行 IR 中移除。`,
                });
            }
            return;
        }

        const labels = normalizeConditionFlowLabels(
            nodeTypeById.get(flow.source) || 'other',
            flowsBySource.get(flow.source) || []
        );
        const label = labels.get(flow.id);
        edges.push({
            id: flow.id,
            source: flow.source,
            target: flow.target,
            ...(label ? { label } : {}),
        });
    });

    definition.nodes = orderNodesForExecution(nodes, edges);
    definition.edges = edges;

    nodes
        .filter((node) => node.type === 'trigger')
        .forEach((node) => {
            const outgoing = edges.filter((edge) => edge.source === node.id);
            if (outgoing.length > 1) {
                issues.push({
                    level: 'warning',
                    elementId: node.id,
                    message: `开始事件「${node.label}」有多条出线，执行器只会沿第一条执行。`,
                });
            }
        });

    const irIssues = validateDefinition(definition, {
        terminalNodeIds: Array.from(terminalByEndEvent),
    });

    return { definition, issues: dedupeIssues([...issues, ...irIssues]) };
}
