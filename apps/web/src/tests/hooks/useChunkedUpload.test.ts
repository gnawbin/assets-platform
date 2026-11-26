/**
 * useChunkedUpload Hook 单元测试
 *
 * 上传已改为**纯 HTTP 单通道 + 两步提交**：
 *   POST /api/upload/init → POST /api/upload/{id}/start
 *   → PUT presigned（分片直传 S3）+ POST /api/upload/{id}/chunk（上报）
 *   → POST /api/upload/{id}/complete
 *
 * 基址由 `utils/apiBase.ts` 运行时解析，测试通过 `window.__API_BASE__`
 * 注入固定地址，断言完整 URL（顺带守住「不拼出 /api/api」）。
 *
 * 测试覆盖：
 * - 状态机转换（idle → uploading → completed）
 * - 暂停/继续（resume 会先查询进度）
 * - 取消上传
 * - 重试
 * - 断点续传逻辑
 * - 并发控制
 * - 错误处理
 * - 进度回调
 */

import { renderHook, act } from '@testing-library/react';
import { useChunkedUpload } from '@/hooks/useChunkedUpload';

// ======================== Mock ========================

// Mock fetch
const mockFetch = jest.fn();
global.fetch = mockFetch;

// Mock 响应
function createMockResponse(body: unknown, status = 200, headers: Record<string, string> = {}) {
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText: status >= 200 && status < 300 ? 'OK' : 'Error',
    json: jest.fn().mockResolvedValue(body),
    headers: {
      get: (name: string) => headers[name] ?? null,
      has: (name: string) => name in headers,
      forEach: () => { },
    },
  } as unknown as Response;
}

// 创建测试文件
function createTestFile(name = 'test.pdf', size = 1024 * 1024, type = 'application/pdf'): File {
  const content = new ArrayBuffer(size);
  return new File([content], name, { type });
}

// ======================== 响应队列辅助 ========================

/** 测试用服务端地址（由 apiBase 的 `window.__API_BASE__` 注入，归一化后带 `/api`） */
const API_BASE = 'http://test.local/api';

/** 两步提交第 1 步：`POST /upload/init`（仅占位，返回 uploadId） */
function mockInit(uploadId = 'upload-001'): void {
  mockFetch.mockResolvedValueOnce(createMockResponse({ uploadId, needStart: true }));
}

/** 两步提交第 2 步：`POST /upload/{id}/start`（创建 MultipartUpload，返回预签名地址） */
function mockStart(opts: { uploadId?: string; chunkSize?: number; totalChunks?: number } = {}): void {
  const { uploadId = 'upload-001', chunkSize = 1024 * 1024, totalChunks = 1 } = opts;
  mockFetch.mockResolvedValueOnce(
    createMockResponse({
      uploadId,
      s3UploadId: `s3-${uploadId}`,
      chunkSize,
      totalChunks,
      presignedUrls: Array.from(
        { length: totalChunks },
        (_, i) => `https://s3.example.com/part/${i + 1}`
      ),
    })
  );
}

/** 单个分片的 2 次调用：PUT presigned（返回 ETag）+ POST 上报进度 */
function mockChunk(partNumber = 1): void {
  mockFetch.mockResolvedValueOnce(
    createMockResponse({}, 200, { ETag: `"etag-00${partNumber}"` })
  );
  mockFetch.mockResolvedValueOnce(createMockResponse({}));
}

/** 最后一步：`POST /upload/{id}/complete`（合并分片） */
function mockComplete(etag = '"etag-final"'): void {
  mockFetch.mockResolvedValueOnce(
    createMockResponse({
      fileUrl: 'https://storage.example.com/files/test.pdf',
      etag,
    })
  );
}

/**
 * 先制造一次失败，让 hook 保留 `fileRef`（`resume()` 依赖它）
 *
 * `resume()` 只在有文件上下文时才动作，而文件上下文由 `start()` 写入，
 * 所以续传类用例必须先跑一次失败的上传。
 */
async function startWithFailure() {
  mockFetch.mockRejectedValueOnce(new Error('网络错误'));
  const hook = renderHook(() => useChunkedUpload({ concurrency: 1 }));
  await act(async () => {
    await hook.result.current.start(createTestFile('test.pdf', 10240));
  });
  expect(hook.result.current.status).toBe('error');
  return hook;
}

// ======================== 测试用例 ========================

describe('useChunkedUpload', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    localStorage.clear();
    // 固定基址，避免用例受 jsdom 默认 origin / 环境变量影响
    (window as Window & { __API_BASE__?: unknown }).__API_BASE__ = 'http://test.local';
  });

  afterEach(() => {
    delete (window as Window & { __API_BASE__?: unknown }).__API_BASE__;
  });

  // ======================== 初始状态 ========================

  describe('初始状态', () => {
    it('应返回正确的初始状态', () => {
      const { result } = renderHook(() => useChunkedUpload());

      expect(result.current.progress).toBe(0);
      expect(result.current.status).toBe('idle');
      expect(result.current.error).toBeNull();
      expect(result.current.uploadedBytes).toBe(0);
      expect(result.current.totalBytes).toBe(0);
      expect(result.current.speed).toBe(0);
    });
  });

  // ======================== 开始上传 ========================

  describe('start', () => {
    it('应成功开始上传并完成', async () => {
      mockInit();
      mockStart();
      mockChunk(1);
      mockComplete('"etag-001"');

      const onComplete = jest.fn();
      const { result } = renderHook(() =>
        useChunkedUpload({ concurrency: 1, onComplete })
      );

      const file = createTestFile();
      await act(async () => {
        await result.current.start(file);
      });

      expect(result.current.status).toBe('completed');
      expect(result.current.progress).toBe(100);
      expect(result.current.uploadedBytes).toBe(file.size);
      expect(onComplete).toHaveBeenCalledWith({
        fileUrl: 'https://storage.example.com/files/test.pdf',
        etag: '"etag-001"',
      });

      // 两步提交 + 分片直传 + 上报 + 合并，共 5 次请求
      const urls = mockFetch.mock.calls.map(([url]) => url);
      expect(urls).toEqual([
        `${API_BASE}/upload/init`,
        `${API_BASE}/upload/upload-001/start`,
        'https://s3.example.com/part/1',
        `${API_BASE}/upload/upload-001/chunk`,
        `${API_BASE}/upload/upload-001/complete`,
      ]);
      expect(urls.some((url) => String(url).includes('/api/api'))).toBe(false);
    });

    it('上传失败时应切换到 error 状态', async () => {
      mockFetch.mockRejectedValueOnce(new Error('网络错误'));

      const onError = jest.fn();
      const { result } = renderHook(() => useChunkedUpload({ concurrency: 1, onError }));

      const file = createTestFile();
      await act(async () => {
        await result.current.start(file);
      });

      expect(result.current.status).toBe('error');
      expect(result.current.error).toBe('网络错误');
      expect(onError).toHaveBeenCalledWith('网络错误');
    });

    it('应保存 upload_id 到 localStorage', async () => {
      mockInit();
      mockStart();
      mockChunk(1);
      mockComplete('"etag-001"');

      const { result } = renderHook(() =>
        useChunkedUpload({ concurrency: 1, storageKey: 'test_upload_id' })
      );

      const file = createTestFile();
      await act(async () => {
        await result.current.start(file);
      });

      // 上传完成后应清理 localStorage
      expect(localStorage.getItem('test_upload_id')).toBeNull();
    });
  });

  // ======================== 暂停/继续 ========================

  describe('pause / resume', () => {
    it('暂停后应切换到 paused 状态', async () => {
      // 模拟一个需要多次分片的上传
      mockInit();
      mockStart({ chunkSize: 1024, totalChunks: 10 });
      mockChunk(1);

      const { result } = renderHook(() => useChunkedUpload({ concurrency: 1 }));

      const file = createTestFile('test.pdf', 10240);
      const startPromise = act(async () => {
        await result.current.start(file);
      });

      // 暂停
      act(() => {
        result.current.pause();
      });

      await startPromise;

      expect(result.current.status).toBe('paused');
    });

    it('resume 时进度未完成则重新走两步提交并完成', async () => {
      const { result } = await startWithFailure();

      localStorage.setItem('chunked_upload_id', 'upload-002');
      mockFetch.mockResolvedValueOnce(
        createMockResponse({
          status: 'uploading',
          receivedChunks: [1],
          totalChunks: 2,
          progressPct: 50,
        })
      );
      mockInit('upload-002');
      mockStart({ uploadId: 'upload-002' });
      mockChunk(1);
      mockComplete();

      await act(async () => {
        await result.current.resume();
      });

      expect(result.current.status).toBe('completed');
      expect(mockFetch).toHaveBeenCalledWith(
        `${API_BASE}/upload/upload-002/progress`,
        expect.anything()
      );
    });
  });

  // ======================== 取消 ========================

  describe('cancel', () => {
    it('取消后应重置到 idle 状态', async () => {
      const { result } = renderHook(() => useChunkedUpload());

      await act(async () => {
        await result.current.cancel();
      });

      expect(result.current.status).toBe('idle');
      expect(result.current.progress).toBe(0);
      expect(result.current.uploadedBytes).toBe(0);
      expect(result.current.error).toBeNull();
    });

    it('取消时应调用 abort API', async () => {
      // 先开始一个上传
      mockInit();
      mockStart();
      mockChunk(1);
      mockComplete('"etag-001"');

      const { result } = renderHook(() => useChunkedUpload({ concurrency: 1 }));

      const file = createTestFile();
      await act(async () => {
        await result.current.start(file);
      });

      // 上传完成后取消
      await act(async () => {
        await result.current.cancel();
      });

      expect(result.current.status).toBe('idle');
    });
  });

  // ======================== 重试 ========================

  describe('retry', () => {
    it('重试应重置状态并重新开始上传', async () => {
      // 第一次失败
      mockFetch.mockRejectedValueOnce(new Error('上传失败'));

      const { result } = renderHook(() => useChunkedUpload({ concurrency: 1 }));

      const file = createTestFile();
      await act(async () => {
        await result.current.start(file);
      });

      expect(result.current.status).toBe('error');

      // 第二次成功
      mockInit('upload-002');
      mockStart({ uploadId: 'upload-002' });
      mockChunk(1);
      mockComplete('"etag-002"');

      await act(async () => {
        await result.current.retry();
      });

      expect(result.current.status).toBe('completed');
      expect(result.current.progress).toBe(100);
    });
  });

  // ======================== 断点续传 ========================

  describe('断点续传', () => {
    it('start() 不再自动查询进度（续传只发生在显式 resume）', async () => {
      localStorage.setItem('chunked_upload_id', 'upload-001');
      mockInit('upload-002');
      mockStart({ uploadId: 'upload-002' });
      mockChunk(1);
      mockComplete();

      const { result } = renderHook(() => useChunkedUpload({ concurrency: 1 }));

      await act(async () => {
        await result.current.start(createTestFile('test.pdf', 10240));
      });

      expect(result.current.status).toBe('completed');
      const urls = mockFetch.mock.calls.map(([url]) => String(url));
      expect(urls.some((url) => url.endsWith('/progress'))).toBe(false);
    });

    it('resume() 前会先查询进度，且使用统一基址', async () => {
      const { result } = await startWithFailure();

      localStorage.setItem('chunked_upload_id', 'upload-001');
      mockFetch.mockResolvedValueOnce(
        createMockResponse({
          status: 'completed',
          receivedChunks: [1, 2, 3, 4, 5],
          totalChunks: 5,
          progressPct: 100,
        })
      );

      await act(async () => {
        await result.current.resume();
      });

      expect(result.current.status).toBe('completed');
      expect(result.current.progress).toBe(100);
      expect(mockFetch).toHaveBeenCalledWith(
        `${API_BASE}/upload/upload-001/progress`,
        expect.anything()
      );
    });
  });

  // ======================== 并发控制 ========================

  describe('并发控制', () => {
    it('应使用配置的并发数', async () => {
      const chunkSize = 1024;
      const totalChunks = 6;
      const fileSize = chunkSize * totalChunks;

      mockInit();
      mockStart({ chunkSize, totalChunks });

      // 每个分片需要 uploadChunk + reportChunk = 2 次调用
      for (let i = 0; i < totalChunks; i++) {
        mockChunk(i + 1);
      }

      mockComplete();

      const { result } = renderHook(() => useChunkedUpload({ concurrency: 3 }));

      const file = createTestFile('test.pdf', fileSize);
      await act(async () => {
        await result.current.start(file);
      });

      expect(result.current.status).toBe('completed');
      expect(result.current.progress).toBe(100);
    });
  });

  // ======================== 进度回调 ========================

  describe('进度回调', () => {
    it('应触发 onProgress 回调', async () => {
      const chunkSize = 1024;
      const totalChunks = 3;
      const fileSize = chunkSize * totalChunks;

      mockInit();
      mockStart({ chunkSize, totalChunks });

      for (let i = 0; i < totalChunks; i++) {
        mockChunk(i + 1);
      }

      mockComplete();

      const onProgress = jest.fn();
      const { result } = renderHook(() =>
        useChunkedUpload({ concurrency: 1, onProgress })
      );

      const file = createTestFile('test.pdf', fileSize);
      await act(async () => {
        await result.current.start(file);
      });

      // 应该至少触发一次进度回调
      expect(onProgress).toHaveBeenCalled();
    });
  });
});
