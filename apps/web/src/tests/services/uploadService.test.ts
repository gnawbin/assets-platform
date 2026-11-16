/**
 * UploadService 单元测试
 *
 * 测试覆盖：
 * - init: 初始化分片上传
 * - uploadChunk: 上传单个分片
 * - reportChunk: 上报分片完成
 * - getProgress: 查询上传进度
 * - complete: 完成合并
 * - abort: 取消上传
 * - 错误处理
 */

import { UploadService } from '@/services/uploadService';

// ======================== Mock 数据 ========================

const mockInitResponse = {
  uploadId: 'upload-001',
  needStart: true,
};

const mockStartResponse = {
  uploadId: 'upload-001',
  s3UploadId: 's3-upload-001',
  chunkSize: 5242880,
  totalChunks: 10,
  presignedUrls: [
    'https://s3.example.com/part/1',
    'https://s3.example.com/part/2',
    'https://s3.example.com/part/3',
  ],
};

const mockProgressResponse = {
  status: 'uploading',
  receivedChunks: [1, 2, 3],
  totalChunks: 10,
  progressPct: 30,
};

const mockCompleteResponse = {
  fileUrl: 'https://storage.example.com/files/report.pdf',
  etag: '"abc123def456"',
};

// ======================== 工具函数 ========================

function createMockResponse(body: unknown, status = 200, headers: Record<string, string> = {}) {
  // 将 headers 键统一转为小写，以便 case-insensitive 查找
  const normalizedHeaders: Record<string, string> = {};
  for (const [key, value] of Object.entries(headers)) {
    normalizedHeaders[key.toLowerCase()] = value;
  }
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText: status >= 200 && status < 300 ? 'OK' : 'Error',
    json: jest.fn().mockResolvedValue(body),
    text: jest.fn().mockResolvedValue(typeof body === 'string' ? body : JSON.stringify(body)),
    headers: {
      get: (name: string) => normalizedHeaders[name.toLowerCase()] ?? null,
      has: (name: string) => name.toLowerCase() in normalizedHeaders,
      forEach: () => { },
    },
  } as unknown as Response;
}

// ======================== 测试用例 ========================

describe('UploadService', () => {
  let service: UploadService;

  beforeEach(() => {
    service = new UploadService('/api');
    (global.fetch as jest.Mock).mockClear();
    // 令牌由 authStore 写入 localStorage；每个用例默认未登录
    localStorage.clear();
  });

  // ======================== init ========================

  describe('init', () => {
    it('应成功初始化分片上传', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse(mockInitResponse));

      const result = await service.init('report.pdf', 52428800, 'application/pdf');

      expect(result).toEqual(mockInitResponse);
      expect(global.fetch).toHaveBeenCalledWith('/api/upload/init', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          filename: 'report.pdf',
          file_size: 52428800,
          mime_type: 'application/pdf',
        }),
      });
    });

    it('初始化失败时应抛出错误', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse({}, 500));

      await expect(service.init('test.pdf', 1000, 'text/plain')).rejects.toThrow(
        '初始化上传失败'
      );
    });

    it('应正确处理超大文件', async () => {
      const largeResponse = {
        ...mockStartResponse,
        totalChunks: 2000,
        presignedUrls: Array(2000).fill('https://s3.example.com/part/1'),
      };
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse(largeResponse));

      const result = await service.startUpload('upload-001');

      expect(result.totalChunks).toBe(2000);
      expect(result.presignedUrls).toHaveLength(2000);
    });
  });

  // ======================== startUpload ========================

  describe('startUpload', () => {
    it('应成功开始上传并返回 S3 分片信息', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse(mockStartResponse));

      const result = await service.startUpload('upload-001');

      expect(result).toEqual(mockStartResponse);
      expect(global.fetch).toHaveBeenCalledWith('/api/upload/upload-001/start', {
        method: 'POST',
        // 未登录时 withAuth() 返回空对象（后端会统一返回 401）
        headers: {},
      });
    });
  });

  // ======================== uploadChunk ========================

  describe('uploadChunk', () => {
    it('应成功上传分片并返回 ETag', async () => {
      const mockEtag = '"etag-001"';
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(
        createMockResponse({}, 200, { etag: mockEtag })
      );

      const chunk = new Blob(['test data'], { type: 'application/octet-stream' });
      const etag = await service.uploadChunk('https://s3.example.com/part/1', chunk, 1);

      expect(etag).toBe(mockEtag);
      expect(global.fetch).toHaveBeenCalledWith('https://s3.example.com/part/1', {
        method: 'PUT',
        body: chunk,
        headers: { 'Content-Length': chunk.size.toString() },
      });
    });

    it('分片上传失败时应抛出错误', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse({}, 403));

      const chunk = new Blob(['test']);
      await expect(service.uploadChunk('https://s3.example.com/part/1', chunk, 1)).rejects.toThrow(
        '分片 1 上传失败'
      );
    });

    it('ETag 为空时应返回空字符串', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse({}));

      const chunk = new Blob(['test']);
      const etag = await service.uploadChunk('https://s3.example.com/part/1', chunk, 1);

      expect(etag).toBe('');
    });
  });

  // ======================== reportChunk ========================

  describe('reportChunk', () => {
    it('应成功上报分片', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse({}));

      await service.reportChunk('upload-001', 1, '"etag-001"');

      expect(global.fetch).toHaveBeenCalledWith('/api/upload/upload-001/chunk', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ part_number: 1, etag: '"etag-001"' }),
      });
    });

    it('上报失败时应抛出错误', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse({}, 404));

      await expect(service.reportChunk('upload-001', 1, '"etag-001"')).rejects.toThrow(
        '上报分片失败'
      );
    });
  });

  // ======================== getProgress ========================

  describe('getProgress', () => {
    it('应成功查询上传进度', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse(mockProgressResponse));

      const result = await service.getProgress('upload-001');

      expect(result).toEqual(mockProgressResponse);
      expect(global.fetch).toHaveBeenCalledWith('/api/upload/upload-001/progress', {
        headers: {},
      });
    });

    it('查询进度失败时应抛出错误', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse({}, 500));

      await expect(service.getProgress('upload-001')).rejects.toThrow('查询进度失败');
    });

    it('应正确处理已完成的上传进度', async () => {
      const completedProgress = {
        status: 'completed',
        receivedChunks: [1, 2, 3, 4, 5],
        totalChunks: 5,
        progressPct: 100,
      };
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse(completedProgress));

      const result = await service.getProgress('upload-001');

      expect(result.status).toBe('completed');
      expect(result.progressPct).toBe(100);
    });
  });

  // ======================== complete ========================

  describe('complete', () => {
    it('应成功完成上传', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse(mockCompleteResponse));

      const result = await service.complete('upload-001');

      expect(result).toEqual(mockCompleteResponse);
      expect(global.fetch).toHaveBeenCalledWith('/api/upload/upload-001/complete', {
        method: 'POST',
        headers: {},
      });
    });

    it('完成上传失败时应抛出错误', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse({}, 500));

      await expect(service.complete('upload-001')).rejects.toThrow('完成上传失败');
    });
  });

  // ======================== abort ========================

  describe('abort', () => {
    it('应成功取消上传', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse({}));

      await service.abort('upload-001');

      expect(global.fetch).toHaveBeenCalledWith('/api/upload/upload-001', {
        method: 'DELETE',
        headers: {},
      });
    });

    it('取消失败时应抛出错误', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse({}, 500));

      await expect(service.abort('upload-001')).rejects.toThrow('取消上传失败');
    });
  });

  // ======================== getVersionHistory ========================
  // 后端路由：GET /api/upload/versions/{fileGroupId}
  // 响应形状与 Tauri 命令 upload_get_version_history 一致（camelCase）

  describe('getVersionHistory', () => {
    const mockVersions = [
      {
        id: '990002',
        version: 2,
        isLatest: true,
        originalFilename: '手册-v2.pdf',
        fileSize: 2048,
        fileUrl: 'https://minio.local/k/a-v2.pdf',
        changeReason: '修订',
        createdAt: '2026-11-13T07:49:07.809372+00:00',
      },
    ];

    it('应请求 /upload/versions/{fileGroupId} 并原样返回 camelCase 数组', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse(mockVersions));

      const result = await service.getVersionHistory('group-001');

      expect(global.fetch).toHaveBeenCalledWith('/api/upload/versions/group-001', {
        headers: {},
      });
      expect(result).toEqual(mockVersions);
      expect(result[0].id).toBe('990002');
      expect(result[0].isLatest).toBe(true);
    });

    it('已登录时应附加 Bearer 令牌', async () => {
      localStorage.setItem('auth_token', 'token-abc');
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse([]));

      await service.getVersionHistory('group-001');

      expect(global.fetch).toHaveBeenCalledWith('/api/upload/versions/group-001', {
        headers: { Authorization: 'Bearer token-abc' },
      });
    });

    it('分组 ID 应做 URL 编码', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse([]));

      await service.getVersionHistory('group/with slash');

      expect((global.fetch as jest.Mock).mock.calls[0][0]).toBe(
        '/api/upload/versions/group%2Fwith%20slash'
      );
    });

    it('查询失败时应抛出错误', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse({}, 500));

      await expect(service.getVersionHistory('group-001')).rejects.toThrow('获取版本历史失败');
    });
  });

  // ======================== 401 续期（D-7） ========================

  describe('401 自动续期', () => {
    /** 401 响应（后端 ApiError） */
    function unauthorizedResponse() {
      return createMockResponse({ code: 'UNAUTHORIZED', message: '访问令牌已过期' }, 401);
    }

    /** 续期成功响应（`POST /api/auth/refresh`，后端 `ApiResponse` 包裹在 `data` 中） */
    function refreshedResponse(token = 'token-new') {
      return createMockResponse({ code: 0, message: 'ok', data: { token, refresh_token: 'refresh-new' } });
    }

    it('应续期并重放一次，成功后返回数据', async () => {
      localStorage.setItem('auth_token', 'token-old');
      localStorage.setItem('refresh_token', 'refresh-old');

      (global.fetch as jest.Mock) = jest
        .fn()
        // 首次 complete：401
        .mockResolvedValueOnce(unauthorizedResponse())
        // 续期请求
        .mockResolvedValueOnce(refreshedResponse())
        // 重放 complete：成功
        .mockResolvedValueOnce(
          createMockResponse({ file_url: 'https://storage.example.com/f.pdf', etag: '"e1"' })
        );

      const result = await service.complete('upload-001');

      expect(result).toEqual({ fileUrl: 'https://storage.example.com/f.pdf', etag: '"e1"' });

      const calls = (global.fetch as jest.Mock).mock.calls;
      expect(calls).toHaveLength(3);
      // 续期走统一基址（jsdom 下为 http://localhost/api）
      expect(calls[1][0]).toBe(`${window.location.origin}/api/auth/refresh`);
      // 重放时使用新令牌
      expect(calls[2][1].headers).toEqual({ Authorization: 'Bearer token-new' });
      // 新令牌已落盘，authStore 由事件同步
      expect(localStorage.getItem('auth_token')).toBe('token-new');
      expect(localStorage.getItem('refresh_token')).toBe('refresh-new');
    });

    it('续期失败时应清会话并抛出后端消息', async () => {
      localStorage.setItem('auth_token', 'token-old');
      localStorage.setItem('refresh_token', 'refresh-old');
      localStorage.setItem('auth_user', '{"id":1}');

      (global.fetch as jest.Mock) = jest
        .fn()
        .mockResolvedValueOnce(unauthorizedResponse())
        // 续期失败
        .mockResolvedValueOnce(createMockResponse({ code: 'INVALID_GRANT', message: '刷新令牌无效' }, 401));

      await expect(service.complete('upload-001')).rejects.toThrow('访问令牌已过期');

      // 会话被清空，避免「有令牌但一直 401」的僵死状态
      expect(localStorage.getItem('auth_token')).toBeNull();
      expect(localStorage.getItem('refresh_token')).toBeNull();
      expect(localStorage.getItem('auth_user')).toBeNull();
    });

    it('未登录（无令牌）时不尝试续期', async () => {
      (global.fetch as jest.Mock) = jest
        .fn()
        .mockResolvedValueOnce(unauthorizedResponse());

      await expect(service.complete('upload-001')).rejects.toThrow('访问令牌已过期');

      // 只有原始请求，没有额外的续期调用
      expect((global.fetch as jest.Mock).mock.calls).toHaveLength(1);
    });

    it('S3 presigned PUT 不参与续期（不带 Authorization）', async () => {
      localStorage.setItem('auth_token', 'token-old');
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(
        createMockResponse({}, 200, { etag: '"etag-001"' })
      );

      await service.uploadChunk('https://s3.example.com/part/1', new Blob(['x']), 1);

      const [, init] = (global.fetch as jest.Mock).mock.calls[0];
      expect(init.headers.Authorization).toBeUndefined();
      expect((global.fetch as jest.Mock).mock.calls).toHaveLength(1);
    });
  });

  // ======================== 自定义 baseUrl ========================

  describe('自定义 baseUrl', () => {
    it('应使用自定义 baseUrl', async () => {
      const customService = new UploadService('https://api.example.com/v2');
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse(mockInitResponse));

      await customService.init('test.pdf', 1000, 'text/plain');

      expect(global.fetch).toHaveBeenCalledWith(
        'https://api.example.com/v2/upload/init',
        expect.any(Object)
      );
    });
  });

  // ======================== 认证头 ========================
  // `/api/upload/*` 已纳入后端 auth_middleware，HTTP 模式必须带 Bearer 令牌；
  // S3 presigned PUT 不能带认证头（否则签名不匹配）。

  describe('认证头', () => {
    it('已登录时应附加 Bearer 令牌', async () => {
      localStorage.setItem('auth_token', 'token-abc');
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse(mockCompleteResponse));

      await service.complete('upload-001');

      expect(global.fetch).toHaveBeenCalledWith('/api/upload/upload-001/complete', {
        method: 'POST',
        headers: { Authorization: 'Bearer token-abc' },
      });
    });

    it('已登录时应保留 Content-Type 并附加令牌', async () => {
      localStorage.setItem('auth_token', 'token-abc');
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse(mockInitResponse));

      await service.init('report.pdf', 1024, 'application/pdf');

      expect(global.fetch).toHaveBeenCalledWith('/api/upload/init', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: 'Bearer token-abc' },
        body: expect.any(String),
      });
    });

    it('S3 presigned 分片直传不应附加认证头', async () => {
      localStorage.setItem('auth_token', 'token-abc');
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(
        createMockResponse({}, 200, { etag: '"etag-001"' })
      );

      const chunk = new Blob(['x']);
      await service.uploadChunk('https://s3.example.com/part/1', chunk, 1);

      const [, init] = (global.fetch as jest.Mock).mock.calls[0];
      expect(init.headers).toEqual({ 'Content-Length': chunk.size.toString() });
      expect(init.headers.Authorization).toBeUndefined();
    });
  });
});
