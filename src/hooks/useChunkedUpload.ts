/**
 * 大文件分片上传 Hook
 *
 * 纯逻辑 Hook，不依赖任何 UI 框架。
 * 管理上传状态机，支持：
 * - 并发上传（可配置并发数）
 * - 暂停/继续
 * - 断点续传（localStorage 持久化 upload_id）
 * - 取消上传
 * - 重试
 * - 上传速度计算
 */

import { useState, useRef, useCallback, useEffect } from 'react';
import { UploadService, type UploadCompleteResponse } from '@/services/uploadService';

// ======================== 类型定义 ========================

export type UploadStatus = 'idle' | 'uploading' | 'paused' | 'completed' | 'error';

export interface UseChunkedUploadOptions {
  /** 并发上传数，默认 3 */
  concurrency?: number;
  /** 是否自动断点续传，默认 true */
  autoResume?: boolean;
  /** localStorage 存储 key，用于断点续传 */
  storageKey?: string;
  /** 进度回调 */
  onProgress?: (pct: number) => void;
  /** 完成回调 */
  onComplete?: (result: UploadCompleteResponse) => void;
  /** 错误回调 */
  onError?: (err: string) => void;
}

export interface UseChunkedUploadReturn {
  /** 上传进度 0-100 */
  progress: number;
  /** 当前状态 */
  status: UploadStatus;
  /** 错误信息 */
  error: string | null;
  /** 已上传字节数 */
  uploadedBytes: number;
  /** 总字节数 */
  totalBytes: number;
  /** 上传速度（字节/秒） */
  speed: number;

  /** 开始上传 */
  start: (file: File) => Promise<void>;
  /** 暂停上传 */
  pause: () => void;
  /** 继续上传（断点续传） */
  resume: () => Promise<void>;
  /** 取消上传 */
  cancel: () => Promise<void>;
  /** 重试上传 */
  retry: () => Promise<void>;
}

// ======================== 工具函数 ========================

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec === 0) return '0 B/s';
  const k = 1024;
  const sizes = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
  const i = Math.floor(Math.log(bytesPerSec) / Math.log(k));
  return parseFloat((bytesPerSec / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

// ======================== Hook ========================

export function useChunkedUpload(
  options?: UseChunkedUploadOptions
): UseChunkedUploadReturn {
  const {
    concurrency = 3,
    autoResume = true,
    storageKey = 'chunked_upload_id',
    onProgress,
    onComplete,
    onError,
  } = options || {};

  const [progress, setProgress] = useState(0);
  const [status, setStatus] = useState<UploadStatus>('idle');
  const [error, setError] = useState<string | null>(null);
  const [uploadedBytes, setUploadedBytes] = useState(0);
  const [totalBytes, setTotalBytes] = useState(0);
  const [speed, setSpeed] = useState(0);

  const uploadServiceRef = useRef(new UploadService());
  const abortControllerRef = useRef<AbortController | null>(null);
  const fileRef = useRef<File | null>(null);
  const uploadIdRef = useRef<string | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const chunkSizeRef = useRef(0);
  const pausedRef = useRef(false);
  const speedBytesRef = useRef(0);

  // 清理 localStorage 中的 upload_id
  const clearStorageId = useCallback(() => {
    if (typeof window !== 'undefined') {
      localStorage.removeItem(storageKey);
    }
  }, [storageKey]);

  // 保存 upload_id 到 localStorage
  const saveStorageId = useCallback((id: string) => {
    if (typeof window !== 'undefined') {
      localStorage.setItem(storageKey, id);
    }
  }, [storageKey]);

  // 从 localStorage 获取 upload_id
  const getStorageId = useCallback((): string | null => {
    if (typeof window !== 'undefined') {
      return localStorage.getItem(storageKey);
    }
    return null;
  }, [storageKey]);

  // 暂停上传
  const pause = useCallback(() => {
    pausedRef.current = true;
    abortControllerRef.current?.abort();
    setStatus('paused');
  }, []);

  // 取消上传
  const cancel = useCallback(async () => {
    pausedRef.current = true;
    abortControllerRef.current?.abort();

    // 通知后端取消上传
    if (uploadIdRef.current) {
      try {
        await uploadServiceRef.current.abort(uploadIdRef.current);
      } catch {
        // 忽略取消失败
      }
    }

    clearStorageId();
    setStatus('idle');
    setProgress(0);
    setUploadedBytes(0);
    setSpeed(0);
    setError(null);
    fileRef.current = null;
    uploadIdRef.current = null;
    chunksRef.current = [];
  }, [clearStorageId]);

  // 核心上传逻辑
  const startUpload = useCallback(async (file: File, existingUploadId?: string) => {
    const uploadService = uploadServiceRef.current;
    pausedRef.current = false;
    abortControllerRef.current = new AbortController();

    fileRef.current = file;
    setTotalBytes(file.size);
    setStatus('uploading');
    setError(null);

    try {
      let uploadId: string;
      let presignedUrls: string[];
      let chunkSize: number;
      let totalChunks: number;

      if (existingUploadId) {
        // 断点续传：查询进度，只传缺失分片
        const progressData = await uploadService.getProgress(existingUploadId);

        if (progressData.status === 'completed') {
          // 已经上传完成
          setProgress(100);
          setUploadedBytes(file.size);
          setStatus('completed');
          onComplete?.({
            fileUrl: '',
            etag: '',
          });
          clearStorageId();
          return;
        }

        // 重新初始化获取 presigned URLs
        const initResp = await uploadService.init(file.name, file.size, file.type);
        uploadId = initResp.uploadId;
        chunkSize = initResp.chunkSize;
        totalChunks = initResp.totalChunks;
        presignedUrls = initResp.presignedUrls;

        // 只上传缺失的分片
        const uploadedSet = new Set(progressData.receivedChunks);
        const missingIndices: number[] = [];
        for (let i = 1; i <= totalChunks; i++) {
          if (!uploadedSet.has(i)) {
            missingIndices.push(i);
          }
        }

        // 分片
        const chunks: Blob[] = [];
        for (let start = 0; start < file.size; start += chunkSize) {
          chunks.push(file.slice(start, Math.min(start + chunkSize, file.size)));
        }
        chunksRef.current = chunks;
        chunkSizeRef.current = chunkSize;

        // 并发上传缺失分片
        let uploadedCount = progressData.receivedChunks.length;
        let lastLoaded = uploadedCount * chunkSize;
        let lastTime = Date.now();

        const uploadOneChunk = async (partNumber: number): Promise<void> => {
          if (pausedRef.current) return;
          const presignedUrl = presignedUrls[partNumber - 1];
          const chunk = chunks[partNumber - 1];

          const etag = await uploadService.uploadChunk(presignedUrl, chunk, partNumber);
          await uploadService.reportChunk(uploadId, partNumber, etag);

          uploadedCount++;
          const pct = Math.round((uploadedCount / totalChunks) * 100);
          setProgress(pct);
          setUploadedBytes(uploadedCount * chunkSize);
          onProgress?.(pct);

          // 计算速度
          const now = Date.now();
          const elapsed = (now - lastTime) / 1000;
          if (elapsed > 0.5) {
            const currentLoaded = uploadedCount * chunkSize;
            const bytesPerSec = (currentLoaded - lastLoaded) / elapsed;
            setSpeed(bytesPerSec);
            speedBytesRef.current = bytesPerSec;
            lastLoaded = currentLoaded;
            lastTime = now;
          }
        };

        // 并发控制
        const workers = [];
        for (let i = 0; i < concurrency; i++) {
          workers.push((async () => {
            for (let j = i; j < missingIndices.length; j += concurrency) {
              if (pausedRef.current) break;
              await uploadOneChunk(missingIndices[j]);
            }
          })());
        }
        await Promise.all(workers);

      } else {
        // 新上传
        const initResp = await uploadService.init(file.name, file.size, file.type);
        uploadId = initResp.uploadId;
        chunkSize = initResp.chunkSize;
        totalChunks = initResp.totalChunks;
        presignedUrls = initResp.presignedUrls;

        // 保存 upload_id 用于断点续传
        uploadIdRef.current = uploadId;
        saveStorageId(uploadId);

        // 分片
        const chunks: Blob[] = [];
        for (let start = 0; start < file.size; start += chunkSize) {
          chunks.push(file.slice(start, Math.min(start + chunkSize, file.size)));
        }
        chunksRef.current = chunks;
        chunkSizeRef.current = chunkSize;

        // 并发上传
        let uploadedCount = 0;
        let lastLoaded = 0;
        let lastTime = Date.now();

        const uploadOneChunk = async (partNumber: number): Promise<void> => {
          if (pausedRef.current) return;
          const presignedUrl = presignedUrls[partNumber - 1];
          const chunk = chunks[partNumber - 1];

          const etag = await uploadService.uploadChunk(presignedUrl, chunk, partNumber);
          await uploadService.reportChunk(uploadId, partNumber, etag);

          uploadedCount++;
          const pct = Math.round((uploadedCount / totalChunks) * 100);
          setProgress(pct);
          setUploadedBytes(uploadedCount * chunkSize);
          onProgress?.(pct);

          // 计算速度
          const now = Date.now();
          const elapsed = (now - lastTime) / 1000;
          if (elapsed > 0.5) {
            const currentLoaded = uploadedCount * chunkSize;
            const bytesPerSec = (currentLoaded - lastLoaded) / elapsed;
            setSpeed(bytesPerSec);
            speedBytesRef.current = bytesPerSec;
            lastLoaded = currentLoaded;
            lastTime = now;
          }
        };

        // 并发控制
        const workers = [];
        for (let i = 0; i < concurrency; i++) {
          workers.push((async () => {
            for (let j = i; j < totalChunks; j += concurrency) {
              if (pausedRef.current) break;
              await uploadOneChunk(j + 1);
            }
          })());
        }
        await Promise.all(workers);
      }

      // 如果被暂停了，不执行 complete
      if (pausedRef.current) {
        return;
      }

      // 完成合并
      const result = await uploadService.complete(uploadId);
      setProgress(100);
      setUploadedBytes(file.size);
      setStatus('completed');
      clearStorageId();
      onComplete?.(result);

    } catch (err: any) {
      if (pausedRef.current) {
        // 暂停导致的错误，不处理
        return;
      }
      const errMsg = err.message || '上传失败';
      setError(errMsg);
      setStatus('error');
      onError?.(errMsg);
    }
  }, [concurrency, saveStorageId, clearStorageId, onProgress, onComplete, onError]);

  // 开始上传
  const start = useCallback(async (file: File) => {
    // 检查是否有未完成的断点续传
    if (autoResume) {
      const savedId = getStorageId();
      if (savedId) {
        try {
          const progressData = await uploadServiceRef.current.getProgress(savedId);
          if (progressData.status === 'completed') {
            // 已经上传完成，直接设置完成状态
            setProgress(100);
            setUploadedBytes(file.size);
            setStatus('completed');
            clearStorageId();
            onComplete?.({
              fileUrl: '',
              etag: '',
            });
            return;
          }
          if (progressData.status === 'uploading') {
            // 有未完成的上传，继续
            await startUpload(file, savedId);
            return;
          }
        } catch {
          // 查询失败，重新开始
          clearStorageId();
        }
      }
    }

    await startUpload(file);
  }, [autoResume, getStorageId, clearStorageId, startUpload, onComplete]);

  // 继续上传（断点续传）
  const resume = useCallback(async () => {
    if (!fileRef.current) return;
    const savedId = getStorageId();
    if (savedId) {
      await startUpload(fileRef.current, savedId);
    } else {
      await startUpload(fileRef.current);
    }
  }, [getStorageId, startUpload]);

  // 重试
  const retry = useCallback(async () => {
    if (!fileRef.current) return;
    clearStorageId();
    setError(null);
    setProgress(0);
    setUploadedBytes(0);
    setSpeed(0);
    await startUpload(fileRef.current);
  }, [clearStorageId, startUpload]);

  // 组件卸载时清理
  useEffect(() => {
    return () => {
      abortControllerRef.current?.abort();
    };
  }, []);

  return {
    progress,
    status,
    error,
    uploadedBytes,
    totalBytes,
    speed,
    start,
    pause,
    resume,
    cancel,
    retry,
  };
}
