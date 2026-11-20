'use client';

/**
 * 容器终端 Hook：Bearer 换票据 → WS 升级 → 全双工字节流
 *
 * 实现与 `useSshTerminal` 完全同构，只替换 REST 端点 / WS 地址：
 * - 输入走 Binary 帧（`term.onData` → `TextEncoder`）；
 * - 输出走 Binary 帧直接 `term.write`（保留 ANSI 转义）；
 * - resize 用 ResizeObserver + 120ms 防抖，发 `{type:'resize'}`；
 * - 卸载 / 断开时先关 socket，再 `closeOpsTerminalSession`（后端幂等）。
 */

import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';
import type { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';

import {
    buildOpsTerminalWsUrl,
    closeOpsTerminalSession,
    openOpsTerminalSession,
    type OpsTerminalTarget,
} from '@/services/opsService';

export type ContainerTerminalStatus = 'idle' | 'connecting' | 'active' | 'disconnected' | 'failed';

interface ServerFrame {
    type?: string;
    code?: number | null;
    reason?: string;
}

function errorMessage(err: unknown): string {
    if (err instanceof Error) return err.message;
    if (typeof err === 'string') return err;
    return '未知错误';
}

export interface UseContainerTerminalResult {
    status: ContainerTerminalStatus;
    error: string | null;
    exitCode: number | null;
    connect: (
        connectionId: string,
        target: OpsTerminalTarget,
        shell?: string | null,
    ) => Promise<void>;
    getSelection: () => string;
    disconnect: () => Promise<void>;
}

export function useContainerTerminal(
    containerRef: RefObject<HTMLDivElement | null>,
): UseContainerTerminalResult {
    const termRef = useRef<Terminal | null>(null);
    const fitRef = useRef<FitAddon | null>(null);
    const wsRef = useRef<WebSocket | null>(null);
    const sessionRef = useRef<string | null>(null);
    const resizeTimerRef = useRef<number | null>(null);
    const observerRef = useRef<(() => void) | null>(null);
    const readyRef = useRef<Promise<void> | null>(null);
    const resolveReadyRef = useRef<(() => void) | null>(null);
    if (readyRef.current === null) {
        readyRef.current = new Promise<void>((resolve) => {
            resolveReadyRef.current = resolve;
        });
    }

    const [status, setStatus] = useState<ContainerTerminalStatus>('idle');
    const [error, setError] = useState<string | null>(null);
    const [exitCode, setExitCode] = useState<number | null>(null);

    const sendResize = useCallback(() => {
        const term = termRef.current;
        const ws = wsRef.current;
        if (!term || !ws || ws.readyState !== WebSocket.OPEN) return;
        ws.send(JSON.stringify({ type: 'resize', cols: term.cols, rows: term.rows }));
    }, []);

    // 创建终端（xterm 动态导入，避免 SSR 触碰 window）
    useEffect(() => {
        const container = containerRef.current;
        if (!container) return undefined;
        let disposed = false;

        void (async () => {
            const { Terminal: XtermTerminal } = await import('@xterm/xterm');
            if (disposed) return;
            const term = new XtermTerminal({
                cursorBlink: true,
                convertEol: false,
                fontFamily: 'Menlo, Consolas, "Courier New", monospace',
                fontSize: 13,
                scrollback: 5000,
                theme: {
                    background: '#0b0f14',
                    foreground: '#d8dee9',
                    cursor: '#88c0d0',
                },
            });
            const fit = new FitAddon();
            term.loadAddon(fit);
            term.open(container);
            fit.fit();
            termRef.current = term;
            fitRef.current = fit;

            term.onData((data: string) => {
                const ws = wsRef.current;
                if (ws && ws.readyState === WebSocket.OPEN) {
                    ws.send(new TextEncoder().encode(data));
                }
            });

            const observer = new ResizeObserver(() => {
                if (resizeTimerRef.current !== null) {
                    window.clearTimeout(resizeTimerRef.current);
                }
                resizeTimerRef.current = window.setTimeout(() => {
                    fitRef.current?.fit();
                    sendResize();
                }, 120);
            });
            observer.observe(container);
            observerRef.current = () => observer.disconnect();
            resolveReadyRef.current?.();
        })();

        return () => {
            disposed = true;
            observerRef.current?.();
            observerRef.current = null;
            if (resizeTimerRef.current !== null) {
                window.clearTimeout(resizeTimerRef.current);
                resizeTimerRef.current = null;
            }
            termRef.current?.dispose();
            termRef.current = null;
            fitRef.current = null;
            resolveReadyRef.current?.();
            resolveReadyRef.current = null;
        };
    }, [containerRef, sendResize]);

    const disconnect = useCallback(async () => {
        const ws = wsRef.current;
        wsRef.current = null;
        if (ws) {
            ws.onclose = null;
            ws.onmessage = null;
            ws.onerror = null;
            if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING) {
                ws.close();
            }
        }
        const sessionId = sessionRef.current;
        sessionRef.current = null;
        if (sessionId) {
            try {
                await closeOpsTerminalSession(sessionId);
            } catch {
                // 404（已被回收）/ 401（切换账号）都不影响本地状态
            }
        }
        setStatus((prev) => (prev === 'failed' ? prev : 'disconnected'));
    }, []);

    const handleServerText = useCallback((term: Terminal, raw: string) => {
        let frame: ServerFrame | null = null;
        try {
            frame = JSON.parse(raw) as ServerFrame;
        } catch {
            frame = null;
        }
        if (frame?.type === 'exit') {
            const code = typeof frame.code === 'number' ? frame.code : null;
            setExitCode(code);
            term.writeln(`\r\n\x1b[90m[远端进程已退出${code === null ? '' : `，退出码 ${code}`}]\x1b[0m`);
            return;
        }
        if (frame?.type === 'closed') {
            term.writeln(`\r\n\x1b[90m[会话已关闭：${frame.reason ?? '未知原因'}]\x1b[0m`);
            setStatus('disconnected');
            return;
        }
        if (frame?.type === 'pong') return;
        term.write(raw);
    }, []);


    /** 连接容器 / Pod */
    const connect = useCallback(
        async (connectionId: string, target: OpsTerminalTarget, shell?: string | null) => {
            await disconnect();
            await Promise.race([
                readyRef.current ?? Promise.resolve(),
                new Promise<void>((resolve) => {
                    window.setTimeout(resolve, 5000);
                }),
            ]);
            const term = termRef.current;
            if (!term) {
                setStatus('failed');
                setError('终端尚未初始化，请稍后重试');
                return;
            }
            setStatus('connecting');
            setError(null);
            setExitCode(null);
            term.reset();
            fitRef.current?.fit();

            try {
                const session = await openOpsTerminalSession(
                    connectionId,
                    target,
                    shell,
                    term.cols || 120,
                    term.rows || 32,
                );
                sessionRef.current = session.session_id;
                const socket = new WebSocket(
                    buildOpsTerminalWsUrl(session.session_id, session.ticket),
                );
                socket.binaryType = 'arraybuffer';
                wsRef.current = socket;

                socket.onopen = () => {
                    setStatus('active');
                    sendResize();
                };
                socket.onmessage = (event: MessageEvent) => {
                    if (typeof event.data === 'string') {
                        handleServerText(term, event.data);
                        return;
                    }
                    if (event.data instanceof ArrayBuffer) {
                        term.write(new Uint8Array(event.data));
                    }
                };
                socket.onerror = () => {
                    setError('终端连接异常，请重新连接');
                };
                socket.onclose = () => {
                    if (wsRef.current === socket) {
                        wsRef.current = null;
                    }
                    setStatus((prev) => (prev === 'failed' ? prev : 'disconnected'));
                };
            } catch (err) {
                const message = errorMessage(err);
                setStatus('failed');
                setError(message);
                term.writeln(`\r\n\x1b[31m[连接失败] ${message}\x1b[0m`);
            }
        },
        [disconnect, handleServerText, sendResize],
    );

    // 卸载时断开（避免后端连接泄漏）
    useEffect(
        () => () => {
            const ws = wsRef.current;
            wsRef.current = null;
            if (ws) {
                ws.onclose = null;
                ws.close();
            }
            const sessionId = sessionRef.current;
            sessionRef.current = null;
            if (sessionId) {
                void closeOpsTerminalSession(sessionId).catch(() => undefined);
            }
        },
        [],
    );

    const getSelection = useCallback(() => termRef.current?.getSelection() ?? '', []);

    return { status, error, exitCode, connect, disconnect, getSelection };
}

