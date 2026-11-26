import type { StorybookConfig } from '@storybook/nextjs-vite';

const config: StorybookConfig = {
  stories: ['../src/**/*.stories.@(ts|tsx)'],
  addons: ['@storybook/addon-themes'],
  framework: {
    name: '@storybook/nextjs-vite',
    options: {},
  },
  staticDirs: ['../public'],
  viteFinal: async (config) => {
    const previousOnWarn = config.build?.rollupOptions?.onwarn;
    config.build = {
      ...config.build,
      rollupOptions: {
        ...config.build?.rollupOptions,
        onwarn(warning, warn) {
          // Storybook preview 本身就是纯客户端环境，忽略 "use client" 指令告警；
          // Sourcemap 告警只是上报位置信息缺失，不影响构建产物。
          if (warning.code === 'MODULE_LEVEL_DIRECTIVE' || warning.code === 'SOURCEMAP_BROKEN') {
            return;
          }
          if (previousOnWarn) {
            previousOnWarn(warning, warn);
          } else {
            warn(warning);
          }
        },
      },
    };
    return config;
  },
};

export default config;
