/**
 * API 适配器统一导出
 *
 * 提供运行时动态切换适配器的能力：
 * - 构建时通过环境变量 NEXT_PUBLIC_API_ADAPTER 设置默认值
 * - 运行时通过 setAdapter() 动态切换
 * - 自动检测 Tauri 环境并选择合适的适配器
 *
 * 桌面版和 Web 版可以共存，共享同一套前端代码。
 */

export type { IApiAdapter, ApiConfig } from './types';
export { tauriAdapter } from './tauriAdapter';
export { httpAdapter } from './httpAdapter';
export {
    setAdapter,
    getAdapter,
    getAdapterType,
    resetAdapter,
} from './manager';
export type { AdapterType } from './manager';


