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
      expect(global.fetch).toHaveBeenCalledWith('/api/upload/upload-001/progress');
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
      });
    });

    it('取消失败时应抛出错误', async () => {
      (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(createMockResponse({}, 500));

      await expect(service.abort('upload-001')).rejects.toThrow('取消上传失败');
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
});
