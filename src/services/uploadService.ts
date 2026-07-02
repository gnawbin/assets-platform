/**
 * 大文件分片上传 API 服务
 *
 * 封装 S3 原生分片上传的 5 个核心接口。
 * 通过 HTTP fetch 直接调用后端 API（上传接口只有 HTTP 路由，没有 Tauri 命令）。
 * - init: 初始化分片上传
 * - uploadChunk: 上传单个分片到 S3 Presigned URL
 * - reportChunk: 上报分片完成
 * - getProgress: 查询上传进度
 * - complete: 完成合并
 * - abort: 取消上传
 */

// ======================== 类型定义 ========================

export interface InitUploadResponse {
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

// ======================== UploadService ========================

export class UploadService {
  private baseUrl: string;

  constructor() {
    // 上传接口只有 HTTP 路由（无 Tauri 命令），直接调用后端 HTTP API
    const envBaseUrl = typeof process !== 'undefined'
      ? process.env?.NEXT_PUBLIC_API_BASE_URL
      : undefined;
    this.baseUrl = envBaseUrl || 'http://localhost:3001/api';
  }

  /** 初始化分片上传 */
  async init(
    filename: string,
    fileSize: number,
    mimeType: string
  ): Promise<InitUploadResponse> {
    const res = await fetch(`${this.baseUrl}/upload/init`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ filename, fileSize, mimeType }),
    });
    if (!res.ok) throw new Error(`初始化上传失败: ${res.statusText}`);
    return res.json();
  }

  /** 上传单个分片到 S3 Presigned URL */
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
    const res = await fetch(`${this.baseUrl}/upload/${uploadId}/chunk`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ partNumber, etag } satisfies ReportChunkRequest),
    });
    if (!res.ok) throw new Error(`上报分片失败: ${res.statusText}`);
  }

  /** 查询上传进度 */
  async getProgress(uploadId: string): Promise<UploadProgress> {
    const res = await fetch(`${this.baseUrl}/upload/${uploadId}/progress`);
    if (!res.ok) throw new Error(`查询进度失败: ${res.statusText}`);
    return res.json();
  }

  /** 完成分片上传 */
  async complete(uploadId: string): Promise<UploadCompleteResponse> {
    const res = await fetch(`${this.baseUrl}/upload/${uploadId}/complete`, {
      method: 'POST',
    });
    if (!res.ok) throw new Error(`完成上传失败: ${res.statusText}`);
    return res.json();
  }

  /** 取消上传 */
  async abort(uploadId: string): Promise<void> {
    const res = await fetch(`${this.baseUrl}/upload/${uploadId}`, {
      method: 'DELETE',
    });
    if (!res.ok) throw new Error(`取消上传失败: ${res.statusText}`);
  }
}