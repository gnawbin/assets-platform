import type { Meta, StoryObj } from '@storybook/react';
import SystemSettings from './SystemSettings';
import React from 'react';

/**
 * 系统配置页面
 *
 * 通用的系统配置页面组件，支持传入标题来区分不同的配置模块。
 */
const meta: Meta<typeof SystemSettings> = {
  title: '页面/SystemSettings',
  component: SystemSettings,
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          '系统配置页面组件，通过 title 属性区分不同的配置模块（数据库配置、权限管理、资产分类、部门管理、用户管理、系统日志）。',
      },
    },
  },
  tags: ['autodocs'],
  argTypes: {
    title: {
      control: 'select',
      options: ['数据库配置', '权限管理','部门管理', '用户管理', '系统日志'],
      description: '配置模块标题',
    },
  },
};

export default meta;
type Story = StoryObj<typeof SystemSettings>;

/**
 * 数据库配置
 */
export const Database: Story = {
  args: {
    title: '数据库配置',
  },
};

/**
 * 权限管理
 */
export const Permissions: Story = {
  args: {
    title: '权限管理',
  },
};


/**
 * 部门管理
 */
export const Departments: Story = {
  args: {
    title: '部门管理',
  },
};

/**
 * 用户管理
 */
export const Users: Story = {
  args: {
    title: '用户管理',
  },
};

/**
 * 系统日志
 */
export const Logs: Story = {
  args: {
    title: '系统日志',
  },
};
