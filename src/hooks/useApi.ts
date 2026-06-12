/**
 * useApi Hook
 *
 * 封装 API 调用的 React Hook，自动管理 loading / error / data 状态。
 * 避免在每个页面中重复编写 try/catch 和 loading 状态管理。
 *
 * 使用方式：
 * ```tsx
 * const { data, loading, error, execute } = useApi<Category[]>();
 *
 * useEffect(() => {
 *   execute('get_categories');
 * }, []);
 *
 * if (loading) return <Loader />;
 * if (error) return <Alert color="red">{error}</Alert>;
 * return <div>{data?.map(...)}</div>;
 * ```
 */

'use client';

import { useState, useCallback, useRef } from 'react';
import { api, type ApiConfig } from '@/utils/api';

// ======================== 类型定义 ========================

/** useApi 返回的状态 */
export interface UseApiState<T> {
    /** 响应数据 */
    data: T | null;
    /** 是否加载中 */
    loading: boolean;
    /** 错误信息 */
    error: string | null;
}

/** useApi 返回值 */
export interface UseApiReturn<T> extends UseApiState<T> {
    /** 执行 API 调用 */
    execute: (
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ) => Promise<T | null>;
    /** 重置状态 */
    reset: () => void;
    /** 手动设置数据 */
    setData: (data: T | null) => void;
}

// ======================== Hook ========================

/**
 * useApi - 统一 API 调用 Hook
 *
 * @param defaultConfig 默认 API 配置
 * @returns UseApiReturn<T>
 */
export function useApi<T = unknown>(
    defaultConfig?: ApiConfig,
): UseApiReturn<T> {
    const [state, setState] = useState<UseApiState<T>>({
        data: null,
        loading: false,
        error: null,
    });

    // 使用 ref 防止组件卸载后继续更新状态
    const mountedRef = useRef(true);

    const execute = useCallback(
        async (
            command: string,
            args?: Record<string, unknown>,
            config?: ApiConfig,
        ): Promise<T | null> => {
            setState((prev) => ({ ...prev, loading: true, error: null }));

            try {
                const result = await api.get<T>(command, args, {
                    ...defaultConfig,
                    ...config,
                });

                if (mountedRef.current) {
                    setState({ data: result, loading: false, error: null });
                }
                return result;
            } catch (err) {
                const message =
                    typeof err === 'string'
                        ? err
                        : err instanceof Error
                            ? err.message
                            : '操作失败';

                if (mountedRef.current) {
                    setState((prev) => ({
                        ...prev,
                        loading: false,
                        error: message,
                    }));
                }
                return null;
            }
        },
        [defaultConfig],
    );

    const reset = useCallback(() => {
        setState({ data: null, loading: false, error: null });
    }, []);

    const setData = useCallback((data: T | null) => {
        setState((prev) => ({ ...prev, data }));
    }, []);

    return { ...state, execute, reset, setData };
}
