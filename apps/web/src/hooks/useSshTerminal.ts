'use client';

/**
 * xterm.js 终端 Hook：Bearer 换票据 → WS 升级 → 全双工字节流
 *
 * 状态机：idle → connecting → active → disconnected | failed
 * - 输入走 Binary 帧（`term.onData` → `TextEncoder`），避开 Text/JSON 控制帧；
 * - 输出走 Binary 帧直接 `term.write`（保留 ANSI 转义序列）；
 * - 窗口 resize 用 ResizeObserver + 120ms 防抖，发 `{type:'resize'}`；
 * - 卸载 / 断开时先关 socket，再 `closeSshSession`（后端幂等，404 可忽略）。
 */

import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';
import type { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';

import {
    buildSshWsUrl,
    closeSshSession,
    openSshSession,
} from '@/services/sshService';

/** 终端连接状态 */
export type SshTerminalStatus = 'idle' | 'connecting' | 'active' | 'disconnected' | 'failed';

/** 服务端下行控制帧 */
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

export interface UseSshTerminalResult {
    /** 当前状态 */
    status: SshTerminalStatus;
    /** 失败原因（status === 'failed' 时有值） */
    error: string | null;
    /** 远端退出码（收到 exit 帧后写入） */
    exitCode: number | null;
    /** 连接指定主机（会先断开已有会话） */
    connect: (targetId: string) => Promise<void>;
    /**
     * 读取选中文本
     * @returns 终端当前选中的文本（未选中为空串）
     */
    getSelection: () => string;
    /** 主动断开（幂等） */
    disconnect: () => Promise<void>;
}

export function useSshTerminal(
    containerRef: RefObject<HTMLDivElement | null>,
): UseSshTerminalResult {
    const termRef = useRef<Terminal | null>(null);
    const fitRef = useRef<FitAddon | null>(null);
    const wsRef = useRef<WebSocket | null>(null);
    const sessionRef = useRef<string | null>(null);
    const resizeTimerRef = useRef<number | null>(null);
    const observerRef = useRef<(() => void) | null>(null);
    /** 终端创建完成的信号（connect 早于 xterm 动态加载时等待） */
    const readyRef = useRef<Promise<void> | null>(null);
    const resolveReadyRef = useRef<(() => void) | null>(null);
    if (readyRef.current === null) {
        readyRef.current = new Promise<void>((resolve) => {
            resolveReadyRef.current = resolve;
        });
    }

    const [status, setStatus] = useState<SshTerminalStatus>('idle');
    const [error, setError] = useState<string | null>(null);
    const [exitCode, setExitCode] = useState<number | null>(null);

    /** 通知 PTY 当前窗口大小（fit 之后调用） */
    const sendResize = useCallback(() => {
        const term = termRef.current;
        const ws = wsRef.current;
        if (!term || !ws || ws.readyState !== WebSocket.OPEN) return;
        ws.send(JSON.stringify({ type: 'resize', cols: term.cols, rows: term.rows }));
    }, []);

    // 创建终端：只在挂载时执行一次（xterm 动态导入，避免 SSR 触碰 window）
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
            // 卸载时释放等待者，避免 connect 悬挂
            resolveReadyRef.current?.();
            resolveReadyRef.current = null;
        };
    }, [containerRef, sendResize]);

    /** 关闭连接：先停 socket，再让后端回收会话 */
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
                await closeSshSession(sessionId);
            } catch {
                // 404（已被回收）/ 401（切换账号）都不影响本地状态
            }
        }
        setStatus((prev) => (prev === 'failed' ? prev : 'disconnected'));
    }, []);

    /** 解析服务端 Text 帧（exit / closed / pong），其余按原样写入 */
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
            term.writeln(
                `\r\n\x1b[90m[远端进程已退出${code === null ? '' : `，退出码 ${code}`}]\x1b[0m`,
            );
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

    /** 连接主机 */
    const connect = useCallback(
        async (targetId: string) => {
            await disconnect();
            // 等待 xterm 动态加载完成（最多 5s，超时按失败处理）
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
                const session = await openSshSession(
                    targetId,
                    term.cols || 120,
                    term.rows || 32,
                );
                sessionRef.current = session.session_id;
                const socket = new WebSocket(
                    buildSshWsUrl(session.session_id, session.ticket),
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
                void closeSshSession(sessionId).catch(() => undefined);
            }
        },
        [],
    );

    /** 读取当前选中的文本（复制按钮用） */
    const getSelection = useCallback(() => termRef.current?.getSelection() ?? '', []);

    return { status, error, exitCode, connect, disconnect, getSelection };
}
