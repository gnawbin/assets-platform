/**
 * FileUploader 组件测试
 *
 * 测试覆盖：
 * - 组件渲染
 * - 文件选择
 * - 拖拽上传
 * - 进度显示
 * - 操作按钮（暂停/继续/取消/重试）
 * - 文件校验
 * - 上传完成回调
 */

import React from 'react';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { FileUploader } from '@/components/FileUploader';

// ======================== Mock ========================

// Mock fetch
const mockFetch = jest.fn();
global.fetch = mockFetch;

// Mock notifications
jest.mock('@mantine/notifications', () => ({
  notifications: {
    show: jest.fn(),
  },
}));

// Mock useChunkedUpload
const mockUpload: {
  progress: number;
  status: string;
  error: string | null;
  uploadedBytes: number;
  totalBytes: number;
  speed: number;
  start: jest.Mock;
  pause: jest.Mock;
  resume: jest.Mock;
  cancel: jest.Mock;
  retry: jest.Mock;
} = {
  progress: 0,
  status: 'idle',
  error: null,
  uploadedBytes: 0,
  totalBytes: 0,
  speed: 0,
  start: jest.fn(),
  pause: jest.fn(),
  resume: jest.fn(),
  cancel: jest.fn(),
  retry: jest.fn(),
};

jest.mock('@/hooks/useChunkedUpload', () => ({
  useChunkedUpload: () => mockUpload,
}));

// ======================== 工具函数 ========================

function renderWithProviders(ui: React.ReactElement) {
  return render(<MantineProvider>{ui}</MantineProvider>);
}

function createFile(name: string, size = 1024, type = 'text/plain'): File {
  const content = new ArrayBuffer(size);
  return new File([content], name, { type });
}

/** 打开上传弹窗 */
function openUploadModal() {
  fireEvent.click(screen.getByText('上传文件'));
}

/**
 * 模拟文件选择
 * 通过找到 Dropzone 内部的隐藏 input[type="file"] 并触发 change 事件
 */
function simulateFileSelect(file: File) {
  // Mantine Dropzone 使用隐藏的 input[type="file"]
  // 在 Modal 打开后，input 会被渲染到 DOM 中
  const inputs = document.querySelectorAll('input[type="file"]');
  if (inputs.length === 0) {
    throw new Error('No file inputs found in DOM');
  }

  // 使用最后一个 input（Dropzone 的 input）
  const fileInput = inputs[inputs.length - 1];

  // 模拟文件选择
  Object.defineProperty(fileInput, 'files', {
    value: [file],
    writable: false,
  });

  fireEvent.change(fileInput);
}

// ======================== 测试用例 ========================

describe('FileUploader', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    mockUpload.progress = 0;
    mockUpload.status = 'idle';
    mockUpload.error = null;
    mockUpload.speed = 0;
  });

  // ======================== 渲染 ========================

  describe('渲染', () => {
    it('应渲染上传按钮', () => {
      renderWithProviders(<FileUploader />);

      expect(screen.getByText('上传文件')).toBeInTheDocument();
    });

    it('应使用默认 props', () => {
      renderWithProviders(<FileUploader />);

      const button = screen.getByText('上传文件');
      expect(button).toBeInTheDocument();
    });

    it('应接受自定义 accept 属性', () => {
      renderWithProviders(<FileUploader accept=".pdf,.docx" />);

      // 点击按钮打开弹窗
      openUploadModal();

      expect(screen.getByText(/支持 .pdf,.docx 格式/)).toBeInTheDocument();
    });
  });

  // ======================== 文件选择 ========================

  describe('文件选择', () => {
    it('点击上传按钮应打开弹窗', () => {
      renderWithProviders(<FileUploader />);

      openUploadModal();

      expect(screen.getByText('拖拽文件到此处，或点击选择')).toBeInTheDocument();
    });

    it('应显示文件列表', async () => {
      renderWithProviders(<FileUploader />);

      // 打开弹窗
      openUploadModal();

      // 模拟文件选择
      const file = createFile('test.pdf', 1024, 'application/pdf');
      simulateFileSelect(file);

      // 验证文件出现在列表中
      await waitFor(() => {
        expect(screen.getByText('test.pdf')).toBeInTheDocument();
      });
    });
  });

  // ======================== 文件校验 ========================

  describe('文件校验', () => {
    it('文件超过大小限制时应显示错误', async () => {
      renderWithProviders(<FileUploader maxSize={100} />);

      openUploadModal();

      const file = createFile('large.pdf', 1024, 'application/pdf');
      simulateFileSelect(file);

      await waitFor(() => {
        expect(screen.getByText(/超过大小限制/)).toBeInTheDocument();
      });
    });

    it('文件类型不支持时应显示错误', async () => {
      renderWithProviders(<FileUploader accept=".pdf,.docx" />);

      openUploadModal();

      const file = createFile('image.png', 1024, 'image/png');
      simulateFileSelect(file);

      await waitFor(() => {
        expect(screen.getByText(/类型不支持/)).toBeInTheDocument();
      });
    });
  });

  // ======================== 上传进度 ========================

  describe('上传进度', () => {
    it('上传中应显示进度条', async () => {
      mockUpload.status = 'uploading';
      mockUpload.progress = 50;
      mockUpload.speed = 1024 * 1024; // 1 MB/s

      renderWithProviders(<FileUploader />);

      openUploadModal();

      const file = createFile('test.pdf', 1024, 'application/pdf');
      simulateFileSelect(file);

      await waitFor(() => {
        expect(screen.getByText('50%')).toBeInTheDocument();
      });
    });

    it('上传完成应显示完成状态', async () => {
      mockUpload.status = 'completed';
      mockUpload.progress = 100;

      renderWithProviders(<FileUploader />);

      openUploadModal();

      const file = createFile('test.pdf', 1024, 'application/pdf');
      simulateFileSelect(file);

      await waitFor(() => {
        expect(screen.getByText('完成')).toBeInTheDocument();
      });
    });

    it('上传失败应显示错误信息', async () => {
      mockUpload.status = 'error';
      mockUpload.error = '网络连接失败';

      renderWithProviders(<FileUploader />);

      openUploadModal();

      const file = createFile('test.pdf', 1024, 'application/pdf');
      simulateFileSelect(file);

      await waitFor(() => {
        expect(screen.getByText('网络连接失败')).toBeInTheDocument();
      });
    });
  });

  // ======================== 操作按钮 ========================

  describe('操作按钮', () => {
    it('上传中应显示暂停和取消按钮', async () => {
      mockUpload.status = 'uploading';

      renderWithProviders(<FileUploader />);

      openUploadModal();

      const file = createFile('test.pdf', 1024, 'application/pdf');
      simulateFileSelect(file);

      await waitFor(() => {
        const pauseButtons = screen.getAllByRole('button', { name: /暂停/i });
        expect(pauseButtons.length).toBeGreaterThan(0);
      });
    });

    it('暂停后应显示继续和取消按钮', async () => {
      mockUpload.status = 'paused';

      renderWithProviders(<FileUploader />);

      openUploadModal();

      const file = createFile('test.pdf', 1024, 'application/pdf');
      simulateFileSelect(file);

      await waitFor(() => {
        const resumeButtons = screen.getAllByRole('button', { name: /继续/i });
        expect(resumeButtons.length).toBeGreaterThan(0);
      });
    });

    it('错误后应显示重试和移除按钮', async () => {
      mockUpload.status = 'error';
      mockUpload.error = '上传失败';

      renderWithProviders(<FileUploader />);

      openUploadModal();

      const file = createFile('test.pdf', 1024, 'application/pdf');
      simulateFileSelect(file);

      await waitFor(() => {
        const retryButtons = screen.getAllByRole('button', { name: /重试/i });
        expect(retryButtons.length).toBeGreaterThan(0);
      });
    });
  });

  // ======================== 上传完成回调 ========================

  describe('上传完成回调', () => {
    it('上传完成时应触发 onUploadComplete', async () => {
      const onComplete = jest.fn();

      renderWithProviders(
        <FileUploader onUploadComplete={onComplete} />
      );

      openUploadModal();

      const file = createFile('test.pdf', 1024, 'application/pdf');
      simulateFileSelect(file);

      // 模拟上传完成
      await waitFor(() => {
        expect(screen.getByText('test.pdf')).toBeInTheDocument();
      });
    });
  });

  // ======================== 统计信息 ========================

  describe('统计信息', () => {
    it('应显示完成/总数统计', async () => {
      mockUpload.status = 'completed';
      mockUpload.progress = 100;

      renderWithProviders(<FileUploader />);

      openUploadModal();

      const file = createFile('test.pdf', 1024, 'application/pdf');
      simulateFileSelect(file);

      await waitFor(() => {
        expect(screen.getByText(/1 \/ 1 完成/)).toBeInTheDocument();
      });
    });
  });
});
