/**
 * API 基址解析（前端唯一入口）
 *
 * 前端已从「Tauri IPC + HTTP 双通道」切换为**纯 HTTP 单通道**：
 * 桌面版与 Web 版共用同一套 `fetch` 调用，区别只在基址来源。
 * 所有需要和服务端通信的模块（`httpAdapter` / `uploadService` / `useChatStream`）
 * 都必须通过本模块取基址，禁止再各自硬编码 `localhost:3001`。
 *
 * 解析优先级（自上而下，先命中先返回）：
 * 1. `window.__API_BASE__`              桌面端由宿主注入（企业预置地址）
 * 2. `localStorage['api_base_url']`     用户在「系统配置 → 服务端连接」（`/settings/server`）填写的地址
 * 3. `process.env.NEXT_PUBLIC_API_BASE_URL`   构建期注入
 * 4. Tauri 运行时                        `http://127.0.0.1:3001/api`（桌面内置服务端）
 * 5. 开发模式（`next dev`）               `http://localhost:3001/api`
 * 6. 浏览器同源                          `${location.origin}/api`（nginx 反代部署）
 * 7. 其它（SSR / 未知宿主）               `null` → 调用方必须给出可操作错误，不得静默直连
 *
 * @remarks
 * - 返回值**统一带 `/api` 后缀**（如 `http://localhost:3001/api`），
 *   与 `COMMAND_ROUTE_MAP` 中的 `/api/...` 路径拼接时要先去掉后缀，见 [`getApiOrigin`]。
 * - 4 / 5 是**兼容兜底**：桌面形态的服务端就起在本机 3001，开发模式习惯也是本机 3001。
 *   `ServerConfigGate` 只在「用的是这两条兜底 **且** 探活失败」时阻断首屏，
 *   让用户改填远程地址；兜底本身随桌面瘦身（P0-D）再删除（见
 *   `docs/桌面与Web全HTTP改造清单.md` §5.2）。
 * - 本模块不抛异常：读取 localStorage 失败时静默降级到下一优先级。
 *   唯一的例外是 [`setApiBase`]：地址非法或未确认的不安全传输会抛错。
 */

/** localStorage 中存放服务端地址的键名 */
export const API_BASE_STORAGE_KEY = 'api_base_url';

/**
 * 基址来源
 *
 * - `injected`：宿主注入（`window.__API_BASE__`）
 * - `storage`：用户在「服务端连接」页保存的地址
 * - `env`：构建期 `NEXT_PUBLIC_API_BASE_URL`
 * - `tauri-default` / `dev-default`：兼容兜底（探活失败时触发配置 Gate）
 * - `same-origin`：浏览器同源 + `/api`（nginx 反代部署）
 * - `none`：无法确定（SSR / 未知宿主）
 */
export type ApiBaseSource =
    | 'injected'
    | 'storage'
    | 'env'
    | 'tauri-default'
    | 'dev-default'
    | 'same-origin'
    | 'none';

/** 保存地址时需要清理的会话键（跨服务器的令牌无意义，且会引发 401 死循环） */
const SESSION_STORAGE_KEYS = [
    'auth_token',
    'refresh_token',
    'auth_user',
    'available_tenants',
    'selected_tenant_id',
] as const;

/** 回环地址：`http://` 仅在这里允许（内网部署默认给出安全提示） */
const LOOPBACK_HOSTS = new Set(['localhost', '127.0.0.1', '::1', '[::1]']);

/**
 * 非回环 `http://` 地址的错误
 *
 * 令牌会明文上网，因此默认拒绝；调用方（配置页 / Gate）捕获后弹二次确认，
 * 再以 `setApiBase(url, { allowInsecure: true })` 重试。
 */
export class InsecureTransportError extends Error {
    readonly code = 'INSECURE_TRANSPORT';

    constructor() {
        super('远程服务器使用 http:// 会明文传输令牌，请确认网络环境后继续');
        this.name = 'InsecureTransportError';
    }
}

/** 开发模式默认地址（`pnpm dev:server` 起的 Rust 服务） */
export const DEV_DEFAULT_API_BASE = 'http://localhost:3001/api';

/** 桌面形态默认地址（Tauri 内置 HTTP 服务，端口与 `.env.toml` 的 `[api] port` 一致） */
export const TAURI_DEFAULT_API_BASE = 'http://127.0.0.1:3001/api';

/** 是否是「协议://」开头的绝对地址 */
const ABSOLUTE_URL_PATTERN = /^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//;

/**
 * 规范化服务端地址
 *
 * 规则：
 * - 去首尾空白；
 * - 缺少协议时补 `http://`（用户常只填 `192.168.1.10:3001`）；
 * - 去掉末尾 `/`；
 * - **补齐 `/api` 后缀**（已带则原样保留，大小写不敏感）。
 *
 * @example
 * ```ts
 * normalizeApiBase('192.168.1.10:3001')      // 'http://192.168.1.10:3001/api'
 * normalizeApiBase('https://demo.com/api/')   // 'https://demo.com/api'
 * normalizeApiBase('https://demo.com/v2')     // 'https://demo.com/v2/api'
 * normalizeApiBase('')                        // ''
 * ```
 */
export function normalizeApiBase(input: string): string {
    const raw = (input || '').trim();
    if (!raw) {
        return '';
    }

    const withProtocol = ABSOLUTE_URL_PATTERN.test(raw)
        ? raw
        : `http://${raw.replace(/^\/+/, '')}`;

    let parsed: URL;
    try {
        parsed = new URL(withProtocol);
    } catch {
        return '';
    }

    if (!parsed.hostname) {
        return '';
    }

    const trimmedPath = parsed.pathname.replace(/\/+$/, '');
    const basePath = trimmedPath === '' || trimmedPath === '/' ? '' : trimmedPath;
    const fullPath = /\/api$/i.test(basePath) ? basePath : `${basePath}/api`;

    return `${parsed.origin}${fullPath}`;
}

/** 是否是 Tauri v2 / v1 运行时 */
function isTauriRuntime(): boolean {
    if (typeof window === 'undefined') {
        return false;
    }
    const w = window as Window & {
        __TAURI_INTERNALS__?: unknown;
        __TAURI__?: unknown;
    };
    return w.__TAURI_INTERNALS__ !== undefined || w.__TAURI__ !== undefined;
}

/** 是否处于 `next dev` 开发模式（Next.js 会在构建时内联 NODE_ENV） */
function isDevMode(): boolean {
    return typeof process !== 'undefined' && process.env?.NODE_ENV === 'development';
}

/** 安全读取 localStorage（隐私模式下会抛错） */
function safeReadStorage(key: string): string | null {
    try {
        if (typeof window === 'undefined') {
            return null;
        }
        const value = window.localStorage.getItem(key);
        return value && value.trim() ? value : null;
    } catch {
        return null;
    }
}

/** 读取构建期环境变量（SSG / SSR 下 process 仍存在） */
function readEnvBase(): string | null {
    const value =
        typeof process !== 'undefined'
            ? process.env?.NEXT_PUBLIC_API_BASE_URL
            : undefined;
    return value && value.trim() ? value : null;
}

/**
 * 解析当前生效的 API 基址及其来源
 *
 * 内部单一实现：`getApiBase()` / `getApiBaseSource()` / `isFallbackBase()` 都基于它，
 * 避免出现「地址取自 A 层、来源却显示成 B 层」的不一致。
 */
function resolveApiBase(): { base: string | null; source: ApiBaseSource } {
    if (typeof window === 'undefined') {
        // SSR / SSG 阶段不发业务请求
        return { base: null, source: 'none' };
    }

    const injected = (window as Window & { __API_BASE__?: unknown }).__API_BASE__;
    if (typeof injected === 'string' && injected.trim()) {
        const base = normalizeApiBase(injected);
        if (base) {
            return { base, source: 'injected' };
        }
    }

    const stored = safeReadStorage(API_BASE_STORAGE_KEY);
    if (stored) {
        const base = normalizeApiBase(stored);
        if (base) {
            return { base, source: 'storage' };
        }
    }

    const envBase = readEnvBase();
    if (envBase) {
        const base = normalizeApiBase(envBase);
        if (base) {
            return { base, source: 'env' };
        }
    }

    // 桌面形态：HTTP 服务与 WebView 同进程，固定监听本机 3001
    if (isTauriRuntime()) {
        return { base: TAURI_DEFAULT_API_BASE, source: 'tauri-default' };
    }

    // 开发模式：`next dev` 占 1480，`/api` 没有反代，必须直连后端端口
    if (isDevMode()) {
        return { base: DEV_DEFAULT_API_BASE, source: 'dev-default' };
    }

    // 生产 Web：nginx 同源反代
    const origin = window.location?.origin;
    if (origin && /^https?:\/\//i.test(origin)) {
        return { base: `${origin}/api`, source: 'same-origin' };
    }

    return { base: null, source: 'none' };
}

/**
 * 解析当前生效的 API 基址（**含** `/api` 后缀）
 *
 * @returns 基址；无法确定时返回 `null`（调用方必须提示用户配置服务器地址）
 */
export function getApiBase(): string | null {
    return resolveApiBase().base;
}

/** 当前基址的来源（诊断页据此展示「地址来自哪一层」） */
export function getApiBaseSource(): ApiBaseSource {
    return resolveApiBase().source;
}

/**
 * 当前基址是否来自兼容兜底（桌面 / 开发模式的本机 3001）
 *
 * `ServerConfigGate` 只对**兜底来源**做探活：探不通说明用户其实想连别的服务器。
 * 用户手动保存过地址（`storage`）或由部署注入（`env` / `injected` / `same-origin`）时一律不拦。
 */
export function isFallbackBase(): boolean {
    const source = getApiBaseSource();
    return source === 'tauri-default' || source === 'dev-default';
}

/** 基址解析快照（诊断页 / 日志用） */
export function describeApiBase(): {
    base: string | null;
    origin: string | null;
    source: ApiBaseSource;
    isFallback: boolean;
    configured: boolean;
} {
    const { base, source } = resolveApiBase();
    return {
        base,
        origin: base ? base.replace(/\/api\/?$/i, '') : null,
        source,
        isFallback: source === 'tauri-default' || source === 'dev-default',
        configured: base !== null,
    };
}

/**
 * 解析 API 的**源地址**（**不含** `/api` 后缀）
 *
 * `COMMAND_ROUTE_MAP` 里的路径本身都以 `/api/` 开头，
 * 因此拼接时要使用本函数，否则会拼出 `/api/api/...`（历史 bug，已在本轮修复）。
 *
 * @example
 * ```ts
 * getApiBase()   // 'http://localhost:3001/api'
 * getApiOrigin() // 'http://localhost:3001'
 * `${getApiOrigin()}${'/api/categories'}` // 'http://localhost:3001/api/categories'
 * ```
 */
export function getApiOrigin(): string | null {
    const base = getApiBase();
    if (!base) {
        return null;
    }
    return base.replace(/\/api\/?$/i, '');
}

/**
 * 是否已配置可用的服务端地址
 *
 * 注意：桌面 / 开发模式的 3001 兜底也算「已配置」，因此在浏览器里该函数恒为 `true`。
 * 要判断「用户是否显式指定过地址」请看 [`isFallbackBase`] / [`describeApiBase`]；
 * `ServerConfigGate` 正是靠「兜底来源 + 探活失败」识别「兜底其实连不上」的场景。
 */
export function hasServerConfigured(): boolean {
    return getApiBase() !== null;
}

/**
 * 地址是否属于「非回环 `http://`」（会用明文传输令牌）
 *
 * `http://localhost` / `http://127.0.0.1` / `http://[::1]` 视为本机，放行；
 * 远程 `http://` 返回 `true`，由调用方弹二次确认（见 [`setApiBase`]）。
 */
export function isInsecureRemote(input: string): boolean {
    const normalized = normalizeApiBase(input);
    if (!normalized) {
        return false;
    }
    try {
        const parsed = new URL(normalized);
        if (parsed.protocol !== 'http:') {
            return false; // https 天然安全
        }
        return !LOOPBACK_HOSTS.has(parsed.hostname.toLowerCase());
    } catch {
        return false;
    }
}

/**
 * 探活失败分类
 *
 * 浏览器的 `fetch` 无法区分「DNS 失败 / 证书无效 / CORS 被拒」，因此统一归为 `network`，
 * 诊断页文案里同时提示这三种可能。
 */
export type ProbeErrorKind = 'timeout' | 'network' | 'http-4xx' | 'http-5xx';

/** [`probeServer`] 的返回结构 */
export interface ProbeResult {
    /** 是否拿到 2xx */
    ok: boolean;
    /** HTTP 状态码；网络层失败时为 `null` */
    status: number | null;
    /** 往返耗时（ms） */
    latencyMs: number;
    /** `/healthz` 的响应体（解析失败时为 `undefined`） */
    body?: {
        status?: string;
        db?: string;
        service?: string;
        insecure_transport?: boolean;
    };
    /** 失败分类（成功时为空） */
    errorKind?: ProbeErrorKind;
}

/**
 * 探活：`GET {origin}/healthz`
 *
 * `/healthz` 在 `crates/api/src/lib.rs` 的 `public_routes` 里**免鉴权**，
 * 所以配置页在未登录状态下也能判断服务器是否可达。
 * - 2xx → `ok: true`（`body` 里带 `db` / `insecure_transport` 供诊断展示）
 * - 503 等 → `ok: false` + `errorKind`（503 通常表示「服务在但数据库挂了」）
 * - 超时 / 网络失败 → `ok: false` + `errorKind: 'timeout' | 'network'`
 */
export async function probeServer(
    origin: string,
    options?: { timeoutMs?: number },
): Promise<ProbeResult> {
    const timeoutMs = options?.timeoutMs ?? 3000;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs);
    const startedAt = Date.now();

    try {
        const res = await fetch(`${origin.replace(/\/+$/, '')}/healthz`, {
            method: 'GET',
            signal: controller.signal,
        });
        const latencyMs = Date.now() - startedAt;

        let body: ProbeResult['body'];
        try {
            const text = typeof res.text === 'function' ? await res.text() : '';
            body = text ? JSON.parse(text) : undefined;
        } catch {
            body = undefined;
        }

        if (res.ok) {
            return { ok: true, status: res.status, latencyMs, body };
        }
        return {
            ok: false,
            status: res.status,
            latencyMs,
            body,
            errorKind: res.status >= 500 ? 'http-5xx' : 'http-4xx',
        };
    } catch (err) {
        const latencyMs = Date.now() - startedAt;
        const aborted = (err as { name?: string })?.name === 'AbortError';
        return {
            ok: false,
            status: null,
            latencyMs,
            errorKind: aborted ? 'timeout' : 'network',
        };
    } finally {
        clearTimeout(timer);
    }
}

// ======================== 探活缓存 ========================

/**
 * 探活成功缓存（sessionStorage）的键名
 *
 * 只缓存**成功**结果：失败必须每次都重新探，否则用户改好服务器也进不去。
 */
export const API_PROBE_CACHE_KEY = 'api_base_probe_ok';

/** 缓存有效期：5 分钟（跨页面刷新复用，换服务器地址后 origin 不匹配即失效） */
const PROBE_CACHE_TTL_MS = 5 * 60 * 1000;

/**
 * 读取探活成功缓存
 *
 * @param origin 当前生效的源地址（不含 `/api`）；与缓存里的 origin 不一致时视为未命中
 * @returns 5 分钟内的同 origin 成功缓存是否存在
 */
export function readProbeCache(origin: string): boolean {
    try {
        if (typeof window === 'undefined') {
            return false;
        }
        const raw = window.sessionStorage.getItem(API_PROBE_CACHE_KEY);
        if (!raw) {
            return false;
        }
        const parsed = JSON.parse(raw) as { origin?: string; at?: number };
        if (parsed?.origin !== origin || typeof parsed.at !== 'number') {
            return false;
        }
        return Date.now() - parsed.at < PROBE_CACHE_TTL_MS;
    } catch {
        return false;
    }
}

/** 写入探活成功缓存（失败只影响下次是否重复探活，静默忽略） */
export function writeProbeCache(origin: string): void {
    try {
        if (typeof window === 'undefined') {
            return;
        }
        window.sessionStorage.setItem(
            API_PROBE_CACHE_KEY,
            JSON.stringify({ origin, at: Date.now() }),
        );
    } catch {
        // 忽略：缓存失败只影响下次是否重复探活
    }
}

/**
 * 清除探活成功缓存
 *
 * `httpAdapter` 在**传输层失败**（请求没拿到任何 HTTP 响应）时调用：
 * 缓存的「上次探活成功」会掩盖「后端已经挂了」的事实，导致整页加载时
 * `ServerConfigGate` 直接放行、用户直到点击操作才看到网络错误。
 * 清掉后下一次整页加载会重新探活，及时拦住并提示配置服务器地址。
 */
export function clearProbeCache(): void {
    try {
        if (typeof window === 'undefined') {
            return;
        }
        window.sessionStorage.removeItem(API_PROBE_CACHE_KEY);
    } catch {
        // 忽略
    }
}

/** [`setApiBase`] 的选项 */
export interface SetApiBaseOptions {
    /** 允许保存非回环 `http://`（用户已在二次确认里点「仍要保存」） */
    allowInsecure?: boolean;
}

/**
 * 保存服务端地址（规范化后写入 localStorage）
 *
 * 除写入地址外还会**清空本地会话**（`auth_token` / `refresh_token` / 用户与租户缓存）：
 * 跨服务器的令牌没有任何意义，留着只会在切换后触发 401 死循环。
 *
 * @param url 用户输入的地址，允许省略协议与 `/api` 后缀
 * @returns 规范化后的地址
 * @throws 地址为空或无法解析时抛错；非回环 `http://` 且未传 `allowInsecure` 时抛
 *         [`InsecureTransportError`]（调用方应弹二次确认后带选项重试）
 */
export function setApiBase(url: string, options?: SetApiBaseOptions): string {
    const normalized = normalizeApiBase(url);
    if (!normalized) {
        throw new Error('服务器地址格式不正确，请填写形如 http://192.168.1.10:3001 的地址');
    }
    if (isInsecureRemote(normalized) && !options?.allowInsecure) {
        throw new InsecureTransportError();
    }
    if (typeof window !== 'undefined') {
        for (const key of SESSION_STORAGE_KEYS) {
            try {
                window.localStorage.removeItem(key);
            } catch {
                // 忽略：localStorage 不可用时只影响会话清理
            }
        }
        window.localStorage.setItem(API_BASE_STORAGE_KEY, normalized);
    }
    return normalized;
}

/** 清除已保存的服务端地址（回落到优先级 3~6） */
export function clearApiBase(): void {
    try {
        if (typeof window !== 'undefined') {
            window.localStorage.removeItem(API_BASE_STORAGE_KEY);
        }
    } catch {
        // 忽略
    }
}
