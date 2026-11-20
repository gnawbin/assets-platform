/**
 * 容器日志 / 性能 / 终端 service 路由契约测试
 *
 * 1. 6 条命令都在 `COMMAND_ROUTE_MAP` 中登记，method / path 与后端容器路由逐字一致；
 * 2. GET 的占位符与 service 参数同名（connectionId / container / namespace / pod）；
 * 3. `parseOpsResourceTarget` 与后端 `ResourceKey::uid()` 的规则一致；
 * 4. `buildOpsTerminalWsUrl` 只把一次性票据放查询串，并按服务端地址切换 ws / wss。
 */

import { COMMAND_ROUTE_MAP } from '@/utils/adapters/httpAdapter';
import {
    buildOpsTerminalWsUrl,
    closeOpsTerminalSession,
    getOpsContainerLogs,
    getOpsContainerStats,
    getOpsPodLogs,
    getOpsPodMetrics,
    openOpsTerminalSession,
    parseOpsResourceTarget,
} from '@/services/opsService';

const API_BASE = 'http://test.local/api';
const API_ORIGIN = 'http://test.local';

function okResponse(data: unknown): Response {
    return {
        ok: true,
        status: 200,
        statusText: 'OK',
        json: jest.fn().mockResolvedValue({ code: 0, message: 'ok', data }),
        text: jest.fn().mockResolvedValue(''),
    } as unknown as Response;
}

function callAt(index = 0): [string, RequestInit] {
    const calls = (global.fetch as jest.Mock).mock.calls;
    return [calls[index][0], calls[index][1] as RequestInit];
}

type ApiWindow = Window & { __API_BASE__?: unknown };

describe('容器日志 / 性能 / 终端与 COMMAND_ROUTE_MAP 契约', () => {
    beforeEach(() => {
        (window as ApiWindow).__API_BASE__ = API_ORIGIN;
        localStorage.clear();
        (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(okResponse({}));
    });

    afterEach(() => {
        delete (window as ApiWindow).__API_BASE__;
    });

    it('6 条命令都已登记 HTTP 路由', () => {
        expect(COMMAND_ROUTE_MAP.get_ops_container_logs).toEqual({
            method: 'GET',
            path: '/api/ops/connections/{connectionId}/containers/{container}/logs',
        });
        expect(COMMAND_ROUTE_MAP.get_ops_container_stats).toEqual({
            method: 'GET',
            path: '/api/ops/connections/{connectionId}/containers/{container}/stats',
        });
        expect(COMMAND_ROUTE_MAP.get_ops_pod_logs).toEqual({
            method: 'GET',
            path: '/api/ops/connections/{connectionId}/namespaces/{namespace}/pods/{pod}/logs',
        });
        expect(COMMAND_ROUTE_MAP.get_ops_pod_metrics).toEqual({
            method: 'GET',
            path: '/api/ops/connections/{connectionId}/namespaces/{namespace}/pods/{pod}/metrics',
        });
        expect(COMMAND_ROUTE_MAP.open_ops_terminal_session).toEqual({
            method: 'POST',
            path: '/api/ops/terminal/sessions',
        });
        expect(COMMAND_ROUTE_MAP.close_ops_terminal_session).toEqual({
            method: 'DELETE',
            path: '/api/ops/terminal/sessions/{sessionId}',
        });
    });

    it('容器日志：占位符进路径，其余参数进 query', async () => {
        await getOpsContainerLogs('10', 'web-1', 500, true);

        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/ops/connections/10/containers/web-1/logs?lines=500&timestamps=true`);
        expect(init.method).toBe('GET');
        expect(init.body).toBeUndefined();
    });

    it('Pod 日志：namespace / pod 进路径，container / previous 进 query', async () => {
        await getOpsPodLogs('20', 'prod', 'web-0', 'web', true, 200, false);

        const [url] = callAt();
        expect(url).toBe(
            `${API_BASE}/ops/connections/20/namespaces/prod/pods/web-0/logs?container=web&previous=true&lines=200&timestamps=false`,
        );
    });

    it('容器性能：twoSamples 进 query', async () => {
        await getOpsContainerStats('10', 'web-1', true);
        const [url] = callAt();
        expect(url).toBe(`${API_BASE}/ops/connections/10/containers/web-1/stats?twoSamples=true`);
    });

    it('Pod 指标：connectionId / namespace / pod 全部进路径', async () => {
        await getOpsPodMetrics('20', 'prod', 'web-0');
        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/ops/connections/20/namespaces/prod/pods/web-0/metrics`);
        expect(init.method).toBe('GET');
    });

    it('打开终端：请求体为 camelCase，target 按 kind 判别', async () => {
        await openOpsTerminalSession(
            '10',
            { kind: 'docker', container: 'web-1' },
            '/bin/sh',
            120,
            32,
        );

        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/ops/terminal/sessions`);
        expect(init.method).toBe('POST');
        expect(JSON.parse(String(init.body))).toEqual({
            connectionId: '10',
            target: { kind: 'docker', container: 'web-1' },
            shell: '/bin/sh',
            cols: 120,
            rows: 32,
        });
    });

    it('关闭终端：sessionId 进路径，DELETE 不带 body', async () => {
        await closeOpsTerminalSession('123');
        const [url, init] = callAt();
        expect(url).toBe(`${API_BASE}/ops/terminal/sessions/123`);
        expect(init.method).toBe('DELETE');
        expect(init.body).toBeUndefined();
    });
});

describe('parseOpsResourceTarget', () => {
    it('解析 Docker 容器 uid', () => {
        expect(parseOpsResourceTarget('docker://local/container/abc123')).toEqual({
            provider: 'docker',
            endpoint: 'local',
            containerId: 'abc123',
        });
    });

    it('解析远程 Docker 端点', () => {
        expect(parseOpsResourceTarget('docker://10.0.0.5:2375/container/abc')).toEqual({
            provider: 'docker',
            endpoint: '10.0.0.5:2375',
            containerId: 'abc',
        });
    });

    it('解析 K8s Pod uid', () => {
        expect(parseOpsResourceTarget('k8s://10.0.0.10:6443/pod/prod/web-0')).toEqual({
            provider: 'k8s',
            endpoint: '10.0.0.10:6443',
            namespace: 'prod',
            pod: 'web-0',
        });
    });

    it('非法 uid 返回 null', () => {
        expect(parseOpsResourceTarget('docker://local/image/abc')).toBeNull();
        expect(parseOpsResourceTarget('k8s://host/pod/prod')).toBeNull();
        expect(parseOpsResourceTarget('')).toBeNull();
    });
});

describe('buildOpsTerminalWsUrl', () => {
    it('ws + 票据编码', () => {
        (window as ApiWindow).__API_BASE__ = API_ORIGIN;
        expect(buildOpsTerminalWsUrl('123', 'a b/c')).toBe(
            'ws://test.local/api/ops/terminal/sessions/123/ws?ticket=a%20b%2Fc',
        );
        delete (window as ApiWindow).__API_BASE__;
    });

    it('https → wss', () => {
        (window as ApiWindow).__API_BASE__ = 'https://demo.example.com';
        expect(buildOpsTerminalWsUrl('9', 'ticket')).toBe(
            'wss://demo.example.com/api/ops/terminal/sessions/9/ws?ticket=ticket',
        );
        delete (window as ApiWindow).__API_BASE__;
    });
});

