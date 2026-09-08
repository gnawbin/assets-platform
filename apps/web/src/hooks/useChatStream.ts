/**
 * SSE 流式对话 Hook
 *
 * 通过 HTTP SSE 端点实现逐字输出效果。
 * 使用方式：
 *   const chatStream = useChatStream();
 *   chatStream.send({ userId, question, convId, attachments });
 *
 * 后端端点：POST /api/chat/stream（JSON body，支持附件）
 * 后端地址由 NEXT_PUBLIC_API_BASE_URL 环境变量控制，默认 http://localhost:3001
 */

import { useState, useRef, useCallback } from 'react';
import type { ChatAttachment } from '@/services/conversationService';

export interface StreamResult {
    convId?: string;
    citedAssets?: Array<{ id: string; title: string; okfType: string }>;
    usage?: { inputTokens: number; outputTokens: number; totalTokens: number; cost: number };
    error?: string;
}

export interface StreamCallbacks {
    /** 逐字接收 LLM 生成的 token */
    onToken?: (text: string) => void;
    /** 流结束 */
    onDone?: (result: StreamResult) => void;
    /** 错误 */
    onError?: (error: string) => void;
}

const BASE_URL = process.env.NEXT_PUBLIC_API_BASE_URL || 'http://localhost:3001';

/**
 * SSE 流式对话 Hook
 *
 * @example
 * ```tsx
 * const chatStream = useChatStream({
 *     onToken: (text) => console.log('收到 token:', text),
 *     onDone: (result) => console.log('完成:', result),
 *     onError: (err) => console.error('错误:', err),
 * });
 *
 * // 发送问题（不等待，逐字回调）
 * chatStream.send({ userId: '123', question: '什么是折旧？', attachments: [...] });
 * ```
 */
export function useChatStream(callbacks?: StreamCallbacks) {
    const [streaming, setStreaming] = useState(false);
    const abortRef = useRef<AbortController | null>(null);

    const send = useCallback(
        async (params: {
            userId: string;
            question: string;
            convId?: string;
            attachments?: ChatAttachment[];
        }) => {
            // 取消之前的流
            if (abortRef.current) {
                abortRef.current.abort();
            }

            const controller = new AbortController();
            abortRef.current = controller;
            setStreaming(true);

            try {
                const url = `${BASE_URL}/api/chat/stream`;
                const response = await fetch(url, {
                    method: 'POST',
                    signal: controller.signal,
                    headers: {
                        Accept: 'text/event-stream',
                        'Content-Type': 'application/json',
                    },
                    body: JSON.stringify({
                        user_id: params.userId,
                        question: params.question,
                        conv_id: params.convId || null,
                        attachments: params.attachments || [],
                    }),
                });

                if (!response.ok) {
                    throw new Error(`SSE 连接失败: ${response.status} ${response.statusText}`);
                }

                const reader = response.body?.getReader();
                if (!reader) {
                    throw new Error('SSE 响应体为空');
                }

                const decoder = new TextDecoder();
                let buffer = '';

                // 读取 SSE 事件流
                while (true) {
                    const { done, value } = await reader.read();
                    if (done) break;

                    buffer += decoder.decode(value, { stream: true });

                    // 解析 SSE 格式事件
                    const events = buffer.split('\n\n');
                    buffer = events.pop() || ''; // 最后一个可能不完整

                    for (const event of events) {
                        const lines = event.split('\n');
                        let eventType = '';
                        let data = '';

                        for (const line of lines) {
                            if (line.startsWith('event: ')) {
                                eventType = line.slice(7).trim();
                            } else if (line.startsWith('data: ')) {
                                data = line.slice(6).trim();
                            }
                        }

                        if (!eventType || !data) continue;

                        try {
                            const payload = JSON.parse(data);

                            switch (eventType) {
                                case 'token':
                                    callbacks?.onToken?.(payload.text || '');
                                    break;

                                case 'done':
                                    callbacks?.onDone?.({
                                        convId: payload.convId,
                                        citedAssets: payload.citedAssets,
                                        usage: payload.usage,
                                    });
                                    setStreaming(false);
                                    return;

                                case 'error':
                                    callbacks?.onError?.(payload.message || '未知错误');
                                    setStreaming(false);
                                    return;
                            }
                        } catch {
                            // JSON 解析失败，忽略该事件
                        }
                    }
                }
            } catch (err: any) {
                if (err.name === 'AbortError') {
                    // 主动取消，不触发 error
                    return;
                }
                callbacks?.onError?.(err.message || 'SSE 连接异常');
            } finally {
                setStreaming(false);
                abortRef.current = null;
            }
        },
        [callbacks],
    );

    /** 取消当前流 */
    const cancel = useCallback(() => {
        if (abortRef.current) {
            abortRef.current.abort();
            abortRef.current = null;
        }
        setStreaming(false);
    }, []);

    return { send, cancel, streaming };
}



