import type { Meta, StoryObj } from '@storybook/react';
import Layout from './Layout';
import { Text } from '@mantine/core';
import React from 'react';

/**
 * 应用布局组件
 *
 * 使用 Mantine AppShell 构建的全局布局，包含顶部导航栏、左侧边栏和主内容区域。
 * 支持响应式设计，在移动端可折叠侧边栏。
 */
const meta: Meta<typeof Layout> = {
  title: '组件/Layout',
  component: Layout,
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component:
          '应用布局组件，包含顶部导航栏（标题和汉堡菜单）、左侧边栏导航和主内容区域。使用 Mantine AppShell 实现响应式布局。',
      },
    },
  },
  tags: ['autodocs'],
};

export default meta;
type Story = StoryObj<typeof Layout>;

/**
 * 默认布局
 *
 * 显示完整的应用布局，包含顶部导航栏、侧边栏和内容区域。
 * 内容区域展示示例文本。
 */
export const Default: Story = {
  args: {
    children: (
      <div>
        <Text size="xl" fw={700} mb="md">
          欢迎使用资产管理平台
        </Text>
        <Text c="dimmed">
          这是一个示例内容区域，在实际应用中，这里会渲染对应的页面组件。
        </Text>
      </div>
    ),
  },
  parameters: {
    nextjs: {
      appDirectory: true,
      navigation: {
        pathname: '/',
      },
    },
  },
};

/**
 * 带表格内容的布局
 *
 * 展示布局在包含表格数据时的显示效果。
 */
export const WithTableContent: Story = {
  args: {
    children: (
      <div>
        <Text size="xl" fw={700} mb="md">
          硬资产列表
        </Text>
        <div
          style={{
            background: 'white',
            padding: 20,
            borderRadius: 8,
            border: '1px solid #dee2e6',
          }}
        >
          <Text c="dimmed" ta="center" py={40}>
            资产表格将在此处渲染
          </Text>
        </div>
      </div>
    ),
  },
  parameters: {
    nextjs: {
      appDirectory: true,
      navigation: {
        pathname: '/hardware',
      },
    },
  },
};
