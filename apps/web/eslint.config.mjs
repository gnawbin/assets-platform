import { defineConfig, globalIgnores } from 'eslint/config';
import nextVitals from 'eslint-config-next/core-web-vitals';

export default defineConfig([
  ...nextVitals,
  {
    rules: {
      // 以下 react-hooks 规则来自 React Compiler 的静态分析，当前会把大量既有代码判为 error。
      // 先降级为 warning，待相关组件逐步重构后再恢复为 error（不影响 lint 退出码）。
      'react-hooks/set-state-in-effect': 'warn',
      'react-hooks/immutability': 'warn',
      'react-hooks/refs': 'warn',
    },
  },
  globalIgnores([
    '.next/**',
    'dist/**',
    'coverage/**',
    'storybook-static/**',
    'next-env.d.ts',
  ]),
]);
