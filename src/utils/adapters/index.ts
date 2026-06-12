/**
 * API 适配器统一导出
 *
 * 根据环境变量 NEXT_PUBLIC_API_ADAPTER 自动选择适配器：
 * - 'tauri'（默认）：通过 Tauri invoke 调用 Rust 后端
 * - 'http'：通过 HTTP fetch 调用 REST API（未来前后端分离时使用）
 */

import type { IApiAdapter } from './types';
import { tauriAdapter } from './tauriAdapter';
import { httpAdapter } from './httpAdapter';

export type { IApiAdapter, ApiConfig } from './types';
export { tauriAdapter } from './tauriAdapter';
export { httpAdapter } from './httpAdapter';

/**
 * 获取当前 API 适配器
 *
 * 通过环境变量 NEXT_PUBLIC_API_ADAPTER 控制：
 * - 桌面版（Tauri）：不设置或设置为 'tauri'
 * - Web 版（前后端分离）：设置为 'http'
 */
export function getAdapter(): IApiAdapter {
    const adapterType = process.env.NEXT_PUBLIC_API_ADAPTER || 'tauri';

    switch (adapterType) {
        case 'http':
            return httpAdapter;
        case 'tauri':
        default:
            return tauriAdapter;
    }
}
