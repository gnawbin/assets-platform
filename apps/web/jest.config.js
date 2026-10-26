import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

import nextJest from 'next/jest.js';

const dir = dirname(fileURLToPath(import.meta.url));
// 使用 Next.js 内置的 SWC 转换器，避免 ts-jest 依赖 TypeScript JS Compiler API
const createJestConfig = nextJest({ dir });

/** @type {import('jest').Config} */
const config = {
  testEnvironment: 'jsdom',
  setupFilesAfterEnv: ['<rootDir>/src/tests/setup.ts'],
  moduleNameMapper: {
    '^@/(.*)$': '<rootDir>/src/$1',
    '^@assets/shared$': '<rootDir>/../../packages/shared/src/index.ts',
    '^@assets/shared/(.*)$': '<rootDir>/../../packages/shared/src/$1',
  },
  testMatch: [
    '<rootDir>/src/tests/**/*.test.ts',
    '<rootDir>/src/tests/**/*.test.tsx',
  ],
  collectCoverageFrom: [
    'src/**/*.{ts,tsx}',
    '!src/**/*.d.ts',
    '!src/tests/**/*',
  ],
  coverageDirectory: 'coverage',
  coverageReporters: ['text', 'lcov', 'html'],
};

export default createJestConfig(config);

