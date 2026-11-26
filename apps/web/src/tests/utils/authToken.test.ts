/**
 * authToken 工具单元测试
 *
 * 覆盖：令牌读取（未登录 / 空串 / 正常）、认证头构造。
 * 该模块被 `uploadService`（/api/upload/*）与 `useChatStream`（SSE）复用。
 */

import { AUTH_TOKEN_KEY, getAuthToken, withAuthHeaders } from '@/utils/authToken';

describe('authToken', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('键名应与 authStore / httpAdapter 一致', () => {
    expect(AUTH_TOKEN_KEY).toBe('auth_token');
  });

  it('未登录时应返回 null', () => {
    expect(getAuthToken()).toBeNull();
  });

  it('应读取登录后写入的令牌', () => {
    localStorage.setItem(AUTH_TOKEN_KEY, 'token-abc');
    expect(getAuthToken()).toBe('token-abc');
  });

  it('空字符串按未登录处理', () => {
    localStorage.setItem(AUTH_TOKEN_KEY, '');
    expect(getAuthToken()).toBeNull();
  });

  it('withAuthHeaders 应附加 Bearer 令牌并保留原有头', () => {
    localStorage.setItem(AUTH_TOKEN_KEY, 'token-abc');

    expect(withAuthHeaders({ 'Content-Type': 'application/json' })).toEqual({
      'Content-Type': 'application/json',
      Authorization: 'Bearer token-abc',
    });
  });

  it('withAuthHeaders 未登录时应原样返回', () => {
    expect(withAuthHeaders()).toEqual({});
    expect(withAuthHeaders({ Accept: 'text/event-stream' })).toEqual({
      Accept: 'text/event-stream',
    });
  });
});
