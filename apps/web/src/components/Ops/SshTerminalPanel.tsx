'use client';

import React, { useEffect, useRef, useState } from 'react';
import { ActionIcon, Badge, Group, Paper, Stack, Text, Tooltip } from '@mantine/core';
import {
    IconArrowsMaximize,
    IconArrowsMinimize,
    IconCopy,
    IconPlugOff,
    IconRefresh,
    IconTerminal2,
} from '@tabler/icons-react';
import { notifications } from '@mantine/notifications';
import '@xterm/xterm/css/xterm.css';

import { useSshTerminal, type SshTerminalStatus } from '@/hooks/useSshTerminal';
import type { SshTargetView } from '@/services/sshService';

const STATUS_META: Record<SshTerminalStatus, { label: string; color: string }> = {
    idle: { label: '未连接', color: 'gray' },
    connecting: { label: '连接中', color: 'blue' },
    active: { label: '已连接', color: 'teal' },
    disconnected: { label: '已断开', color: 'yellow' },
    failed: { label: '连接失败', color: 'red' },
};

export interface SshTerminalPanelProps {
    /** 当前选中的主机；为空时展示空态 */
    target: SshTargetView | null;
    /** 会话建立 / 结束后的回调（用于刷新会话列表） */
    onSessionChange?: () => void;
}

export default function SshTerminalPanel({ target, onSessionChange }: SshTerminalPanelProps) {
    const containerRef = useRef<HTMLDivElement>(null);
    const rootRef = useRef<HTMLDivElement>(null);
    const { status, error, exitCode, connect, disconnect, getSelection } = useSshTerminal(containerRef);
    const connectedTargetId = useRef<string | null>(null);
    const [maximized, setMaximized] = useState(false);

    // 用户按 Esc 退出全屏时同步按钮图标
    useEffect(() => {
        const sync = () => setMaximized(Boolean(document.fullscreenElement));
        document.addEventListener('fullscreenchange', sync);
        return () => document.removeEventListener('fullscreenchange', sync);
    }, []);

    // 目标切换：自动建连；卸载或清空目标：主动断开
    useEffect(() => {
        const targetId = target?.id ?? null;
        if (!targetId) {
            if (connectedTargetId.current) {
                connectedTargetId.current = null;
                void disconnect();
                onSessionChange?.();
            }
            return;
        }
        if (connectedTargetId.current === targetId) return;
        connectedTargetId.current = targetId;
        void connect(targetId);
        onSessionChange?.();
        // connect / disconnect 为稳定引用，target 只按 id 变化触发
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [target?.id]);

    useEffect(
        () => () => {
            if (connectedTargetId.current) {
                connectedTargetId.current = null;
                onSessionChange?.();
            }
        },
        [onSessionChange],
    );

    const meta = STATUS_META[status];

    const reconnect = () => {
        if (!target) return;
        void connect(target.id);
        onSessionChange?.();
    };

    const closeNow = () => {
        void disconnect();
        onSessionChange?.();
    };

    /** 复制终端里选中的文本 */
    const copySelection = async () => {
        const text = getSelection();
        if (!text) {
            notifications.show({ color: 'gray', title: '没有选中内容', message: '请先在终端里框选要复制的文本' });
            return;
        }
        try {
            await navigator.clipboard.writeText(text);
            notifications.show({ color: 'teal', title: '已复制', message: `${text.length} 个字符` });
        } catch {
            notifications.show({ color: 'red', title: '复制失败', message: '浏览器拒绝了剪贴板写入' });
        }
    };

    /** 最大化 / 还原终端面板（浏览器 Fullscreen API） */
    const toggleMaximize = async () => {
        const node = rootRef.current;
        if (!node) return;
        try {
            if (document.fullscreenElement) {
                await document.exitFullscreen();
                setMaximized(false);
            } else {
                await node.requestFullscreen();
                setMaximized(true);
            }
        } catch {
            notifications.show({ color: 'red', title: '无法切换全屏', message: '浏览器拒绝了全屏请求' });
        }
    };

    return (
        <Paper
            ref={rootRef}
            withBorder
            radius="md"
            p="xs"
            style={{
                height: '100%',
                display: 'flex',
                flexDirection: 'column',
                background: 'var(--mantine-color-body)',
            }}
        >
            <Stack gap="xs" style={{ flex: 1, minHeight: 0 }}>
                <Group justify="space-between" wrap="nowrap">
                    <Group gap="xs" wrap="nowrap">
                        <IconTerminal2 size={18} />
                        <Text fw={600} size="sm" lineClamp={1}>
                            {target ? `${target.name}（${target.username}@${target.host}:${target.port}）` : '未选择主机'}
                        </Text>
                    </Group>
                    <Group gap="xs" wrap="nowrap">
                        {exitCode !== null && (
                            <Badge color="gray" variant="light">退出码 {exitCode}</Badge>
                        )}
                        <Badge color={meta.color} variant="light">{meta.label}</Badge>
                        <Tooltip label="复制选中内容">
                            <ActionIcon
                                variant="light"
                                aria-label="复制选中内容"
                                onClick={() => void copySelection()}
                            >
                                <IconCopy size={16} />
                            </ActionIcon>
                        </Tooltip>
                        <Tooltip label={maximized ? '还原' : '全屏'}>
                            <ActionIcon
                                variant="light"
                                aria-label={maximized ? '还原' : '全屏'}
                                onClick={() => void toggleMaximize()}
                            >
                                {maximized ? <IconArrowsMinimize size={16} /> : <IconArrowsMaximize size={16} />}
                            </ActionIcon>
                        </Tooltip>
                        <Tooltip label="重新连接">
                            <ActionIcon
                                variant="light"
                                aria-label="重新连接"
                                disabled={!target || status === 'connecting'}
                                onClick={reconnect}
                            >
                                <IconRefresh size={16} />
                            </ActionIcon>
                        </Tooltip>
                        <Tooltip label="断开">
                            <ActionIcon
                                variant="light"
                                color="red"
                                aria-label="断开"
                                disabled={status !== 'active' && status !== 'connecting'}
                                onClick={closeNow}
                            >
                                <IconPlugOff size={16} />
                            </ActionIcon>
                        </Tooltip>
                    </Group>
                </Group>

                {error && <Text size="xs" c="red">{error}</Text>}

                <div
                    ref={containerRef}
                    data-testid="ssh-terminal"
                    style={{
                        flex: 1,
                        minHeight: 320,
                        padding: 6,
                        borderRadius: 6,
                        overflow: 'hidden',
                        background: '#0b0f14',
                    }}
                />

                {!target && (
                    <Text size="sm" c="dimmed" ta="center" py="xs">
                        请在左侧选择一台主机后建立 SSH 会话
                    </Text>
                )}
            </Stack>
        </Paper>
    );
}
