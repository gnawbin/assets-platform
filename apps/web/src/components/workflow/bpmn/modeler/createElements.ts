/**
 * 根据业务模板创建标准 BPMN 业务对象。
 *
 * 这里只依赖 moddle 工厂，不直接依赖 bpmn-js 的类型定义，便于升级。
 */

import type { EngineFields, ElementKind } from '../types';
import { ENGINE_CONFIG_TYPE } from '../moddle/assetsModdle';
import { asString } from '../mapping/helpers';
import { defaultEngineForPreset, getPreset, type NodePreset } from '../mapping/nodeKinds';

export interface CreateElementOptions {
    id?: string;
    name?: string;
    engine?: Partial<EngineFields>;
    eventDefinition?: 'none' | 'timer' | 'message';
    scriptFormat?: string;
    script?: string;
    calledElement?: string;
    /** definitions 根元素，用于挂载 bpmn:Message */
    definitions?: any;
}

/** 过滤空值，避免在 XML 中写出 `skillId=""` 这样的噪声 */
export function normalizeEngineAttrs(engine: Partial<EngineFields> | undefined): Record<string, unknown> {
    const attrs: Record<string, unknown> = {};
    Object.entries(engine || {}).forEach(([key, value]) => {
        if (value === undefined || value === null || value === '') return;
        attrs[key] = value;
    });
    return attrs;
}

export function createEngineConfig(bpmnFactory: any, engine: Partial<EngineFields> | undefined): any {
    return bpmnFactory.create(ENGINE_CONFIG_TYPE, normalizeEngineAttrs(engine));
}

function createExtensionElements(
    bpmnFactory: any,
    engine: Partial<EngineFields> | undefined
): any | undefined {
    const attrs = normalizeEngineAttrs(engine);
    if (Object.keys(attrs).length === 0) return undefined;
    return bpmnFactory.create('bpmn:ExtensionElements', {
        values: [bpmnFactory.create(ENGINE_CONFIG_TYPE, attrs)],
    });
}

/** timer 事件需要一个合法的 timeCycle，避免 bpmn-js 校验报错 */
export function createTimerEventDefinition(bpmnFactory: any): any {
    return bpmnFactory.create('bpmn:TimerEventDefinition', {
        timeCycle: bpmnFactory.create('bpmn:FormalExpression', { body: 'R/PT24H' }),
    });
}

/** message 事件引用根元素中的 bpmn:Message */
export function createMessageEventDefinition(
    bpmnFactory: any,
    definitions: any,
    baseId: string
): any {
    const messageId = `Message_${(baseId || 'Webhook').replace(/[^\w-]/g, '_')}`;
    let message = Array.isArray(definitions?.rootElements)
        ? definitions.rootElements.find(
              (element: any) => element?.$type === 'bpmn:Message' && element.id === messageId
          )
        : undefined;

    if (!message) {
        message = bpmnFactory.create('bpmn:Message', {
            id: messageId,
            name: 'webhook',
        });
        if (Array.isArray(definitions?.rootElements)) {
            definitions.rootElements.push(message);
        }
    }

    return bpmnFactory.create('bpmn:MessageEventDefinition', { messageRef: message });
}

function resolveEventDefinitions(
    bpmnFactory: any,
    kind: ElementKind,
    options: CreateElementOptions,
    businessObject: any
): any[] {
    const eventDefinition =
        options.eventDefinition ||
        (kind === 'trigger' ? normalizeTriggerEvent(options.engine?.triggerType) : 'none');

    if (eventDefinition === 'timer') {
        return [createTimerEventDefinition(bpmnFactory)];
    }
    if (eventDefinition === 'message') {
        return [createMessageEventDefinition(bpmnFactory, options.definitions, asString(businessObject.id))];
    }
    return [];
}

/** 执行参数里的 triggerType 映射到标准事件定义 */
export function normalizeTriggerEvent(triggerType: string | undefined): 'none' | 'timer' | 'message' {
    if (triggerType === 'scheduled' || triggerType === 'timer') return 'timer';
    if (triggerType === 'webhook' || triggerType === 'message') return 'message';
    return 'none';
}

/** 根据模板创建业务对象（标准 BPMN 类型 + assets 扩展） */
export function createBusinessObjectForPreset(
    preset: NodePreset,
    bpmnFactory: any,
    options: CreateElementOptions = {}
): any {
    const engine: Partial<EngineFields> = {
        ...defaultEngineForPreset(preset),
        kind: preset.kind === 'skill' || preset.kind === 'llm' ? preset.kind : undefined,
        ...normalizeEngineAttrs(options.engine),
    };
    const name = options.name ?? preset.label;
    const baseAttrs: Record<string, unknown> = { name };
    const extensionElements = createExtensionElements(bpmnFactory, engine);
    if (extensionElements) baseAttrs.extensionElements = extensionElements;
    if (options.id) baseAttrs.id = options.id;

    switch (preset.bpmnType) {
        case 'bpmn:StartEvent': {
            const businessObject = bpmnFactory.create('bpmn:StartEvent', baseAttrs);
            const eventDefinitions = resolveEventDefinitions(bpmnFactory, preset.kind, options, businessObject);
            if (eventDefinitions.length > 0) {
                businessObject.eventDefinitions = eventDefinitions;
            }
            return businessObject;
        }
        case 'bpmn:ServiceTask':
            return bpmnFactory.create('bpmn:ServiceTask', {
                ...baseAttrs,
                implementation: options.engine?.kind === 'llm' || engine.kind === 'llm' ? '##LLM' : '##Skill',
            });
        case 'bpmn:ScriptTask':
            return bpmnFactory.create('bpmn:ScriptTask', {
                ...baseAttrs,
                scriptFormat: options.scriptFormat || 'python',
                script: options.script ?? '',
            });
        case 'bpmn:ExclusiveGateway':
            return bpmnFactory.create('bpmn:ExclusiveGateway', baseAttrs);
        case 'bpmn:CallActivity':
            return bpmnFactory.create('bpmn:CallActivity', {
                ...baseAttrs,
                calledElement: options.calledElement || asString(options.engine?.wfType),
            });
        case 'bpmn:EndEvent':
            return bpmnFactory.create('bpmn:EndEvent', baseAttrs);
        default:
            return bpmnFactory.create(preset.bpmnType, baseAttrs);
    }
}

/** 通过 presetId 创建（供拖放 / 点击插入使用） */
export function createBusinessObjectByPresetId(
    presetId: string,
    bpmnFactory: any,
    options: CreateElementOptions = {}
): { preset: NodePreset; businessObject: any } | null {
    const preset = getPreset(presetId);
    if (!preset) return null;
    return { preset, businessObject: createBusinessObjectForPreset(preset, bpmnFactory, options) };
}
