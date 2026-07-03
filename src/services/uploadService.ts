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
        fileSize,
        mimeType,
        fileGroupId: options?.fileGroupId,
        changeReason: options?.changeReason,
        fileMd5: options?.fileMd5,
      }),
    });
    if (!res.ok) throw new Error(`初始化上传失败: ${res.statusText}`);
    const json = await res.json();
    return json.data || json; // HTTP 适配器返回 { code, message, data }
  }

  async startUpload(uploadId: string): Promise<StartUploadResponse> {
    const res = await fetch(`${this.baseUrl}/upload/${uploadId}/start`, {
      method: 'POST',
    });
    if (!res.ok) throw new Error(`开始上传失败: ${res.statusText}`);
    const json = await res.json();
    return json.data || json;
  }

  async commit(uploadId: string, contextType: string, contextId: string): Promise<void> {
    const res = await fetch(`${this.baseUrl}/upload/${uploadId}/commit`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ contextType, contextId }),
    });
    if (!res.ok) throw new Error(`提交上传失败: ${res.statusText}`);
  }

  async getVersionHistory(fileGroupId: string): Promise<UploadVersionInfo[]> {
    const res = await fetch(`${this.baseUrl}/upload/version-history/${fileGroupId}`);
    if (!res.ok) throw new Error(`获取版本历史失败: ${res.statusText}`);
    const json = await res.json();
    return json.data || json;
  }
}

// ======================== UploadService（统一入口）=================

/**
 * 自动检测 Tauri 环境的函数
 */
function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export class UploadService {
  private adapter: UploadAdapter;
  private baseUrl: string;

  /**
   * @param mode 可选，强制指定模式。不传则自动检测：
   *   - Tauri 桌面环境 → 使用 invoke
   *   - 浏览器环境 → 使用 HTTP
   */
  constructor(mode?: UploadAdapterMode) {
    const useTauri = mode ? mode === 'tauri' : isTauri();
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

  // ======================== S3 直传 API ========================

  /** 上传单个分片到 S3 Presigned URL（始终使用 HTTP） */
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
    const res = await fetch(
      `${this.baseUrl}/upload/${uploadId}/chunk`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ partNumber, etag } satisfies ReportChunkRequest),
      }
    );
    if (!res.ok) throw new Error(`上报分片失败: ${res.statusText}`);
  }

  /** 查询上传进度 */
  async getProgress(uploadId: string): Promise<UploadProgress> {
    const res = await fetch(
      `${this.baseUrl}/upload/${uploadId}/progress`
    );
    if (!res.ok) throw new Error(`查询进度失败: ${res.statusText}`);
    return res.json();
  }

  /** 完成分片上传 */
  async complete(uploadId: string): Promise<UploadCompleteResponse> {
    const res = await fetch(
      `${this.baseUrl}/upload/${uploadId}/complete`,
      { method: 'POST' }
    );
    if (!res.ok) throw new Error(`完成上传失败: ${res.statusText}`);
    return res.json();
  }

  /** 取消上传 */
  async abort(uploadId: string): Promise<void> {
    const res = await fetch(
      `${this.baseUrl}/upload/${uploadId}`,
      { method: 'DELETE' }
    );
    if (!res.ok) throw new Error(`取消上传失败: ${res.statusText}`);
  }

  // ======================== 版本管理 API ========================

  async getVersionHistory(fileGroupId: string): Promise<UploadVersionInfo[]> {
    return this.adapter.getVersionHistory(fileGroupId);
  }
}