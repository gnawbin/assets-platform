/**
 * API 适配器运行时管理器
 *
 * 提供在运行时动态切换适配器的能力，支持：
 * - 构建时通过环境变量设置默认适配器
 * - 运行时通过 setAdapter() 动态切换
 * - 自动检测 Tauri 环境并切换
 *
 * 桌面版和 Web 版可以共存，共享同一套前端代码：
 * - 在 Tauri 中运行时，自动使用 tauri 适配器
 * - 在浏览器中运行时，自动使用 http 适配器
 */

import type { IApiAdapter } from './types';
import { tauriAdapter } from './tauriAdapter';
import { httpAdapter } from './httpAdapter';

// ======================== 适配器类型 ========================

/** 支持的适配器类型 */
export type AdapterType = 'tauri' | 'http';

// ======================== 状态 ========================

let currentAdapter: IApiAdapter | null = null;
let currentAdapterType: AdapterType | null = null;

// ======================== 环境检测 ========================

/**
 * 检测是否运行在 Tauri 环境中
 */
function isTauriEnvironment(): boolean {
    try {
        return typeof window !== 'undefined' && window.__TAURI__ !== undefined;
    } catch {
        return false;
    }
}

/**
 * 获取默认适配器类型
 *
 * 优先级：
 * 1. 运行时已设置的适配器
 * 2. 环境变量 NEXT_PUBLIC_API_ADAPTER
 * 3. 自动检测 Tauri 环境
 * 4. 兜底使用 'tauri'
 */
function getDefaultAdapterType(): AdapterType {
    const envAdapter = process.env.NEXT_PUBLIC_API_ADAPTER as AdapterType | undefined;

    if (envAdapter === 'http' || envAdapter === 'tauri') {
        return envAdapter;
    }

    // 自动检测 Tauri 环境
    if (isTauriEnvironment()) {
        return 'tauri';
    }

    // 兜底：默认使用 tauri（HTTP 后端服务尚未就绪时更安全）
    return 'tauri';

}

// ======================== 管理器方法 ========================

/**
 * 设置当前适配器
 *
 * @param type 适配器类型：'tauri' | 'http'
 *
 * @example
 * ```ts
 * import { setAdapter } from '@/utils/adapters/manager';
 *
 * // 切换到 HTTP 适配器（Web 版）
 * setAdapter('http');
 *
 * // 切换到 Tauri 适配器（桌面版）
 * setAdapter('tauri');
 * ```
 */
export function setAdapter(type: AdapterType): void {
    switch (type) {
        case 'http':
            currentAdapter = httpAdapter;
            currentAdapterType = 'http';
            break;
        case 'tauri':
        default:
            currentAdapter = tauriAdapter;
            currentAdapterType = 'tauri';
            break;
    }
}

/**
 * 获取当前适配器
 *
 * 如果尚未设置，则根据环境变量或自动检测初始化默认适配器。
 */
export function getAdapter(): IApiAdapter {
    if (!currentAdapter) {
        const defaultType = getDefaultAdapterType();
        setAdapter(defaultType);
    }
    return currentAdapter!;
}

/**
 * 获取当前适配器类型
 */
export function getAdapterType(): AdapterType {
    if (!currentAdapterType) {
        const defaultType = getDefaultAdapterType();
        setAdapter(defaultType);
    }
    return currentAdapterType!;
}

/**
 * 重置适配器为默认值
 *
 * 下次调用 getAdapter() 时会重新根据环境变量或自动检测初始化。
 */
export function resetAdapter(): void {
    currentAdapter = null;
    currentAdapterType = null;
}
