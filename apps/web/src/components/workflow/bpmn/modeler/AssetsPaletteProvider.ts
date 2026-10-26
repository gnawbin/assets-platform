/**
 * 业务模板面板（替换 bpmn-js 默认 Palette）。
 *
 * 所有条目仍然创建标准 BPMN 元素，只是预先写入 `assets:EngineConfig` 与
 * implementation，帮助用户快速搭建可执行流程。
 */

import { createBusinessObjectForPreset } from './createElements';
import { defaultEngineForPreset, getPreset, NODE_PRESETS } from '../mapping/nodeKinds';

export interface PaletteEntry {
    group: string;
    className: string;
    title: string;
    action: {
        dragstart: (event: Event) => void;
        click: (event: Event) => void;
    };
}

export default class AssetsPaletteProvider {
    static $inject = ['palette', 'create', 'elementFactory', 'bpmnFactory', 'translate'];

    private readonly palette: any;
    private readonly create: any;
    private readonly elementFactory: any;
    private readonly bpmnFactory: any;
    private readonly translate: (text: string) => string;

    constructor(palette: any, create: any, elementFactory: any, bpmnFactory: any, translate: any) {
        this.palette = palette;
        this.create = create;
        this.elementFactory = elementFactory;
        this.bpmnFactory = bpmnFactory;
        this.translate = translate;
        palette.registerProvider(2000, this);
    }

    getPaletteEntries(): Record<string, PaletteEntry> {
        const entries: Record<string, PaletteEntry> = {};

        NODE_PRESETS.forEach((preset) => {
            entries[`create.assets-${preset.id}`] = {
                group: 'assets',
                className: preset.paletteIcon,
                title: this.translate(`${preset.label} — ${preset.description}`),
                action: {
                    dragstart: (event: Event) => this.startCreate(event, preset.id),
                    click: (event: Event) => this.startCreate(event, preset.id),
                },
            };
        });

        return entries;
    }

    getGroups(): Record<string, { order: number; title: string }> {
        return {
            assets: {
                order: -1,
                title: this.translate('业务模板'),
            },
        };
    }

    private startCreate(event: Event, presetId: string): void {
        const preset = getPreset(presetId);
        if (!preset) return;

        const businessObject = createBusinessObjectForPreset(preset, this.bpmnFactory, {
            name: preset.label,
            engine: defaultEngineForPreset(preset),
        });
        const shape = this.elementFactory.createShape({
            type: preset.bpmnType,
            businessObject,
        });
        this.create.start(event, shape);
    }
}

export const AssetsPaletteModule = {
    __init__: ['assetsPaletteProvider'],
    assetsPaletteProvider: ['type', AssetsPaletteProvider],
};
