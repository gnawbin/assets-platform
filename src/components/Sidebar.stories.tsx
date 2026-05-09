import type { Meta, StoryObj } from '@storybook/react';
import Sidebar from './Sidebar';
import React from 'react';

/**
 * 侧边栏导航组件
 *
 * 显示系统的主要导航菜单，包括仪表盘、资产台账、流程管理、统计分析和系统配置等模块。
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
          '侧边栏导航组件，用于展示系统的主要导航菜单。包含多级菜单、路由高亮等功能。',
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
 * 显示完整的侧边栏导航菜单，包含所有导航项和子菜单。
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

/**
 * 仪表盘激活状态
 *
 * 当用户位于仪表盘页面时，仪表盘菜单项高亮显示。
 */
export const DashboardActive: Story = {
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
 * 硬资产页面激活状态
 *
 * 当用户位于硬资产页面时，资产台账菜单展开且硬资产子菜单高亮。
 */
export const HardwareActive: Story = {
  parameters: {
    nextjs: {
      appDirectory: true,
      navigation: {
        pathname: '/hardware',
      },
    },
  },
};

/**
 * 流程管理-领用审批激活状态
 *
 * 当用户位于领用审批页面时，流程管理菜单展开且领用审批子菜单高亮。
 */
export const ApprovalActive: Story = {
  parameters: {
    nextjs: {
      appDirectory: true,
      navigation: {
        pathname: '/process/approval',
      },
    },
  },
};

/**
 * 统计分析-资产统计激活状态
 *
 * 当用户位于资产统计页面时，统计分析菜单展开且资产统计子菜单高亮。
 */
export const StatisticsActive: Story = {
  parameters: {
    nextjs: {
      appDirectory: true,
      navigation: {
        pathname: '/statistics/assets',
      },
    },
  },
};

/**
 * 系统配置-数据库配置激活状态
 *
 * 当用户位于数据库配置页面时，系统配置菜单展开且数据库配置子菜单高亮。
 */
export const SettingsActive: Story = {
  parameters: {
    nextjs: {
      appDirectory: true,
      navigation: {
        pathname: '/settings/database',
      },
    },
  },
};
