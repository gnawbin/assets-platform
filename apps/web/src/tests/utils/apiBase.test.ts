/**
 * apiBase 基址解析单元测试
 *
 * 覆盖前端「唯一基址来源」的核心行为：
 * - `normalizeApiBase` 规范化（裸主机、尾斜杠、已带 `/api`、非法输入）
 * - `getApiBase()` 优先级（`window.__API_BASE__` > localStorage > 同源兜底）
 * - `getApiBaseSource()` / `isFallbackBase()` / `describeApiBase()` 来源判定（Gate 的判据）
 * - `getApiOrigin()` 去 `/api` 后缀（保证 `httpAdapter` 不拼出 `/api/api`）
 * - `isInsecureRemote()` / `setApiBase` 的明文 http 校验与会话清理
 * - `probeServer()` 对 `/healthz` 的探活分类
 */

import {
    API_BASE_STORAGE_KEY,
    InsecureTransportError,
    clearApiBase,
    describeApiBase,
    getApiBase,
    getApiBaseSource,
    getApiOrigin,
    hasServerConfigured,
    isFallbackBase,
    isInsecureRemote,
    normalizeApiBase,
    probeServer,
    setApiBase,
} from '@/utils/apiBase';

type WindowWithApiBase = Window & { __API_BASE__?: unknown };

function setInjectedBase(value: unknown): void {
    (window as WindowWithApiBase).__API_BASE__ = value;
}

describe('normalizeApiBase', () => {
    it('裸主机补协议并追加 /api', () => {
        expect(normalizeApiBase('192.168.1.10:3001')).toBe('http://192.168.1.10:3001/api');
    });

    it('去掉末尾斜杠后追加 /api', () => {
        expect(normalizeApiBase('https://demo.example.com/')).toBe('https://demo.example.com/api');
    });

    it('已带 /api 时原样保留（大小写不敏感）', () => {
        expect(normalizeApiBase('https://demo.example.com/api/')).toBe('https://demo.example.com/api');
        expect(normalizeApiBase('http://127.0.0.1:3001/API')).toBe('http://127.0.0.1:3001/API');
    });

    it('自定义前缀路径保留并追加 /api', () => {
        expect(normalizeApiBase('https://demo.example.com/v2')).toBe('https://demo.example.com/v2/api');
    });

    it('去除首尾空白', () => {
        expect(normalizeApiBase('  http://127.0.0.1:3001  ')).toBe('http://127.0.0.1:3001/api');
    });

    it('空值与非法输入返回空串', () => {
        expect(normalizeApiBase('')).toBe('');
        expect(normalizeApiBase('   ')).toBe('');
        expect(normalizeApiBase('http://')).toBe('');
    });
});

describe('getApiBase / getApiOrigin', () => {
    afterEach(() => {
        delete (window as WindowWithApiBase).__API_BASE__;
        clearApiBase();
    });

    it('优先使用宿主注入的 window.__API_BASE__', () => {
        setInjectedBase('http://10.0.0.5:3001');

        expect(getApiBase()).toBe('http://10.0.0.5:3001/api');
        expect(getApiOrigin()).toBe('http://10.0.0.5:3001');
    });

    it('注入值已带 /api 时不重复追加', () => {
        setInjectedBase('http://10.0.0.5:3001/api');

        expect(getApiBase()).toBe('http://10.0.0.5:3001/api');
        expect(getApiBase()?.endsWith('/api/api')).toBe(false);
    });

    it('注入值优先于 localStorage', () => {
        setInjectedBase('http://10.0.0.5:3001');
        window.localStorage.setItem(API_BASE_STORAGE_KEY, 'http://10.0.0.9:3001/api');

        expect(getApiBase()).toBe('http://10.0.0.5:3001/api');
    });

    it('无注入时使用 localStorage 中保存的地址', () => {
        window.localStorage.setItem(API_BASE_STORAGE_KEY, 'http://10.0.0.9:3001');

        expect(getApiBase()).toBe('http://10.0.0.9:3001/api');
        expect(getApiOrigin()).toBe('http://10.0.0.9:3001');
    });

    it('都未配置时回落到浏览器同源 /api（nginx 反代部署）', () => {
        expect(getApiBase()).toBe(`${window.location.origin}/api`);
        expect(hasServerConfigured()).toBe(true);
    });

    it('自定义前缀路径的 origin 保留前缀（路径拼接仍正确）', () => {
        setInjectedBase('https://demo.example.com/v2');

        expect(getApiBase()).toBe('https://demo.example.com/v2/api');
        expect(getApiOrigin()).toBe('https://demo.example.com/v2');
        expect(`${getApiOrigin()}/api/categories`).toBe('https://demo.example.com/v2/api/categories');
    });
});

describe('setApiBase / clearApiBase', () => {
    afterEach(() => {
        delete (window as WindowWithApiBase).__API_BASE__;
        clearApiBase();
    });

    it('写入规范化后的地址并可被 getApiBase 读回', () => {
        expect(setApiBase('10.0.0.7:3001', { allowInsecure: true })).toBe(
            'http://10.0.0.7:3001/api',
        );
        expect(window.localStorage.getItem(API_BASE_STORAGE_KEY)).toBe('http://10.0.0.7:3001/api');
        expect(getApiBase()).toBe('http://10.0.0.7:3001/api');
    });

    it('地址非法时抛错（由页面提示用户）', () => {
        expect(() => setApiBase('   ')).toThrow('服务器地址格式不正确');
    });

    it('清除后回落到默认解析', () => {
        setApiBase('https://demo.example.com');
        clearApiBase();

        expect(window.localStorage.getItem(API_BASE_STORAGE_KEY)).toBeNull();
        expect(getApiBase()).toBe(`${window.location.origin}/api`);
    });
});

describe('getApiBaseSource / isFallbackBase / describeApiBase', () => {
    afterEach(() => {
        delete (window as WindowWithApiBase).__API_BASE__;
        clearApiBase();
    });

    it('宿主注入 → source=injected', () => {
        setInjectedBase('http://10.0.0.5:3001');

        expect(getApiBaseSource()).toBe('injected');
        expect(isFallbackBase()).toBe(false);
    });

    it('localStorage 中保存的地址 → source=storage，不算兜底', () => {
        setApiBase('https://demo.example.com');

        expect(getApiBaseSource()).toBe('storage');
        expect(isFallbackBase()).toBe(false);
    });

    it('未配置时回落到同源 → source=same-origin，不算兜底', () => {
        expect(getApiBaseSource()).toBe('same-origin');
        expect(isFallbackBase()).toBe(false);
    });

    it('describeApiBase 返回基址 / origin / 来源快照', () => {
        setApiBase('https://demo.example.com/v2');

        expect(describeApiBase()).toEqual({
            base: 'https://demo.example.com/v2/api',
            origin: 'https://demo.example.com/v2',
            source: 'storage',
            isFallback: false,
            configured: true,
        });
    });
});

describe('isInsecureRemote', () => {
    it('远程（非回环）http 视为不安全', () => {
        expect(isInsecureRemote('http://192.168.1.10:3001')).toBe(true);
        // 省略协议时会被补成 http://，同样不安全
        expect(isInsecureRemote('192.168.1.10:3001')).toBe(true);
    });

    it('本机回环地址的 http 放行', () => {
        expect(isInsecureRemote('http://localhost:3001')).toBe(false);
        expect(isInsecureRemote('http://127.0.0.1:3001')).toBe(false);
    });

    it('https 与非法输入都不算不安全', () => {
        expect(isInsecureRemote('https://demo.example.com')).toBe(false);
        expect(isInsecureRemote('   ')).toBe(false);
    });
});

describe('setApiBase 明文校验与会话清理', () => {
    afterEach(() => {
        delete (window as WindowWithApiBase).__API_BASE__;
        clearApiBase();
    });

    it('远程 http 默认抛 InsecureTransportError，且不写 localStorage', () => {
        expect(() => setApiBase('http://192.168.1.10:3001')).toThrow(InsecureTransportError);
        expect(window.localStorage.getItem(API_BASE_STORAGE_KEY)).toBeNull();
    });

    it('错误码便于调用方判定（gate / 页面据此弹二次确认）', () => {
        try {
            setApiBase('http://192.168.1.10:3001');
            throw new Error('应当抛错');
        } catch (err) {
            expect(err).toBeInstanceOf(InsecureTransportError);
            expect((err as InsecureTransportError).code).toBe('INSECURE_TRANSPORT');
        }
    });

    it('allowInsecure 后可以保存远程 http', () => {
        expect(setApiBase('http://192.168.1.10:3001', { allowInsecure: true })).toBe(
            'http://192.168.1.10:3001/api',
        );
        expect(getApiBase()).toBe('http://192.168.1.10:3001/api');
    });

    it('保存时清空跨服务器的本地会话（避免 401 死循环）', () => {
        for (const key of [
            'auth_token',
            'refresh_token',
            'auth_user',
            'available_tenants',
            'selected_tenant_id',
        ]) {
            window.localStorage.setItem(key, 'stale');
        }

        setApiBase('https://demo.example.com');

        for (const key of [
            'auth_token',
            'refresh_token',
            'auth_user',
            'available_tenants',
            'selected_tenant_id',
        ]) {
            expect(window.localStorage.getItem(key)).toBeNull();
        }
        expect(window.localStorage.getItem(API_BASE_STORAGE_KEY)).toBe('https://demo.example.com/api');
    });
});

describe('probeServer', () => {
    const originalFetch = global.fetch;

    afterEach(() => {
        global.fetch = originalFetch;
    });

    it('2xx → ok，并解析 healthz 字段', async () => {
        const fetchMock = jest.fn().mockResolvedValue({
            ok: true,
            status: 200,
            text: async () =>
                JSON.stringify({ status: 'ok', db: 'up', service: 'assets-api' }),
        });
        global.fetch = fetchMock as unknown as typeof fetch;

        const result = await probeServer('http://10.0.0.5:3001/');

        expect(fetchMock).toHaveBeenCalledWith(
            'http://10.0.0.5:3001/healthz',
            expect.objectContaining({ method: 'GET' }),
        );
        expect(result.ok).toBe(true);
        expect(result.status).toBe(200);
        expect(result.body).toEqual({ status: 'ok', db: 'up', service: 'assets-api' });
        expect(result.latencyMs).toBeGreaterThanOrEqual(0);
        expect(result.errorKind).toBeUndefined();
    });

    it('5xx → http-5xx（服务在但内部异常）', async () => {
        global.fetch = jest.fn().mockResolvedValue({
            ok: false,
            status: 503,
            text: async () => '',
        }) as unknown as typeof fetch;

        const result = await probeServer('http://10.0.0.5:3001');

        expect(result.ok).toBe(false);
        expect(result.status).toBe(503);
        expect(result.errorKind).toBe('http-5xx');
    });

    it('4xx → http-4xx（路径配错）', async () => {
        global.fetch = jest.fn().mockResolvedValue({
            ok: false,
            status: 404,
            text: async () => '',
        }) as unknown as typeof fetch;

        const result = await probeServer('http://10.0.0.5:3001');

        expect(result.errorKind).toBe('http-4xx');
    });

    it('网络异常（含 CORS 被拒）→ network', async () => {
        global.fetch = jest
            .fn()
            .mockRejectedValue(new TypeError('Failed to fetch')) as unknown as typeof fetch;

        const result = await probeServer('http://10.0.0.5:3001');

        expect(result.ok).toBe(false);
        expect(result.status).toBeNull();
        expect(result.errorKind).toBe('network');
    });

    it('超时中断 → timeout', async () => {
        const abortError = Object.assign(new Error('The operation was aborted'), {
            name: 'AbortError',
        });
        global.fetch = jest.fn().mockRejectedValue(abortError) as unknown as typeof fetch;

        const result = await probeServer('http://10.0.0.5:3001', { timeoutMs: 1 });

        expect(result.ok).toBe(false);
        expect(result.errorKind).toBe('timeout');
    });
});


