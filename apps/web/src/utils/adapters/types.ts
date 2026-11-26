/**
 * API 适配器类型定义
 *
 * 定义统一的 API 适配器接口。前端现为**纯 HTTP 单通道**，
 * 唯一实现是 `httpAdapter`（`fetch`）；接口保留是为了让 service 层
 * 不依赖具体传输方式，也便于测试替身。
 */

// ======================== API 调用配置 ========================

/** API 调用配置 */
export interface ApiConfig {
    /** 是否自动显示错误通知（默认 true） */
    showError?: boolean;
    /** 失败重试次数（默认 0） */
    retryCount?: number;
    /** 重试间隔毫秒数（默认 1000） */
    retryDelay?: number;
    /** 自定义错误消息 */
    errorMessage?: string;
}

// ======================== 适配器接口 ========================

/**
 * API 适配器接口
 *
 * 所有适配器（Tauri / HTTP）必须实现此接口。
 * 提供 get/post/put/delete 语义化方法。
 */
export interface IApiAdapter {
    /**
     * GET 请求（查询类操作）
     * @param command Tauri 命令名称 或 HTTP URL 路径
     * @param args 请求参数
     * @param config 调用配置
     */
    get<T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T>;

    /**
     * POST 请求（新增类操作）
     * @param command Tauri 命令名称 或 HTTP URL 路径
     * @param args 请求体参数
     * @param config 调用配置
     */
    post<T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T>;

    /**
     * PUT 请求（更新类操作）
     * @param command Tauri 命令名称 或 HTTP URL 路径
     * @param args 请求体参数
     * @param config 调用配置
     */
    put<T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T>;

    /**
     * DELETE 请求（删除类操作）
     * @param command Tauri 命令名称 或 HTTP URL 路径
     * @param args 请求参数
     * @param config 调用配置
     */
    delete<T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T>;
}
