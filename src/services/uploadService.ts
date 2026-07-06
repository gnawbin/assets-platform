/**
 * 大文件分片上传 API 服务
 *
 * 支持两种调用模式：
 * - tauri: 通过 Tauri invoke() 调用 Rust 命令（桌面版）
 * - http: 通过 HTTP fetch 调用后端 API（Web 版/兼容模式）
 *
 * S3 分片直传始终通过 HTTP（PUT presigned URL），与模式无关。
 */

import { invoke } from '@tauri-apps/api/core';

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

// ======================== 适配器类型 ========================

/** 调用模式 */
export type UploadAdapterMode = 'tauri' | 'http';

/** 适配器接口：定义上传服务需要的基础操作 */
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

// ======================== Tauri 适配器 ========================

class TauriUploadAdapter implements UploadAdapter {
  async init(
    filename: string,
    fileSize: number,
    mimeType: string,
    options?: { fileGroupId?: string; changeReason?: string; fileMd5?: string }
  ): Promise<InitUploadResponse> {
    const uploadId = await invoke<string>('upload_init', {
      filename,
      fileSize,
      mimeType,
      fileGroupId: options?.fileGroupId || null,
      changeReason: options?.changeReason || null,
      fileMd5: options?.fileMd5 || null,
    });
    return { uploadId, needStart: true };
  }

  async startUpload(uploadId: string): Promise<StartUploadResponse> {
    const result = await invoke<StartUploadResponse>('upload_start', {
      uploadId,
    });
    return result;
  }

  async commit(uploadId: string, contextType: string, contextId: string): Promise<void> {
    await invoke('upload_commit', {
      uploadId,
      contextType,
      contextId,
    });
  }

  async getVersionHistory(fileGroupId: string): Promise<UploadVersionInfo[]> {
    return invoke<UploadVersionInfo[]>('upload_get_version_history', {
      fileGroupId,
    });
  }

  /** 上报分片上传完成（通过 Tauri IPC） */
  async reportChunk(uploadId: string, partNumber: number, etag: string): Promise<void> {
    await invoke('upload_report_chunk', {
      uploadId,
      partNumber,
      etag,
    });
  }

  /** 完成上传合并分片（通过 Tauri IPC） */
  async complete(uploadId: string): Promise<UploadCompleteResponse> {
    const result = await invoke<{ fileUrl: string; etag: string }>('upload_complete', {
      uploadId,
    });
    return { fileUrl: result.fileUrl, etag: result.etag };
  }

  /** 获取上传进度（通过 Tauri IPC） */
  async getProgress(uploadId: string): Promise<UploadProgress> {
    const result = await invoke<{
      status: string;
      receivedChunks: number[];
      totalChunks: number;
      progressPct: number;
    }>('upload_get_progress', { uploadId });
    return {
      status: result.status,
      receivedChunks: result.receivedChunks,
      totalChunks: result.totalChunks,
      progressPct: result.progressPct,
    };
  }

  /** 取消上传（通过 Tauri IPC） */
  async abort(uploadId: string): Promise<void> {
    await invoke('upload_abort', { uploadId });
  }
}

// ======================== HTTP 适配器 ========================

class HttpUploadAdapter implements UploadAdapter {
  private baseUrl: string;

  constructor() {
    const envBaseUrl = typeof process !== 'undefined'
      ? process.env?.NEXT_PUBLIC_API_BASE_URL
      : undefined;
    this.baseUrl = envBaseUrl || 'http://localhost:3001/api';
  }

  async init(
    filename: string,
    fileSize: number,
    mimeType: string,
    options?: { fileGroupId?: string; changeReason?: string; fileMd5?: string }
  ): Promise<InitUploadResponse> {
    const res = await fetch(`${this.baseUrl}/upload/init`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        filename,
        file_size: fileSize,
        mime_type: mimeType,
        file_group_id: options?.fileGroupId,
        change_reason: options?.changeReason,
        file_md5: options?.fileMd5,
      }),
    });
    if (!res.ok) throw new Error(`初始化上传失败: ${res.statusText}`);
    const json = await res.json();
    const data = json.data || json;
    // HTTP 适配器返回 snake_case，转换为 camelCase
    return {
      uploadId: data.upload_id ?? data.uploadId,
      needStart: data.need_start ?? data.needStart ?? true,
    };
  }

  async startUpload(uploadId: string): Promise<StartUploadResponse> {
    const res = await fetch(`${this.baseUrl}/upload/${uploadId}/start`, {
      method: 'POST',
    });
    if (!res.ok) throw new Error(`开始上传失败: ${res.statusText}`);
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
    const res = await fetch(`${this.baseUrl}/upload/${uploadId}/commit`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ context_type: contextType, context_id: Number(contextId) }),
    });
    if (!res.ok) throw new Error(`提交上传失败: ${res.statusText}`);
  }

  async getVersionHistory(fileGroupId: string): Promise<UploadVersionInfo[]> {
    const res = await fetch(`${this.baseUrl}/upload/version-history/${fileGroupId}`);
    if (!res.ok) throw new Error(`获取版本历史失败: ${res.statusText}`);
    const json = await res.json();
    return json.data || json;
  }

  async reportChunk(uploadId: string, partNumber: number, etag: string): Promise<void> {
    const res = await fetch(`${this.baseUrl}/upload/${uploadId}/chunk`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ part_number: partNumber, etag }),
    });
    if (!res.ok) throw new Error(`上报分片失败: ${res.statusText}`);
  }

  async complete(uploadId: string): Promise<UploadCompleteResponse> {
    const res = await fetch(`${this.baseUrl}/upload/${uploadId}/complete`, {
      method: 'POST',
    });
    if (!res.ok) throw new Error(`完成上传失败: ${res.statusText}`);
    const json = await res.json();
    const data = json.data || json;
    return {
      fileUrl: data.file_url ?? data.fileUrl,
      etag: data.etag,
    };
  }

  async getProgress(uploadId: string): Promise<UploadProgress> {
    const res = await fetch(`${this.baseUrl}/upload/${uploadId}/progress`);
    if (!res.ok) throw new Error(`查询进度失败: ${res.statusText}`);
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
    const res = await fetch(`${this.baseUrl}/upload/${uploadId}`, {
      method: 'DELETE',
    });
    if (!res.ok) throw new Error(`取消上传失败: ${res.statusText}`);
  }
}

// ======================== UploadService（统一入口）=================

/**
 * 运行时检测是否应使用 Tauri 适配器
 * 避免在模块加载时（SSR/SSG）检测导致错误
 */
export function shouldUseTauri(mode?: UploadAdapterMode): boolean {
  if (mode) return mode === 'tauri';
  // 1. 运行在 Tauri 运行时中
  if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) return true;
  // 2. 环境变量（Next.js 构建时内联）
  const envAdapter = typeof process !== 'undefined'
    ? process.env?.NEXT_PUBLIC_API_ADAPTER
    : undefined;
  if (envAdapter === 'tauri') return true;
  if (envAdapter === 'http') return false;
  return false;
}

export class UploadService {
  private adapter: UploadAdapter;
  private baseUrl: string;

  /**
   * @param mode 可选，强制指定模式。不传则按优先级自动选择：
   *   1. 运行时检测 Tauri 环境
   *   2. NEXT_PUBLIC_API_ADAPTER 环境变量
   *   3. HTTP 兜底
   */
  constructor(mode?: UploadAdapterMode) {
    const useTauri = shouldUseTauri(mode);
    this.adapter = useTauri
      ? new TauriUploadAdapter()
      : new HttpUploadAdapter();
    this.baseUrl = (() => {
      const envBaseUrl = typeof process !== 'undefined'
        ? process.env?.NEXT_PUBLIC_API_BASE_URL
        : undefined;
      return envBaseUrl || 'http://localhost:3001/api';
    })();
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
    if (!res.ok) throw new Error(`分片 ${partNumber} 上传失败: ${res.statusText}`);
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