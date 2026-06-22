/**
 * Jest 测试环境配置
 */

// 扩展 jest 匹配器
import '@testing-library/jest-dom';

// Mock fetch API
global.fetch = jest.fn();

// Mock localStorage
const localStorageMock = (() => {
  let store: Record<string, string> = {};
  return {
    getItem: jest.fn((key: string) => store[key] ?? null),
    setItem: jest.fn((key: string, value: string) => {
      store[key] = value;
    }),
    removeItem: jest.fn((key: string) => {
      delete store[key];
    }),
    clear: jest.fn(() => {
      store = {};
    }),
    get length() {
      return Object.keys(store).length;
    },
    key: jest.fn((index: number) => Object.keys(store)[index] ?? null),
  };
})();

Object.defineProperty(window, 'localStorage', {
  value: localStorageMock,
});

// Mock URL.createObjectURL
URL.createObjectURL = jest.fn(() => 'blob:mock-url');
URL.revokeObjectURL = jest.fn();

// Mock ResizeObserver
global.ResizeObserver = jest.fn().mockImplementation(() => ({
  observe: jest.fn(),
  unobserve: jest.fn(),
  disconnect: jest.fn(),
}));

// Mock IntersectionObserver
global.IntersectionObserver = jest.fn().mockImplementation(() => ({
  observe: jest.fn(),
  unobserve: jest.fn(),
  disconnect: jest.fn(),
}));

// Mock clipboard API
Object.defineProperty(navigator, 'clipboard', {
  value: {
    writeText: jest.fn(),
    readText: jest.fn(),
  },
  writable: true,
});

// Mock DataTransfer (jsdom 不支持)
// Mantine Dropzone 从 event.dataTransfer.files 读取文件
global.DataTransfer = jest.fn().mockImplementation(() => {
  const files: File[] = [];
  return {
    files: {
      get length() { return files.length; },
      item(index: number) { return files[index] ?? null; },
      [Symbol.iterator]() { return files[Symbol.iterator](); },
    },
    items: {
      add(file: File) { files.push(file); },
      clear() { files.length = 0; },
      get length() { return files.length; },
      [Symbol.iterator]() { return files[Symbol.iterator](); },
    },
  };
}) as unknown as typeof DataTransfer;

// 清理每个测试后的 mock
beforeEach(() => {
  jest.clearAllMocks();
  localStorageMock.clear();
});