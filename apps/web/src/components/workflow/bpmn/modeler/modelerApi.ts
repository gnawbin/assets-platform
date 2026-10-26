/**
 * bpmn-js Modeler 的命令式封装：外层 UI 只通过 BpmnCanvasApi 操作画布。
 */

import type {
    BpmnCanvasApi,
    ConditionFields,
    ElementPatch,
    EngineFields,
    SelectedBpmnElement,
} from '../types';
import { bpmnToDefinition } from '../mapping/bpmnToDefinition';
import {
    asString,
    findEngineConfig,
    formatConditionExpression,
    parseLiteral,
    readDocumentation,
    readElementName,
    readEngineFields,
} from '../mapping/helpers';
import { classifyBpmnElement, isExecutableBpmnType } from '../mapping/nodeKinds';
import { ENGINE_CONFIG_TYPE } from '../moddle/assetsModdle';
import {
    createBusinessObjectByPresetId,
    createMessageEventDefinition,
    createTimerEventDefinition,
    normalizeTriggerEvent,
} from './createElements';

function asFlows(connections: any): any[] {
    return Array.isArray(connections)
        ? connections.filter((connection) => connection?.type === 'bpmn:SequenceFlow')
        : [];
}

/** 画布元素 → 属性面板视图模型 */
export function toSelectedElement(element: any): SelectedBpmnElement | null {
    const businessObject = element?.businessObject;
    if (!businessObject) return null;

    const type = asString(businessObject.$type) || asString(element.type);
    if (type === 'bpmn:Process' || type === 'bpmn:Collaboration' || type === 'label') return null;

    const engine = readEngineFields(businessObject);
    const kind = classifyBpmnElement(type, asString(businessObject.implementation), engine.kind);
    const eventDefinitionType = Array.isArray(businessObject.eventDefinitions)
        ? asString(businessObject.eventDefinitions[0]?.$type)
        : '';

    return {
        id: asString(businessObject.id) || asString(element.id),
        type,
        kind,
        name: readElementName(businessObject),
        documentation: readDocumentation(businessObject),
        executable: isExecutableBpmnType(type) && kind !== 'other',
        implementation: asString(businessObject.implementation) || undefined,
        calledElement: asString(businessObject.calledElement) || undefined,
        scriptFormat: asString(businessObject.scriptFormat) || undefined,
        script: typeof businessObject.script === 'string' ? businessObject.script : undefined,
        eventDefinition:
            eventDefinitionType
                .replace(/^bpmn:/, '')
                .replace(/EventDefinition$/, '')
                .toLowerCase() || undefined,
        incoming: asFlows(element.incoming).map((connection) => asString(connection.id)),
        outgoing: asFlows(element.outgoing).map((connection) => ({
            id: asString(connection.id),
            name: asString(connection.businessObject?.name),
            condition: asString(connection.businessObject?.conditionExpression?.body) || undefined,
        })),
        engine,
    };
}

function withoutEmptyValues(fields: Partial<EngineFields>): Record<string, unknown> {
    const attrs: Record<string, unknown> = {};
    Object.entries(fields).forEach(([key, value]) => {
        attrs[key] = value === '' ? undefined : value;
    });
    return attrs;
}


export function createModelerApi(modeler: any): BpmnCanvasApi {
    const canvas = modeler.get('canvas');
    const modeling = modeler.get('modeling');
    const selection = modeler.get('selection');
    const commandStack = modeler.get('commandStack');
    const elementRegistry = modeler.get('elementRegistry');
    const elementFactory = modeler.get('elementFactory');
    const bpmnFactory = modeler.get('bpmnFactory');
    const bpmnReplace = modeler.get('bpmnReplace');

    const definitionsRoot = () => canvas.getRootElement()?.businessObject?.$parent;

    /** 把触发方式同步为标准 timer / message 事件定义 */
    const applyTriggerEventDefinition = (element: any, triggerType: string | undefined): void => {
        if (element.businessObject?.$type !== 'bpmn:StartEvent') return;

        const eventKind = normalizeTriggerEvent(triggerType);
        let eventDefinitions: any[] = [];
        if (eventKind === 'timer') {
            eventDefinitions = [createTimerEventDefinition(bpmnFactory)];
        } else if (eventKind === 'message') {
            eventDefinitions = [
                createMessageEventDefinition(
                    bpmnFactory,
                    definitionsRoot(),
                    asString(element.businessObject.id)
                ),
            ];
        }
        modeling.updateProperties(element, { eventDefinitions });
    };

    /** 读取或创建 assets:EngineConfig（通过命令栈写入，支持撤销） */
    const setEngineFields = (
        element: any,
        fields: Partial<EngineFields>,
        options: { syncTriggerEvent?: boolean } = {}
    ): void => {
        let config = findEngineConfig(element.businessObject);
        if (!config) {
            config = bpmnFactory.create(ENGINE_CONFIG_TYPE);
            const extensionElements = element.businessObject.extensionElements;
            if (extensionElements) {
                modeling.updateModdleProperties(element, extensionElements, {
                    values: [...(extensionElements.values || []), config],
                });
            } else {
                modeling.updateProperties(element, {
                    extensionElements: bpmnFactory.create('bpmn:ExtensionElements', {
                        values: [config],
                    }),
                });
            }
        }

        const attrs = withoutEmptyValues(fields);
        if (Object.keys(attrs).length > 0) {
            modeling.updateModdleProperties(element, config, attrs);
        }

        if (fields.triggerType !== undefined && options.syncTriggerEvent !== false) {
            applyTriggerEventDefinition(element, fields.triggerType);
        }
        if (fields.wfType !== undefined && element.businessObject?.$type === 'bpmn:CallActivity') {
            modeling.updateProperties(element, { calledElement: fields.wfType });
        }
    };

    /** 条件分支：第一条出线为 yes（带 conditionExpression），其余为 no/default */
    const applyCondition = (element: any, value: ConditionFields): void => {
        const flows = asFlows(element.outgoing);
        const engine = readEngineFields(element.businessObject);
        const yesFlow =
            flows.find((flow) => asString(flow.businessObject?.name) === (engine.yesLabel || 'yes')) ||
            flows[0];
        const noFlow = flows.find((flow) => flow !== yesFlow);

        if (yesFlow) {
            const body = formatConditionExpression(
                value.field,
                value.operator,
                parseLiteral(value.value)
            );
            modeling.updateProperties(yesFlow, {
                name: value.yesLabel || 'yes',
                conditionExpression: body
                    ? bpmnFactory.create('bpmn:FormalExpression', { body })
                    : undefined,
            });
        }
        if (noFlow) {
            modeling.updateProperties(noFlow, {
                name: value.noLabel || 'no',
                conditionExpression: undefined,
            });
            modeling.updateProperties(element, { default: noFlow.businessObject });
        } else {
            modeling.updateProperties(element, { default: undefined });
        }
        setEngineFields(element, {
            kind: 'condition',
            field: value.field,
            operator: value.operator,
            value: value.value,
            yesLabel: value.yesLabel || 'yes',
            noLabel: value.noLabel || 'no',
        });
    };

    const setTriggerEvent = (element: any, value: 'none' | 'timer' | 'message'): void => {
        if (element.businessObject?.$type !== 'bpmn:StartEvent') return;

        applyTriggerEventDefinition(element, value);
        setEngineFields(
            element,
            {
                triggerType:
                    value === 'timer' ? 'scheduled' : value === 'message' ? 'webhook' : 'manual',
            },
            { syncTriggerEvent: false }
        );
    };

    const insertPresetAt = (
        presetId: string,
        position: { x: number; y: number },
        extra?: { engine?: Partial<EngineFields>; name?: string }
    ): any => {
        const created = createBusinessObjectByPresetId(presetId, bpmnFactory, {
            name: extra?.name,
            engine: extra?.engine,
            definitions: definitionsRoot(),
        });
        if (!created) return null;

        const shape = elementFactory.createShape({
            type: created.preset.bpmnType,
            businessObject: created.businessObject,
        });
        const element = modeling.createShape(shape, position, canvas.getRootElement());
        selection.select(element);
        return element;
    };

    const updateSelected = (patch: ElementPatch): void => {
        const element = selection.get()[0];
        if (!element) return;

        switch (patch.type) {
            case 'name':
                modeling.updateProperties(element, { name: patch.value });
                break;
            case 'documentation':
                modeling.updateProperties(element, {
                    documentation: patch.value
                        ? [bpmnFactory.create('bpmn:Documentation', { text: patch.value })]
                        : [],
                });
                break;
            case 'replace': {
                const replaced = bpmnReplace.replaceElement(element, { type: patch.value });
                if (replaced) selection.select(replaced);
                break;
            }
            case 'implementation': {
                modeling.updateProperties(element, { implementation: patch.value });
                setEngineFields(element, {
                    kind: patch.value.toUpperCase().includes('LLM') ? 'llm' : 'skill',
                });
                break;
            }
            case 'calledElement':
                modeling.updateProperties(element, { calledElement: patch.value });
                setEngineFields(element, { wfType: patch.value });
                break;
            case 'script':
                modeling.updateProperties(element, {
                    scriptFormat: patch.scriptFormat,
                    script: patch.script,
                });
                break;
            case 'triggerEvent':
                setTriggerEvent(element, patch.value);
                break;
            case 'engine':
                setEngineFields(element, patch.value);
                break;
            case 'condition':
                applyCondition(element, patch.value);
                break;
            default:
                break;
        }
    };

    const clearExecutionState = (): void => {
        elementRegistry.getAll().forEach((element: any) => {
            ['running', 'success', 'failed', 'skipped'].forEach((status) => {
                canvas.removeMarker(element.id, `assets-exec-${status}`);
            });
        });
    };

    return {
        saveXml: async () => {
            const result = await modeler.saveXML({ format: true });
            return result?.xml ?? '';
        },
        importXml: async (xml: string) => {
            await modeler.importXML(xml);
            selection.select([]);
            try {
                canvas.zoom('fit-viewport', 'auto');
            } catch {
                // 画布尺寸为 0（例如标签页隐藏）时 fit-viewport 会失败，忽略即可
            }
        },
        buildDefinition: (meta) => bpmnToDefinition(modeler, meta),
        updateProcessMeta: (meta) => {
            const root = canvas.getRootElement();
            const props: Record<string, unknown> = {};
            if (meta.name !== undefined) props.name = meta.name;
            if (meta.documentation !== undefined) {
                props.documentation = meta.documentation
                    ? [bpmnFactory.create('bpmn:Documentation', { text: meta.documentation })]
                    : [];
            }
            if (Object.keys(props).length > 0) modeling.updateProperties(root, props);
        },
        getProcessMeta: () => {
            const businessObject = canvas.getRootElement()?.businessObject;
            return {
                id: asString(businessObject?.id),
                name: asString(businessObject?.name),
                documentation: readDocumentation(businessObject),
            };
        },
        undo: () => commandStack.undo(),
        redo: () => commandStack.redo(),
        canUndo: () => commandStack.canUndo(),
        canRedo: () => commandStack.canRedo(),
        zoomIn: () => canvas.zoom(Math.min(4, canvas.zoom() * 1.15)),
        zoomOut: () => canvas.zoom(Math.max(0.2, canvas.zoom() / 1.15)),
        fitView: () => {
            try {
                canvas.zoom('fit-viewport', 'auto');
            } catch {
                // 布局未完成时忽略
            }
        },
        insertPreset: (presetId, extra) => {
            const viewbox = canvas.viewbox();
            insertPresetAt(
                presetId,
                {
                    x: Math.round(viewbox.x + viewbox.width / 2 - 50),
                    y: Math.round(viewbox.y + viewbox.height / 2 - 40),
                },
                extra
            );
        },
        insertPresetAt: (presetId, position, extra) => {
            insertPresetAt(presetId, position, extra);
        },
        updateSelected,
        getSelected: () => {
            const element = selection.get()[0];
            return element ? toSelectedElement(element) : null;
        },
        selectElement: (id: string) => {
            const element = elementRegistry.get(id);
            if (!element) return;
            selection.select(element);
            canvas.scrollToElement(element);
        },
        applyExecutionState: (results) => {
            clearExecutionState();
            const byId = new Map(results.map((result) => [result.node_id, result.status]));
            const statusClass = (status: string): string => {
                if (status === 'success') return 'success';
                if (status === 'failed') return 'failed';
                if (status === 'running') return 'running';
                return 'skipped';
            };
            elementRegistry.getAll().forEach((element: any) => {
                const status = byId.get(element.id);
                if (status) canvas.addMarker(element.id, `assets-exec-${statusClass(status)}`);
            });
        },
        clearExecutionState,
        getModeler: () => modeler,
    };
}

