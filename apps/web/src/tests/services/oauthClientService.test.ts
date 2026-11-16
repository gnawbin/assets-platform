/**
 * oauthClientService ↔ 后端路由契约测试（OAuth2.1 清单 C-6）
 *
 * 断言两件事：
 * 1. 4 个命令都在 `COMMAND_ROUTE_MAP` 中登记，且方法 / 路径与
 *    `crates/api/src/oauth_routes.rs` 一致；
 * 2. 请求体键名保持后端 serde 的 snake_case（`client_name` / `redirect_uris` /
 *    `allowed_scopes` / `allow_scopes`…），避免前端 camelCase 化后 OAuth 管理页 400。
 *
 * 基址通过 `window.__API_BASE__` 注入，顺带守住「不拼出 /api/api」。
 */

import { COMMAND_ROUTE_MAP } from '@/utils/adapters/httpAdapter';
import {
    createOauthClient,
    deleteOauthClient,
    getOauthClients,
    updateOauthClient,
} from '@/services/oauthClientService';

const API_BASE = 'http://test.local/api';

/** 构造成功的 `ApiResponse<T>` 响应 */
function okResponse(data: unknown): Response {
    return {
        ok: true,
        status: 200,
        statusText: 'OK',
        json: jest.fn().mockResolvedValue({ code: 0, message: 'ok', data }),
        text: jest.fn().mockResolvedValue(''),
    } as unknown as Response;
}

/** 取第 n 次 fetch 调用的 (url, init) */
function callAt(index = 0): [string, RequestInit] {
    const calls = (global.fetch as jest.Mock).mock.calls;
    return [calls[index][0], calls[index][1] as RequestInit];
}

describe('oauthClientService 与 COMMAND_ROUTE_MAP 契约', () => {
    beforeEach(() => {
        (window as Window & { __API_BASE__?: unknown }).__API_BASE__ = 'http://test.local';
        localStorage.clear();
        (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(okResponse({}));
    });

    afterEach(() => {
        delete (window as Window & { __API_BASE__?: unknown }).__API_BASE__;
    });

    it('4 个命令都已登记 HTTP 路由（C-6）', () => {
        expect(COMMAND_ROUTE_MAP.get_oauth_clients).toEqual({
            method: 'GET',
            path: '/api/oauth-clients',
        });
        expect(COMMAND_ROUTE_MAP.create_oauth_client).toEqual({
            method: 'POST',
            path: '/api/oauth-clients',
        });
        expect(COMMAND_ROUTE_MAP.update_oauth_client).toEqual({
            method: 'PUT',
            path: '/api/oauth-clients/{id}',
        });
        expect(COMMAND_ROUTE_MAP.delete_oauth_client).toEqual({
            method: 'DELETE',
            path: '/api/oauth-clients/{id}',
        });
    });

    it('列表：GET /api/oauth-clients', async () => {
        await getOauthClients();

        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/oauth-clients`);
        expect(init.method).toBe('GET');
    });

    it('创建：请求体保持 snake_case', async () => {
        await createOauthClient({
            client_name: '报表同步',
            client_type: 1,
            redirect_uris: ['https://partner.example.com/cb'],
            allowed_scopes: ['asset:hardware:list'],
            access_token_ttl: 900,
        });

        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/oauth-clients`);
        expect(init.method).toBe('POST');
        expect(JSON.parse(String(init.body))).toEqual({
            client_name: '报表同步',
            client_type: 1,
            redirect_uris: ['https://partner.example.com/cb'],
            allowed_scopes: ['asset:hardware:list'],
            access_token_ttl: 900,
        });
    });

    it('更新：id 进路径，其余字段进 body（snake_case）', async () => {
        await updateOauthClient('990001', { client_name: '改名', enable: false });

        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/oauth-clients/990001`);
        expect(init.method).toBe('PUT');
        // id 是路径占位符，不得重复出现在 body 里
        expect(JSON.parse(String(init.body))).toEqual({ client_name: '改名', enable: false });
    });

    it('删除：DELETE /api/oauth-clients/{id}（id 只进路径）', async () => {
        await deleteOauthClient('990002');

        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/oauth-clients/990002`);
        expect(init.method).toBe('DELETE');
        // DELETE 不发送 body；id 已作为路径参数被剥离
        expect(init.body).toBeUndefined();
    });
});
