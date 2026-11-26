'use client';

/**
 * 容器性能轮询 Hook
 *
 * - Docker：打开即拉一次，之后每 2s 用 `twoSamples=true` 取两条样本算 CPU%；
 * - K8s：metrics-server 默认 15s 聚合窗口，因此 5s 轮询一次即可；
 * - `document.hidden` 时暂停轮询，切回前台立即刷新；
 * - 最近 60 个采样点保存在环形缓冲里，供 Sparkline / 趋势展示。
 */

import { useCallback, useEffect, useRef, useState } from 'react';

import {
    getOpsContainerStats,
    getOpsPodMetrics,
    type OpsContainerStatsView,
    type OpsPodMetricsView,
} from '@/services/opsService';

/** 单个采样点（CPU% 可能为 null：one-shot 首次采样没有基线） */
export interface ContainerStatsPoint {
    at: number;
    cpu: number | null;
    memory: number;
}

const MAX_POINTS = 60;
const DOCKER_INTERVAL_MS = 2000;
const K8S_INTERVAL_MS = 5000;

export interface UseContainerStatsOptions {
    provider: 'docker' | 'k8s';
    connectionId: string;
    container?: string | null;
    namespace?: string | null;
    pod?: string | null;
    /** 仅在当前 Tab 可见时传 true */
    enabled: boolean;
}

export interface UseContainerStatsResult {
    dockerStats: OpsContainerStatsView | null;
    podMetrics: OpsPodMetricsView | null;
    points: ContainerStatsPoint[];
    loading: boolean;
    error: string | null;
    refresh: () => Promise<void>;
}

export function useContainerStats(options: UseContainerStatsOptions): UseContainerStatsResult {
    const { provider, connectionId, container, namespace, pod, enabled } = options;
    const [dockerStats, setDockerStats] = useState<OpsContainerStatsView | null>(null);
    const [podMetrics, setPodMetrics] = useState<OpsPodMetricsView | null>(null);
    const [points, setPoints] = useState<ContainerStatsPoint[]>([]);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const timerRef = useRef<number | null>(null);
    const disposedRef = useRef(false);

    const clearTimer = useCallback(() => {
        if (timerRef.current !== null) {
            window.clearTimeout(timerRef.current);
            timerRef.current = null;
        }
    }, []);

    const pushPoint = useCallback((cpu: number | null, memory: number) => {
        setPoints((prev) => {
            const next = [...prev, { at: Date.now(), cpu, memory }];
            return next.length > MAX_POINTS ? next.slice(next.length - MAX_POINTS) : next;
        });
    }, []);

    const fetchOnce = useCallback(async () => {
        if (!enabled || disposedRef.current) return;
        setLoading(true);
        setError(null);
        try {
            if (provider === 'docker') {
                if (!container) return;
                const view = await getOpsContainerStats(connectionId, container, true);
                setDockerStats(view);
                pushPoint(view.cpu_percent, view.mem_used_bytes);
            } else {
                if (!namespace || !pod) return;
                const view = await getOpsPodMetrics(connectionId, namespace, pod);
                setPodMetrics(view);
                pushPoint(null, view.memory_bytes);
            }
        } catch (err) {
            setError(err instanceof Error ? err.message : '加载性能指标失败');
        } finally {
            setLoading(false);
        }
    }, [connectionId, container, enabled, namespace, pod, provider, pushPoint]);

    // 轮询：enabled 变化（切 Tab / 开抽屉）时重建；页面隐藏时暂停
    useEffect(() => {
        if (!enabled) return undefined;
        disposedRef.current = false;
        setPoints([]);
        void fetchOnce();

        const interval = provider === 'docker' ? DOCKER_INTERVAL_MS : K8S_INTERVAL_MS;
        const schedule = () => {
            clearTimer();
            timerRef.current = window.setTimeout(async () => {
                if (document.hidden) {
                    schedule();
                    return;
                }
                await fetchOnce();
                schedule();
            }, interval);
        };
        schedule();

        const onVisibility = () => {
            if (!document.hidden) {
                void fetchOnce();
            }
        };
        document.addEventListener('visibilitychange', onVisibility);

        return () => {
            disposedRef.current = true;
            clearTimer();
            document.removeEventListener('visibilitychange', onVisibility);
        };
    }, [clearTimer, enabled, fetchOnce, provider]);

    return { dockerStats, podMetrics, points, loading, error, refresh: fetchOnce };
}
