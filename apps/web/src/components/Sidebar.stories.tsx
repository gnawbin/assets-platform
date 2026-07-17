import type { Meta, StoryObj } from '@storybook/react';
import Sidebar from './Sidebar';
import React from 'react';

/**
 * 侧边栏导航组件
 *
 * 从 sys_menu 表动态获取菜单数据，展示系统的主要导航菜单。
 * 支持展开/折叠子菜单，并高亮当前激活的路由。
 */
const meta: Meta<typeof Sidebar> = {
  title: '组件/Sidebar',
  component: Sidebar,
  parameters: {
    layout: 'centered',
    docs: {
      description: {
        component:
          '侧边栏导航组件，从 sys_menu 表动态获取菜单数据。包含多级菜单、路由高亮等功能。',
      },
    },
  },
  decorators: [
    (Story) => (
      <div style={{ height: '100vh', width: 280 }}>
        <Story />
      </div>
    ),
  ],
  tags: ['autodocs'],
};

export default meta;
type Story = StoryObj<typeof Sidebar>;

/**
 * 默认状态
 *
 * 显示从 sys_menu 表动态加载的侧边栏导航菜单。
 */
export const Default: Story = {
  parameters: {
    nextjs: {
      appDirectory: true,
      navigation: {
        pathname: '/',
      },
    },
  },
};
