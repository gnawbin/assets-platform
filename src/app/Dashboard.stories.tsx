import type { Meta, StoryObj } from '@storybook/react';
import Dashboard from './Dashboard';
import React from 'react';

/**
 * 仪表盘页面
 *
 * 系统首页，展示资产总览数据，包括统计卡片、资产状态分布、即将到期事项、快速操作和系统信息。
 */
const meta: Meta<typeof Dashboard> = {
  title: '页面/Dashboard',
  component: Dashboard,
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          '仪表盘页面，作为系统首页展示资产总览数据。包含统计卡片（总资产数、硬资产数、软资产数、部门数量）、资产状态分布（进度条）、即将到期事项、快速操作按钮和系统信息等模块。',
      },
    },
  },
  tags: ['autodocs'],
};

export default meta;
type Story = StoryObj<typeof Dashboard>;

/**
 * 默认仪表盘
 *
 * 显示完整的仪表盘页面，包含所有数据模块。
 */
export const Default: Story = {};
