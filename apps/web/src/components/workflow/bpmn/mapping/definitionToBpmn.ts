/**
 * WorkflowDefinition（IR）→ 标准 BPMN 2.0 XML。
 *
 * 仅在浏览器端调用（需要 DOM 容器）。若定义里已保存 bpmn_xml，优先直接返回，
 * 避免无谓的转换与信息丢失。
 */

import type { WorkflowDefinition, WorkflowNode } from '@/services/workflowService';
import type { EngineFields } from '../types';
import { assetsModdle } from '../moddle/assetsModdle';
import { BLANK_BPMN_XML } from '../template/blankDiagram';
import {
    asNumber,
    asRecord,
    asString,
    formatConditionExpression,
    parseJsonObject,
    parseLiteral,
} from './helpers';
import { getPreset, type NodePreset } from './nodeKinds';
import { createBusinessObjectForPreset } from '../modeler/createElements';

const DEFAULT_X_GAP = 240;
const DEFAULT_Y_GAP = 120;

/** IR 节点 config → assets:EngineConfig 字段 */
export function engineFromNode(node: WorkflowNode): Partial<EngineFields> {
    const config = asRecord(node.config);

    switch (node.type) {
        case 'trigger':
            return {
                triggerType: asString(config.trigger_type) || 'manual',
                accept: asString(config.accept) || undefined,
                maxSizeMb: asNumber(config.max_size_mb),
            };
        case 'skill': {
            const { skill_id: skillId, skill_name: skillName, ...rest } = config;
            return {
                kind: 'skill',
                skillId: asString(skillId),
                skillName: asString(skillName) || undefined,
                params: Object.keys(rest).length > 0 ? JSON.stringify(rest) : undefined,
            };
        }
        case 'llm':
            return {
                kind: 'llm',
                prompt: asString(config.prompt),
                model: asString(config.model) || undefined,
                temperature: asNumber(config.temperature),
                maxTokens: asNumber(config.max_tokens),
                outputSchema: config.output_schema ? JSON.stringify(config.output_schema) : undefined,
            };
        case 'condition': {
            const parsed = parseJsonObject(config.value);
            const rawValue = config.value;
            return {
                kind: 'condition',
                field: asString(config.field),
                operator: asString(config.operator) || '>',
                value:
                    typeof rawValue === 'string'
                        ? rawValue
                        : rawValue === undefined || rawValue === null
                          ? ''
                          : Object.keys(parsed).length > 0
                            ? JSON.stringify(rawValue)
                            : String(rawValue),
                yesLabel: asString(config.yes_label) || 'yes',
                noLabel: asString(config.no_label) || 'no',
            };
        }
        case 'code':
            return { kind: 'code' };
        case 'workflow':
            return { kind: 'workflow', wfType: asString(config.wf_type) };
        case 'agent':
            return {
                kind: 'agent',
                prompt: asString(config.prompt),
                tools: asString(config.tools) || undefined,
                maxIterations: asNumber(config.max_iterations),
                interruptPolicy: asString(config.interrupt_policy) || undefined,
                model: asString(config.model) || undefined,
                temperature: asNumber(config.temperature),
                maxTokens: asNumber(config.max_tokens),
            };
        case 'approval':
            return {
                kind: 'approval',
                assignee: asString(config.assignee) || asString(config.approver) || undefined,
                approvalTitle: asString(config.title) || undefined,
                approvalDescription: asString(config.description) || undefined,
                approvalVariable: asString(config.variable) || undefined,
            };
        default:
            return {};
    }
}

const FALLBACK_PRESET: NodePreset = {
    id: 'task',
    label: 'Task',
    description: '',
    bpmnType: 'bpmn:Task',
    kind: 'other',
    icon: '▭',
    color: '#495057',
    paletteIcon: 'bpmn-icon-task',
};

function presetForNode(node: WorkflowNode): NodePreset {
    return getPreset(node.type) ?? { ...FALLBACK_PRESET, label: node.label || 'Task' };
}

function readPosition(
    node: WorkflowNode,
    index: number,
    useSavedLayout: boolean
): { x: number; y: number } {
    const position = node.position;
    if (
        useSavedLayout &&
        position &&
        Number.isFinite(position.x) &&
        Number.isFinite(position.y)
    ) {
        return { x: Math.round(position.x), y: Math.round(position.y) };
    }
    return {
        x: 180 + (index % 3) * DEFAULT_X_GAP,
        y: 120 + Math.floor(index / 3) * DEFAULT_Y_GAP,
    };
}

/** bpmn-js 的 createShape 以形状中心定位；IR 的 position 是左上角 */
function toShapeCenter(
    bpmnType: string,
    topLeft: { x: number; y: number }
): { x: number; y: number } {
    let width = 100;
    let height = 80;
    if (bpmnType === 'bpmn:StartEvent' || bpmnType === 'bpmn:EndEvent') {
        width = 36;
        height = 36;
    } else if (bpmnType.endsWith('Gateway')) {
        width = 50;
        height = 50;
    }
    return {
        x: Math.round(topLeft.x + width / 2),
        y: Math.round(topLeft.y + height / 2),
    };
}

export async function definitionToBpmn(definition: WorkflowDefinition): Promise<string> {
    if (definition.bpmn_xml && definition.bpmn_xml.trim().startsWith('<')) {
        return definition.bpmn_xml;
    }
    if (typeof document === 'undefined') {
        throw new Error('definitionToBpmn 只能在浏览器环境调用');
    }

    const { default: Modeler } = await import('bpmn-js/lib/Modeler');
    const container = document.createElement('div');
    container.style.position = 'absolute';
    container.style.left = '-10000px';
    container.style.top = '0';
    container.style.width = '1200px';
    container.style.height = '800px';
    document.body.appendChild(container);

    const modeler: any = new Modeler({
        container,
        moddleExtensions: { assets: assetsModdle },
    });

    try {
        await modeler.importXML(BLANK_BPMN_XML);

        const canvas = modeler.get('canvas');
        const modeling = modeler.get('modeling');
        const elementFactory = modeler.get('elementFactory');
        const bpmnFactory = modeler.get('bpmnFactory');
        const elementRegistry = modeler.get('elementRegistry');
        const root = canvas.getRootElement();
        const process = root.businessObject;
        const definitions = process.$parent;

        const processProps: Record<string, unknown> = { isExecutable: true };
        if (definition.name) processProps.name = definition.name;
        if (definition.description) {
            processProps.documentation = [
                bpmnFactory.create('bpmn:Documentation', { text: definition.description }),
            ];
        }
        modeling.updateProperties(root, processProps);

        const useSavedLayout = definition.nodes.some(
            (node) => node.position && (node.position.x !== 0 || node.position.y !== 0)
        );

        const created = new Map<string, any>();
        definition.nodes.forEach((node, index) => {
            const preset = presetForNode(node);
            const engine = engineFromNode(node);
            const config = asRecord(node.config);

            const businessObject = createBusinessObjectForPreset(preset, bpmnFactory, {
                id: node.id,
                name: node.label,
                engine,
                definitions,
                scriptFormat: asString(config.language) || 'python',
                script: asString(config.code),
                calledElement: asString(config.wf_type) || undefined,
            });

            const shape = elementFactory.createShape({
                type: preset.bpmnType,
                businessObject,
            });
            const position = readPosition(node, index, useSavedLayout);
            const element = modeling.createShape(
                shape,
                toShapeCenter(preset.bpmnType, position),
                root
            );
            created.set(node.id, element);
        });

        definition.edges.forEach((edge) => {
            const source = created.get(edge.source) ?? elementRegistry.get(edge.source);
            const target = created.get(edge.target) ?? elementRegistry.get(edge.target);
            if (!source || !target) return;

            const connection = modeling.connect(source, target);
            const updates: Record<string, unknown> = {};

            if (edge.label) updates.name = edge.label;

            if (source.businessObject?.$type === 'bpmn:ExclusiveGateway') {
                const sourceNode = definition.nodes.find((node) => node.id === edge.source);
                const config = asRecord(sourceNode?.config);
                const yesLabel = asString(config.yes_label) || 'yes';
                if (edge.label === yesLabel) {
                    const conditionValue =
                        typeof config.value === 'string'
                            ? parseLiteral(config.value)
                            : config.value;
                    const body = formatConditionExpression(
                        asString(config.field),
                        asString(config.operator) || '>',
                        conditionValue
                    );
                    if (body) {
                        updates.conditionExpression = bpmnFactory.create('bpmn:FormalExpression', {
                            body,
                        });
                    }
                }
            }

            if (Object.keys(updates).length > 0) {
                modeling.updateProperties(connection, updates);
            }
        });

        // 标准 BPMN 语义：no 分支设置为网关的 default flow
        definition.nodes
            .filter((node) => node.type === 'condition')
            .forEach((node) => {
                const gateway = created.get(node.id);
                if (!gateway) return;
                const config = asRecord(node.config);
                const noLabel = asString(config.no_label) || 'no';
                const noEdge = definition.edges.find(
                    (edge) => edge.source === node.id && edge.label === noLabel
                );
                if (!noEdge) return;
                const connection = elementRegistry
                    .getAll()
                    .find(
                        (element: any) =>
                            element.type === 'bpmn:SequenceFlow' &&
                            element.source?.id === node.id &&
                            element.target?.id === noEdge.target
                    );
                if (connection) {
                    modeling.updateProperties(gateway, { default: connection.businessObject });
                }
            });

        const { xml } = await modeler.saveXML({ format: true });
        if (!xml) throw new Error('bpmn-js 未能生成 XML');
        return xml;
    } finally {
        modeler.destroy();
        container.remove();
    }
}

