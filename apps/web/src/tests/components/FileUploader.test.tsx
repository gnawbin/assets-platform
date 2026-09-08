/**
 * FileUploader 组件测试
 *
 * 测试覆盖：
 * - 组件渲染
 * - 文件选择
 * - 文件校验
 * - 错误文件展示
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

/**
 * 全局 mock 对象 - 通过修改其属性来模拟不同的上传状态
 * 注意：不能重新赋值，因为 jest.mock 的 factory 函数在提升期捕获引用
 */
const mockUploadState: {
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
  resume: jest.fn().mockResolvedValue(undefined),
  cancel: jest.fn().mockResolvedValue(undefined),
  retry: jest.fn().mockResolvedValue(undefined),
};

// eslint-disable-next-line @typescript-eslint/no-unused-vars
function resetMockUploadState() {
  mockUploadState.progress = 0;
  mockUploadState.status = 'idle';
  mockUploadState.error = null;
  mockUploadState.uploadedBytes = 0;
  mockUploadState.totalBytes = 0;
  mockUploadState.speed = 0;
  mockUploadState.start = jest.fn();
  mockUploadState.pause = jest.fn();
  mockUploadState.resume = jest.fn().mockResolvedValue(undefined);
  mockUploadState.cancel = jest.fn().mockResolvedValue(undefined);
  mockUploadState.retry = jest.fn().mockResolvedValue(undefined);
}

jest.mock('@/hooks/useChunkedUpload', () => ({
  useChunkedUpload: () => mockUploadState,
}));

/**
 * Mock @mantine/dropzone 的 Dropzone 组件
 */
jest.mock('@mantine/dropzone', () => {
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const Dummy = ({ children }: { children?: React.ReactNode }) => <>{children}</>;

  const MockInner = ({
    children,
    onDrop,
  }: {
    children: React.ReactNode;
    onDrop: (files: File[]) => void;
    onReject: (rejections: any[]) => void;
    accept: string[];
    maxSize: number;
    multiple: boolean;
  }) => {
    const ReactLocal = require('react') as typeof React;
    const inputRef = ReactLocal.useRef<HTMLInputElement>(null);
    return ReactLocal.createElement(
      'div',
      {
        className: 'mantine-Dropzone-root',
        'data-testid': 'mock-dropzone',
        onClick: () => inputRef.current?.click(),
      },
      ReactLocal.createElement('input', {
        ref: inputRef,
        type: 'file',
        'data-testid': 'file-input',
        style: { display: 'none' },
        onChange: (e: React.ChangeEvent<HTMLInputElement>) => {
          if (e.target.files && e.target.files.length > 0) {
            const files = Array.from(e.target.files) as File[];
            onDrop(files);
          }
        },
      }),
      ReactLocal.createElement('div', { className: 'mantine-Dropzone-inner' }, children)
    );
  };

  return {
    Dropzone: Object.assign(MockInner, {
      Accept: Dummy,
      Reject: Dummy,
      Idle: Dummy,
    }),
  };
});

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
 * 通过 mock Dropzone 的 input 选择文件
 */
function selectFileViaDropzone(file: File) {
  const fileInput = screen.getByTestId('file-input');
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
    resetMockUploadState();
  });

  // ======================== 渲染 ========================

  describe('渲染', () => {
    it('应渲染上传按钮', () => {
      renderWithProviders(<FileUploader />);
      expect(screen.getByText('上传文件')).toBeInTheDocument();
    });

    it('点击按钮应打开弹窗', async () => {
      renderWithProviders(<FileUploader />);
      openUploadModal();
      await waitFor(() => {
        expect(screen.getByText('拖拽文件到此处，或点击选择')).toBeInTheDocument();
      });
    });
  });

  // ======================== 文件选择 ========================

  describe('文件选择', () => {
    it('选择文件后应显示文件列表', async () => {
      renderWithProviders(<FileUploader />);
      openUploadModal();

      await waitFor(() => {
        expect(screen.getByText('拖拽文件到此处，或点击选择')).toBeInTheDocument();
      });

      const file = createFile('test.pdf', 1024, 'application/pdf');
      selectFileViaDropzone(file);

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

      await waitFor(() => {
        expect(screen.getByText('拖拽文件到此处，或点击选择')).toBeInTheDocument();
      });

      const file = createFile('large.pdf', 1024, 'application/pdf');
      selectFileViaDropzone(file);

      await waitFor(() => {
        expect(screen.getByText(/超过大小限制/)).toBeInTheDocument();
      });
    });

    it('文件类型不支持时应显示错误', async () => {
      renderWithProviders(<FileUploader accept=".pdf,.docx" />);
      openUploadModal();

      await waitFor(() => {
        expect(screen.getByText('拖拽文件到此处，或点击选择')).toBeInTheDocument();
      });

      const file = createFile('image.png', 1024, 'image/png');
      selectFileViaDropzone(file);

      await waitFor(() => {
        expect(screen.getByText(/类型不支持/)).toBeInTheDocument();
      });
    });
  });
});