/**
 * sshService ↔ 后端路由契约测试（运维终端 T12）
 *
 * 断言三件事：
 * 1. 10 个命令都在 `COMMAND_ROUTE_MAP` 中登记，且方法 / 路径与
 *    `crates/api/src/ssh_routes.rs`（文档 §5.1）逐字一致；
 * 2. 请求体键名保持后端 serde 的 snake_case（`target_id` / `auth_type` /
 *    `private_key_pem`…），避免前端 camelCase 化后 400；
 * 3. `buildSshWsUrl` 只把一次性票据放查询串（JWT 绝不进 URL），并按
 *    服务端地址协议在 ws / wss 之间切换。
 */

import { COMMAND_ROUTE_MAP } from '@/utils/adapters/httpAdapter';
import {
    buildSshWsUrl,
    closeSshSession,
    createSshTarget,
    deleteSshTarget,
    getSshTarget,
    getSshSessions,
    getSshTargets,
    openSshSession,
    probeDraftSshTarget,
    probeSshTarget,
    updateSshTarget,
} from '@/services/sshService';

const API_BASE = 'http://test.local/api';
const API_ORIGIN = 'http://test.local';

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

type ApiWindow = Window & { __API_BASE__?: unknown };

describe('sshService 与 COMMAND_ROUTE_MAP 契约', () => {
    beforeEach(() => {
        (window as ApiWindow).__API_BASE__ = API_ORIGIN;
        localStorage.clear();
        (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(okResponse({}));
    });

    afterEach(() => {
        delete (window as ApiWindow).__API_BASE__;
    });

    it('10 个命令都已登记 HTTP 路由（与后端 §5.1 一致）', () => {
        expect(COMMAND_ROUTE_MAP.get_ssh_targets).toEqual({ method: 'GET', path: '/api/ops/ssh/targets' });
        expect(COMMAND_ROUTE_MAP.get_ssh_target).toEqual({ method: 'GET', path: '/api/ops/ssh/targets/{id}' });
        expect(COMMAND_ROUTE_MAP.create_ssh_target).toEqual({ method: 'POST', path: '/api/ops/ssh/targets' });
        expect(COMMAND_ROUTE_MAP.update_ssh_target).toEqual({ method: 'PUT', path: '/api/ops/ssh/targets/{id}' });
        expect(COMMAND_ROUTE_MAP.delete_ssh_target).toEqual({ method: 'DELETE', path: '/api/ops/ssh/targets/{id}' });
        expect(COMMAND_ROUTE_MAP.probe_ssh_target).toEqual({
            method: 'POST',
            path: '/api/ops/ssh/targets/{id}/probe',
        });
        expect(COMMAND_ROUTE_MAP.probe_draft_ssh_target).toEqual({
            method: 'POST',
            path: '/api/ops/ssh/targets/test',
        });
        expect(COMMAND_ROUTE_MAP.get_ssh_sessions).toEqual({ method: 'GET', path: '/api/ops/ssh/sessions' });
        expect(COMMAND_ROUTE_MAP.open_ssh_session).toEqual({ method: 'POST', path: '/api/ops/ssh/sessions' });
        expect(COMMAND_ROUTE_MAP.close_ssh_session).toEqual({
            method: 'DELETE',
            path: '/api/ops/ssh/sessions/{sessionId}',
        });
    });

    it('主机列表：GET /api/ops/ssh/targets', async () => {
        await getSshTargets();

        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/ops/ssh/targets`);
        expect(init.method).toBe('GET');
    });

    it('主机详情：id 只进路径', async () => {
        await getSshTarget('t-1');

        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/ops/ssh/targets/t-1`);
        expect(init.method).toBe('GET');
        expect(init.body).toBeUndefined();
    });

    it('新建主机：请求体保持 snake_case', async () => {
        await createSshTarget({
            name: '跳板机',
            host: '10.0.0.8',
            port: 22,
            username: 'ops',
            auth_type: 'private_key',
            private_key_pem: '-----BEGIN OPENSSH PRIVATE KEY-----',
            passphrase: null,
            strict_host_key_check: true,
            host_key_fingerprint: 'SHA256:abc',
            connect_timeout_secs: 15,
        });

        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/ops/ssh/targets`);
        expect(init.method).toBe('POST');
        expect(JSON.parse(String(init.body))).toEqual({
            name: '跳板机',
            host: '10.0.0.8',
            port: 22,
            username: 'ops',
            auth_type: 'private_key',
            private_key_pem: '-----BEGIN OPENSSH PRIVATE KEY-----',
            passphrase: null,
            strict_host_key_check: true,
            host_key_fingerprint: 'SHA256:abc',
            connect_timeout_secs: 15,
        });
    });

    it('编辑主机：id 进路径，其余字段进 body（缺省凭证 = 沿用原值）', async () => {
        await updateSshTarget('t-2', { name: '改名', host: '10.0.0.9', username: 'ops' });

        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/ops/ssh/targets/t-2`);
        expect(init.method).toBe('PUT');
        // id 是路径占位符，不得重复出现在 body 里；凭证字段缺省即不发送
        expect(JSON.parse(String(init.body))).toEqual({
            name: '改名',
            host: '10.0.0.9',
            username: 'ops',
        });
    });

    it('删除主机：DELETE /api/ops/ssh/targets/{id}（无 body）', async () => {
        await deleteSshTarget('t-3');

        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/ops/ssh/targets/t-3`);
        expect(init.method).toBe('DELETE');
        expect(init.body).toBeUndefined();
    });

    it('已保存主机的指纹探测：POST /targets/{id}/probe', async () => {
        await probeSshTarget('t-4');

        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/ops/ssh/targets/t-4/probe`);
        expect(init.method).toBe('POST');
        // id 已作为路径参数被剥离，body 里没有额外字段
        expect(init.body).toBeUndefined();
    });

    it('未保存主机的指纹探测：POST /targets/test', async () => {
        await probeDraftSshTarget({ host: '10.0.0.10', port: 2222, username: 'ops', auth_type: 'password' });

        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/ops/ssh/targets/test`);
        expect(init.method).toBe('POST');
        expect(JSON.parse(String(init.body))).toEqual({
            host: '10.0.0.10',
            port: 2222,
            username: 'ops',
            auth_type: 'password',
        });
    });

    it('会话列表：GET /api/ops/ssh/sessions', async () => {
        await getSshSessions();

        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/ops/ssh/sessions`);
        expect(init.method).toBe('GET');
    });

    it('打开会话：POST /api/ops/ssh/sessions（target_id / cols / rows）', async () => {
        await openSshSession('t-5', 120, 32);

        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/ops/ssh/sessions`);
        expect(init.method).toBe('POST');
        expect(JSON.parse(String(init.body))).toEqual({ target_id: 't-5', cols: 120, rows: 32 });
    });

    it('关闭会话：DELETE /api/ops/ssh/sessions/{sessionId}（无 body）', async () => {
        await closeSshSession('s-1');

        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/ops/ssh/sessions/s-1`);
        expect(init.method).toBe('DELETE');
        expect(init.body).toBeUndefined();
    });
});

describe('buildSshWsUrl', () => {
    afterEach(() => {
        delete (window as ApiWindow).__API_BASE__;
    });

    it('https 服务端 → wss，且 sessionId / ticket 编码后只进查询串', () => {
        (window as ApiWindow).__API_BASE__ = 'https://ops.example.com';

        const url = buildSshWsUrl('s/1', 'tk+en=');

        expect(url).toBe('wss://ops.example.com/api/ops/ssh/sessions/s%2F1/ws?ticket=tk%2Ben%3D');
    });

    it('http 服务端 → ws', () => {
        (window as ApiWindow).__API_BASE__ = 'http://10.0.0.2:3001';

        expect(buildSshWsUrl('s-2', 'abc')).toBe(
            'ws://10.0.0.2:3001/api/ops/ssh/sessions/s-2/ws?ticket=abc',
        );
    });
});
