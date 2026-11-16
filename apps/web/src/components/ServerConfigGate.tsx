'use client';

/**
 * 首次启动的服务器连接 Gate
 *
 * 触发条件（任一）：
 * 1. `getApiBase()` 为 `null`（SSR / 未知宿主，完全没有可用地址）；
 * 2. 当前地址来自**兼容兜底**（桌面 / 开发模式的本机 3001）且 `GET /healthz` 探活失败
 *    —— 说明用户其实想连别的服务器，但既没填地址也没设 env，继续渲染只会得到一屏 401 / 网络错误。
 *
 * 用户手动保存过地址（`storage`）或地址由部署注入（`env` / `injected` / 同源）时**一律放行**，
 * 因此 nginx 同源部署与本地开发完全不受影响。
 *
 * 探活结果按 origin 缓存在 `sessionStorage`（5 分钟），避免每次刷新都打一次 `/healthz`。
 */

import React, { useEffect, useState } from 'react';
import { Center, Container, Loader, Paper, Stack, Text, Title } from '@mantine/core';
import { describeApiBase, isFallbackBase, probeServer } from '@/utils/apiBase';
import { ServerConnectionForm } from './ServerConnectionForm';

/** 探活成功缓存（sessionStorage） */
const PROBE_CACHE_KEY = 'api_base_probe_ok';
/** 缓存有效期：5 分钟（跨页面刷新复用，换服务器地址后 origin 不匹配即失效） */
const PROBE_CACHE_TTL_MS = 5 * 60 * 1000;

type GateState = 'probing' | 'ready' | 'need-config';

function readProbeCache(origin: string): boolean {
    try {
        const raw = window.sessionStorage.getItem(PROBE_CACHE_KEY);
        if (!raw) {
            return false;
        }
        const parsed = JSON.parse(raw) as { origin?: string; at?: number };
        if (parsed?.origin !== origin || typeof parsed.at !== 'number') {
            return false;
        }
        return Date.now() - parsed.at < PROBE_CACHE_TTL_MS;
    } catch {
        return false;
    }
}

function writeProbeCache(origin: string): void {
    try {
        window.sessionStorage.setItem(
            PROBE_CACHE_KEY,
            JSON.stringify({ origin, at: Date.now() }),
        );
    } catch {
        // 忽略：缓存失败只影响下次是否重复探活
    }
}

export interface ServerConfigGateProps {
    children: React.ReactNode;
}

export function ServerConfigGate({ children }: ServerConfigGateProps) {
    // 首帧统一渲染 Loader：客户端组件在静态导出时也会预渲染 HTML，
    // 只有统一初值才能避免「服务端 need-config / 客户端 ready」的水合不一致。
    const [state, setState] = useState<GateState>('probing');
    const snapshot = describeApiBase();

    useEffect(() => {
        let cancelled = false;

        const decide = async () => {
            const current = describeApiBase();
            if (!current.origin) {
                if (!cancelled) {
                    setState('need-config');
                }
                return;
            }
            if (!isFallbackBase()) {
                // 用户显式配置 / 部署注入 / nginx 同源：信任，不做额外探活
                if (!cancelled) {
                    setState('ready');
                }
                return;
            }
            if (readProbeCache(current.origin)) {
                if (!cancelled) {
                    setState('ready');
                }
                return;
            }
            const result = await probeServer(current.origin);
            if (cancelled) {
                return;
            }
            if (result.ok) {
                writeProbeCache(current.origin);
                setState('ready');
            } else {
                setState('need-config');
            }
        };

        void decide();
        return () => {
            cancelled = true;
        };
    }, []);

    if (state === 'ready') {
        return <>{children}</>;
    }

    if (state === 'probing') {
        return (
            <Center mih="60vh">
                <Stack align="center" gap="sm">
                    <Loader />
                    <Text size="sm" c="dimmed">
                        正在连接服务器…
                    </Text>
                </Stack>
            </Center>
        );
    }

    return (
        <Center mih="100vh" bg="var(--mantine-color-gray-0)" p="md">
            <Container size={520} w="100%">
                <Paper withBorder shadow="lg" p="xl" radius="md" data-testid="server-config-gate">
                    <Stack gap="md">
                        <Title order={3}>先连接服务器</Title>
                        <Text size="sm" c="dimmed">
                            {snapshot.origin
                                ? `默认地址 ${snapshot.origin} 无法连接（探测 /healthz 失败）。请填写实际的后端地址。`
                                : '未能确定后端地址，请填写服务器地址后继续。'}
                        </Text>
                        <ServerConnectionForm mode="gate" />
                    </Stack>
                </Paper>
            </Container>
        </Center>
    );
}

export default ServerConfigGate;
