/**
 * 大文件分片上传 API 服务（**纯 HTTP**）
 *
 * 全部接口经 `fetch` 调用后端 `/api/upload/*`（`crates/api/src/upload_routes.rs`），
 * 桌面版与 Web 版共用同一套代码，不再有 `invoke()` 调用点。
 *
 * - 基址由 `utils/apiBase.ts` 统一解析（禁止硬编码 `localhost:3001`）；
 * - S3 分片直传始终以 presigned URL 直接 PUT，**不带** Authorization
 *   （否则签名不匹配，S3 会返回 403）；
 * - 401 处理：`/api/upload/*` 已纳入 `auth_middleware`，访问令牌过期时
 *   先用 refresh token 静默续期并重放一次（与 `httpAdapter` 一致，见 D-7）。
 */

import { getAuthToken } from '../utils/authToken';
import { getApiBase } from '../utils/apiBase';
import {
  clearAuthSession,
  extractServerMessage,
  refreshAccessToken,
} from '../utils/adapters/httpAdapter';

// ======================== 类型定义 ========================

/** 初始化上传（占位）响应 */
export interface InitUploadResponse {
  uploadId: string;
  needStart: boolean;
}

/** 开始上传（创建 S3 MultipartUpload）响应 */
export interface StartUploadResponse {
  uploadId: string;
  s3UploadId: string;
  chunkSize: number;
  totalChunks: number;
  presignedUrls: string[];
}

export interface UploadProgress {
  status: string;
  receivedChunks: number[];
  totalChunks: number;
  progressPct: number;
}

export interface UploadCompleteResponse {
  fileUrl: string;
  etag: string;
}

export interface ReportChunkRequest {
  partNumber: number;
  etag: string;
}

export interface UploadVersionInfo {
  id: string;
  version: number;
  isLatest: boolean;
  originalFilename: string;
  fileSize: number;
  fileUrl: string | null;
  changeReason: string | null;
  createdAt: string;
}

// ======================== 上传接口契约 ========================

/** 上传接口契约：HTTP 实现（`HttpUploadAdapter`）需实现的基础操作 */
interface UploadAdapter {
  init(filename: string, fileSize: number, mimeType: string, options?: {
    fileGroupId?: string;
    changeReason?: string;
    fileMd5?: string;
  }): Promise<InitUploadResponse>;

  startUpload(uploadId: string): Promise<StartUploadResponse>;

  commit(uploadId: string, contextType: string, contextId: string): Promise<void>;

  getVersionHistory(fileGroupId: string): Promise<UploadVersionInfo[]>;

  /** 上报分片上传完成 */
  reportChunk(uploadId: string, partNumber: number, etag: string): Promise<void>;

  /** 完成上传合并分片 */
  complete(uploadId: string): Promise<UploadCompleteResponse>;

  /** 获取上传进度 */
  getProgress(uploadId: string): Promise<UploadProgress>;

  /** 取消上传 */
  abort(uploadId: string): Promise<void>;
}

// ======================== HTTP 适配器 ========================

/**
 * 附加认证头的请求头构造器
 *
 * `/api/upload/*` 已纳入后端 `auth_middleware`，HTTP 模式必须带 Bearer 令牌。
 * 注意：S3 presigned URL 的 PUT 请求**不能**加此头（否则签名不匹配）。
 */
function withAuth(headers: Record<string, string> = {}): Record<string, string> {
  const token = getAuthToken();
  return token ? { ...headers, Authorization: `Bearer ${token}` } : headers;
}

/**
 * 读取错误响应体（`res.text()` 在部分测试替身/流式响应上不存在）
 */
async function readErrorBody(res: Response): Promise<string> {
  try {
    return typeof res.text === 'function' ? await res.text() : '';
  } catch {
    return '';
  }
}

/**
 * 抛出带业务消息的上传错误
 *
 * 优先使用后端 `ApiError.message`（如「上传会话已过期」），
 * 拿不到时回落到 `HTTP 401 Unauthorized` 形式的状态行。
 * 消息前缀（`初始化上传失败` 等）保持不变，便于页面按操作归类提示。
 */
async function throwHttpError(action: string, res: Response): Promise<never> {
  const body = await readErrorBody(res);
  const detail = extractServerMessage(body)
    || `HTTP ${res.status} ${res.statusText}`.trim();
  throw new Error(`${action}: ${detail}`);
}

class HttpUploadAdapter implements UploadAdapter {
  /** 显式指定的基址（含 `/api`）；为空时由 `getApiBase()` 运行时解析 */
  private readonly baseUrl: string;

  constructor(baseUrl?: string) {
    this.baseUrl = baseUrl ?? '';
  }

  /** 当前基址：显式传入优先，其次运行时解析（localStorage / env / 同源 /api） */
  private resolveBase(): string {
    const base = this.baseUrl || getApiBase();
    if (!base) {
      throw new Error(
        '未配置服务器地址：请到「系统配置 → 服务端连接」填写后端地址'
        + '（或在构建时设置 NEXT_PUBLIC_API_BASE_URL）',
      );
    }
    return base;
  }

  /**
   * 发送带认证头的请求
   *
   * 401 → 用 refresh token 静默续期并重放一次（D-7）；
   * 续期失败或重放仍 401 → 清会话（避免「有令牌但一直 401」的僵死状态）。
   */
  private async request(url: string, init: RequestInit = {}): Promise<Response> {
    let res = await fetch(url, init);
    if (res.status !== 401 || !getAuthToken()) {
      return res;
    }

    const renewedToken = await refreshAccessToken();
    if (!renewedToken) {
      clearAuthSession();
      return res;
    }

    res = await fetch(url, {
      ...init,
      headers: {
        ...(init.headers as Record<string, string> | undefined),
        Authorization: `Bearer ${renewedToken}`,
      },
    });
    if (res.status === 401) {
      clearAuthSession();
    }
    return res;
  }

  async init(
    filename: string,
    fileSize: number,
    mimeType: string,
    options?: { fileGroupId?: string; changeReason?: string; fileMd5?: string }
  ): Promise<InitUploadResponse> {
    const res = await this.request(`${this.resolveBase()}/upload/init`, {
      method: 'POST',
      headers: withAuth({ 'Content-Type': 'application/json' }),
      body: JSON.stringify({
        filename,
        file_size: fileSize,
        mime_type: mimeType,
        file_group_id: options?.fileGroupId,
        change_reason: options?.changeReason,
        file_md5: options?.fileMd5,
      }),
    });
    if (!res.ok) await throwHttpError('初始化上传失败', res);
    const json = await res.json();
    const data = json.data || json;
    // HTTP 适配器返回 snake_case，转换为 camelCase
    return {
      uploadId: data.upload_id ?? data.uploadId,
      needStart: data.need_start ?? data.needStart ?? true,
    };
  }

  async startUpload(uploadId: string): Promise<StartUploadResponse> {
    const res = await this.request(`${this.resolveBase()}/upload/${uploadId}/start`, {
      method: 'POST',
      headers: withAuth(),
    });
    if (!res.ok) await throwHttpError('开始上传失败', res);
    const json = await res.json();
    const data = json.data || json;
    return {
      uploadId: data.upload_id ?? data.uploadId,
      s3UploadId: data.s3_upload_id ?? data.s3UploadId,
      chunkSize: data.chunk_size ?? data.chunkSize,
      totalChunks: data.total_chunks ?? data.totalChunks,
      presignedUrls: data.presigned_urls ?? data.presignedUrls,
    };
  }

  async commit(uploadId: string, contextType: string, contextId: string): Promise<void> {
    const res = await this.request(`${this.resolveBase()}/upload/${uploadId}/commit`, {
      method: 'POST',
      headers: withAuth({ 'Content-Type': 'application/json' }),
      body: JSON.stringify({ context_type: contextType, context_id: Number(contextId) }),
    });
    if (!res.ok) await throwHttpError('提交上传失败', res);
  }

  async getVersionHistory(fileGroupId: string): Promise<UploadVersionInfo[]> {
    // 后端路由：GET /api/upload/versions/{fileGroupId}
    // 响应与 Tauri 命令 upload_get_version_history 同形（camelCase，id 为字符串）
    const res = await this.request(`${this.resolveBase()}/upload/versions/${encodeURIComponent(fileGroupId)}`, {
      headers: withAuth(),
    });
    if (!res.ok) await throwHttpError('获取版本历史失败', res);
    const json = await res.json();
    return json.data || json;
  }

  async reportChunk(uploadId: string, partNumber: number, etag: string): Promise<void> {
    const res = await this.request(`${this.resolveBase()}/upload/${uploadId}/chunk`, {
      method: 'POST',
      headers: withAuth({ 'Content-Type': 'application/json' }),
      body: JSON.stringify({ part_number: partNumber, etag }),
    });
    if (!res.ok) await throwHttpError('上报分片失败', res);
  }

  async complete(uploadId: string): Promise<UploadCompleteResponse> {
    const res = await this.request(`${this.resolveBase()}/upload/${uploadId}/complete`, {
      method: 'POST',
      headers: withAuth(),
    });
    if (!res.ok) await throwHttpError('完成上传失败', res);
    const json = await res.json();
    const data = json.data || json;
    return {
      fileUrl: data.file_url ?? data.fileUrl,
      etag: data.etag,
    };
  }

  async getProgress(uploadId: string): Promise<UploadProgress> {
    const res = await this.request(`${this.resolveBase()}/upload/${uploadId}/progress`, {
      headers: withAuth(),
    });
    if (!res.ok) await throwHttpError('查询进度失败', res);
    const json = await res.json();
    const data = json.data || json;
    return {
      status: data.status,
      receivedChunks: data.received_chunks ?? data.receivedChunks ?? [],
      totalChunks: data.total_chunks ?? data.totalChunks,
      progressPct: data.progress_pct ?? data.progressPct,
    };
  }

  async abort(uploadId: string): Promise<void> {
    const res = await this.request(`${this.resolveBase()}/upload/${uploadId}`, {
      method: 'DELETE',
      headers: withAuth(),
    });
    if (!res.ok) await throwHttpError('取消上传失败', res);
  }
}

// ======================== UploadService（统一入口）=================

/**
 * 上传服务（统一入口）
 *
 * 只有一条通道：HTTP（`HttpUploadAdapter`）。保留类形式是为了隔离
 * 「分片直传 S3（不经过适配器）」与「后端 API 调用（带认证头）」两类请求。
 */
export class UploadService {
  private adapter: UploadAdapter;

  /**
   * @param options 可选：
   *   - 不传：基址由 `utils/apiBase.ts` 运行时解析（localStorage / env / 同源 `/api`）
   *   - 字符串 baseUrl（如 `'/api'` 或 `'https://host/api'`）：显式指定，
   *     仅供测试与嵌入场景；生产代码不要硬编码
   *   - `{ baseUrl? }`：同上，便于渐进迁移
   */
  constructor(options?: string | { baseUrl?: string }) {
    const baseUrl = typeof options === 'string' ? options : options?.baseUrl;
    this.adapter = new HttpUploadAdapter(baseUrl);
  }

  // ======================== 两步提交 API ========================

  async init(
    filename: string,
    fileSize: number,
    mimeType: string,
    options?: {
      fileGroupId?: string;
      changeReason?: string;
      fileMd5?: string;
    }
  ): Promise<InitUploadResponse> {
    return this.adapter.init(filename, fileSize, mimeType, options);
  }

  async startUpload(uploadId: string): Promise<StartUploadResponse> {
    return this.adapter.startUpload(uploadId);
  }

  async commit(
    uploadId: string,
    contextType: string,
    contextId: string
  ): Promise<void> {
    return this.adapter.commit(uploadId, contextType, contextId);
  }

  // ======================== 适配器路由 API ========================

  /** 上传单个分片到 S3 Presigned URL（始终使用 HTTP，不经过适配器） */
  async uploadChunk(
    presignedUrl: string,
    chunk: Blob,
    partNumber: number
  ): Promise<string> {
    const res = await fetch(presignedUrl, {
      method: 'PUT',
      body: chunk,
      headers: { 'Content-Length': chunk.size.toString() },
    });
    if (!res.ok) {
      // 尝试读取 S3 返回的错误 XML/文本，帮助调试
      let errorBody = '';
      try {
        errorBody = (await res.text()).slice(0, 1000);
      } catch {
        // 读取 body 失败时忽略
      }
      const detail = errorBody ? ` - ${errorBody}` : '';
      throw new Error(`分片 ${partNumber} 上传失败: HTTP ${res.status} ${res.statusText}${detail}`);
    }
    return res.headers.get('ETag') || '';
  }

  /** 上报分片上传完成 */
  async reportChunk(
    uploadId: string,
    partNumber: number,
    etag: string
  ): Promise<void> {
    return this.adapter.reportChunk(uploadId, partNumber, etag);
  }

  /** 查询上传进度 */
  async getProgress(uploadId: string): Promise<UploadProgress> {
    return this.adapter.getProgress(uploadId);
  }

  /** 完成分片上传 */
  async complete(uploadId: string): Promise<UploadCompleteResponse> {
    return this.adapter.complete(uploadId);
  }

  /** 取消上传 */
  async abort(uploadId: string): Promise<void> {
    return this.adapter.abort(uploadId);
  }

  // ======================== 版本管理 API ========================

  async getVersionHistory(fileGroupId: string): Promise<UploadVersionInfo[]> {
    return this.adapter.getVersionHistory(fileGroupId);
  }
}