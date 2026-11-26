/**
 * 认证令牌读取（前端唯一读取入口）
 *
 * 令牌由 `store/authStore.ts` 在登录 / 自动续期成功后写入 localStorage，
 * `utils/adapters/httpAdapter.ts` 在 XHR 路径上读取同一 key 并附加 `Authorization`。
 * 本模块把「key 名 + 读取逻辑 + 头构造」收敛到一处，
 * 供 XHR 之外的 fetch 调用（大文件上传、SSE 对话）复用。
 *
 * @remarks localStorage 在 SSR / SSG 阶段不存在，所有读取都做了守卫并静默失败。
 */

/** localStorage 中存放访问令牌的键名（须与 authStore.ts / httpAdapter.ts 保持一致） */
export const AUTH_TOKEN_KEY = 'auth_token';

/** 读取访问令牌，未登录 / 非浏览器环境返回 null */
export function getAuthToken(): string | null {
    try {
        if (typeof window !== 'undefined') {
            const stored = localStorage.getItem(AUTH_TOKEN_KEY);
            return stored || null;
        }
    } catch {
        // 隐私模式等场景 localStorage 可能抛错：静默降级为匿名
    }
    return null;
}

/**
 * 构造带认证的请求头
 *
 * 未登录时原样返回 `extra`（保持改造前行为，由后端统一返回 401）。
 */
export function withAuthHeaders(
    extra: Record<string, string> = {}
): Record<string, string> {
    const token = getAuthToken();
    return token ? { ...extra, Authorization: `Bearer ${token}` } : { ...extra };
}
