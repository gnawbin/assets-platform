/**
 * 创建 bpmn-js Modeler 实例（客户端专用）。
 */

import BpmnModeler from 'bpmn-js/lib/Modeler';
import { assetsModdle } from '../moddle/assetsModdle';
import { AssetsPaletteModule } from './AssetsPaletteProvider';
import AssetsStylingModule from './AssetsStylingModule';

export function createModeler(container: HTMLElement): any {
    return new BpmnModeler({
        container,
        moddleExtensions: {
            assets: assetsModdle,
        },
        additionalModules: [AssetsPaletteModule, AssetsStylingModule as any],
    });
}
