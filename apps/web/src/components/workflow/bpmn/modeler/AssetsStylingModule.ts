/**
 * 通过 diagram-js marker 给标准 BPMN 元素着色，区分业务语义。
 *
 * 只影响渲染，不改变 BPMN XML；颜色规则见 bpmn.css。
 * 使用 class + $inject，避免生产构建压缩参数名后依赖注入失败。
 */

import { classifyBpmnElement } from '../mapping/nodeKinds';
import { readEngineFields } from '../mapping/helpers';
import type { ElementKind } from '../types';

const KIND_MARKERS = [
    'assets-kind-trigger',
    'assets-kind-skill',
    'assets-kind-llm',
    'assets-kind-condition',
    'assets-kind-code',
    'assets-kind-workflow',
    'assets-kind-output',
];

function markerClass(kind: ElementKind): string | null {
    return kind === 'other' ? null : `assets-kind-${kind}`;
}

class AssetsStylingService {
    static $inject = ['eventBus', 'canvas', 'elementRegistry'];

    private readonly canvas: any;
    private readonly elementRegistry: any;

    constructor(eventBus: any, canvas: any, elementRegistry: any) {
        this.canvas = canvas;
        this.elementRegistry = elementRegistry;

        eventBus.on('import.done', (event: any) => {
            if (event?.error) return;
            this.elementRegistry.getAll().forEach((element: any) => this.decorate(element));
        });
        eventBus.on('shape.added', (event: any) => this.decorate(event.element));
        eventBus.on('element.changed', (event: any) => this.decorate(event.element));
    }

    private decorate(element: any): void {
        if (!element?.businessObject) return;
        const type: string = element.businessObject.$type || element.type;
        if (
            !type ||
            type === 'bpmn:Process' ||
            type === 'bpmn:Collaboration' ||
            type === 'bpmn:SequenceFlow' ||
            type === 'label'
        ) {
            return;
        }

        const engine = readEngineFields(element.businessObject);
        const kind = classifyBpmnElement(type, element.businessObject.implementation, engine.kind);
        const marker = markerClass(kind);

        KIND_MARKERS.forEach((name) => this.canvas.removeMarker(element.id, name));
        if (marker) this.canvas.addMarker(element.id, marker);
    }
}

const AssetsStylingModule = {
    __init__: ['assetsStyling'],
    assetsStyling: ['type', AssetsStylingService],
};

export default AssetsStylingModule;
