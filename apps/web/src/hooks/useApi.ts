/**
 * useApi Hook
 *
 * 封装 API 调用的 React Hook，自动管理 loading / error / data 状态。
 * 避免在每个页面中重复编写 try/catch 和 loading 状态管理。
 *
 * 使用方式：
 * ```tsx
 * // 方式一：直接传入 Service 函数（推荐）
 * const { data, loading, error, execute } = useApi(getCategories);
 *
 * useEffect(() => { execute(); }, []);
 *
 * if (loading) return <Loader />;
 * if (error) return <Alert color="red">{error}</Alert>;
 * return <div>{data?.map(...)}</div>;
 *
 * // 方式二：带参数
 * const { execute } = useApi(deleteCategory);
 * await execute(categoryId);
 * ```
 */

'use client';

import { useState, useCallback, useRef, useEffect } from 'react';

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
export interface UseApiReturn<T, P extends unknown[]> extends UseApiState<T> {
    /** 执行 API 调用 */
    execute: (...args: P) => Promise<T | null>;
    /** 重置状态 */
    reset: () => void;
    /** 手动设置数据 */
    setData: (data: T | null) => void;
}

// ======================== Hook ========================

/**
 * useApi - 统一 API 调用 Hook
 *
 * @param apiFn Service 层的 API 函数
 * @returns UseApiReturn<T, P>
 */
export function useApi<T, P extends unknown[] = []>(
    apiFn?: (...args: P) => Promise<T>,
): UseApiReturn<T, P> {
    const [state, setState] = useState<UseApiState<T>>({
        data: null,
        loading: false,
        error: null,
    });

    // 使用 ref 防止组件卸载后继续更新状态
    const mountedRef = useRef(true);
    const apiFnRef = useRef(apiFn);

    useEffect(() => {
        mountedRef.current = true;
        return () => {
            mountedRef.current = false;
        };
    }, []);

    useEffect(() => {
        apiFnRef.current = apiFn;
    }, [apiFn]);

    const execute = useCallback(
        async (...args: P): Promise<T | null> => {
            if (!apiFnRef.current) {
                console.warn('[useApi] 未提供 API 函数');
                return null;
            }

            setState((prev) => ({ ...prev, loading: true, error: null }));

            try {
                const result = await apiFnRef.current(...args);

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
        [],
    );

    const reset = useCallback(() => {
        setState({ data: null, loading: false, error: null });
    }, []);

    const setData = useCallback((data: T | null) => {
        setState((prev) => ({ ...prev, data }));
    }, []);

    return { ...state, execute, reset, setData };
}
