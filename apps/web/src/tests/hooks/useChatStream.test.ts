/**
 * useChatStream Hook 单元测试
 *
 * 覆盖：
 * - 已登录时 SSE 请求携带 Bearer 令牌（/api/chat/stream 已纳入 auth_middleware）
 * - 未登录时不附加 Authorization 头
 * - BASE_URL 归一化（避免拼出 /api/api/chat/stream）
 * - SSE 事件解析（token / done）
 */

import { renderHook, act } from '@testing-library/react';
import { TextDecoder as NodeTextDecoder, TextEncoder as NodeTextEncoder } from 'node:util';

import { useChatStream } from '@/hooks/useChatStream';

// jsdom 环境未暴露 TextDecoder/TextEncoder，用 Node 实现补齐（Hook 内部依赖）
if (typeof globalThis.TextDecoder === 'undefined') {
  globalThis.TextDecoder = NodeTextDecoder as unknown as typeof globalThis.TextDecoder;
}
if (typeof globalThis.TextEncoder === 'undefined') {
  globalThis.TextEncoder = NodeTextEncoder as unknown as typeof globalThis.TextEncoder;
}

/** 构造一个只读一次的 SSE 响应体 */
function createSseResponse(chunks: string[]): Response {
  let index = 0;
  const reader = {
    read: jest.fn(async () => {
      if (index < chunks.length) {
        return { done: false, value: new TextEncoder().encode(chunks[index++]) };
      }
      return { done: true, value: undefined };
    }),
  };
  return {
    ok: true,
    status: 200,
    statusText: 'OK',
    body: { getReader: () => reader },
  } as unknown as Response;
}

describe('useChatStream', () => {
  it('已登录时应携带 Bearer 令牌并正确解析 SSE 事件', async () => {
    localStorage.setItem('auth_token', 'token-abc');

    const fetchMock = jest.fn().mockResolvedValue(
      createSseResponse([
        'event: token\ndata: {"text":"你好"}\n\n',
        'event: done\ndata: {"convId":"conv-1","usage":{"totalTokens":3}}\n\n',
      ])
    );
    global.fetch = fetchMock as unknown as typeof fetch;

    const onToken = jest.fn();
    const onDone = jest.fn();
    const onError = jest.fn();
    const { result } = renderHook(() => useChatStream({ onToken, onDone, onError }));

    await act(async () => {
      await result.current.send({ userId: '1', question: '什么是折旧？' });
    });

    // 若解析链路异常，先暴露错误信息，便于定位
    expect(onError).not.toHaveBeenCalled();

    const [url, init] = fetchMock.mock.calls[0];

    // 端点拼接：BASE_URL 去掉尾部 /api 后再拼 /api/chat/stream
    expect(url).toContain('/api/chat/stream');
    expect(url).not.toContain('/api/api/');

    expect(init.method).toBe('POST');
    expect(init.headers['Content-Type']).toBe('application/json');
    expect(init.headers.Accept).toBe('text/event-stream');
    expect(init.headers.Authorization).toBe('Bearer token-abc');

    expect(onToken).toHaveBeenCalledWith('你好');
    expect(onDone).toHaveBeenCalledWith(expect.objectContaining({ convId: 'conv-1' }));
  });

  it('未登录时不应附加 Authorization 头', async () => {
    const fetchMock = jest
      .fn()
      .mockResolvedValue(createSseResponse(['event: done\ndata: {}\n\n']));
    global.fetch = fetchMock as unknown as typeof fetch;

    const { result } = renderHook(() => useChatStream());

    await act(async () => {
      await result.current.send({ userId: '1', question: 'hi' });
    });

    const [, init] = fetchMock.mock.calls[0];
    expect(init.headers.Authorization).toBeUndefined();
  });

  it('请求失败时应回调 onError', async () => {
    const fetchMock = jest.fn().mockResolvedValue({
      ok: false,
      status: 401,
      statusText: 'Unauthorized',
    } as unknown as Response);
    global.fetch = fetchMock as unknown as typeof fetch;

    const onError = jest.fn();
    const { result } = renderHook(() => useChatStream({ onError }));

    await act(async () => {
      await result.current.send({ userId: '1', question: 'hi' });
    });

    expect(onError).toHaveBeenCalled();
    expect(onError.mock.calls[0][0]).toContain('401');
  });
});
