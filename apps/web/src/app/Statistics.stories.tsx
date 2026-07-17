import type { Meta, StoryObj } from '@storybook/react';
import Statistics from './Statistics';
import React from 'react';

/**
 * 统计分析页面
 *
 * 通用的统计分析页面组件，支持传入标题来区分不同的统计模块。
 */
const meta: Meta<typeof Statistics> = {
  title: '页面/Statistics',
  component: Statistics,
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          '统计分析页面组件，通过 title 属性区分不同的统计模块（资产统计、部门分布、状态分析、维保统计、授权统计、报表导出）。',
      },
    },
  },
  tags: ['autodocs'],
  argTypes: {
    title: {
      control: 'select',
      options: ['资产统计', '部门分布', '状态分析', '维保统计', '授权统计', '报表导出'],
      description: '统计模块标题',
    },
  },
};

export default meta;
type Story = StoryObj<typeof Statistics>;

/**
 * 资产统计
 */
export const Assets: Story = {
  args: {
    title: '资产统计',
  },
};

/**
 * 部门分布
 */
export const Department: Story = {
  args: {
    title: '部门分布',
  },
};

/**
 * 状态分析
 */
export const Status: Story = {
  args: {
    title: '状态分析',
  },
};

/**
 * 维保统计
 */
export const Maintenance: Story = {
  args: {
    title: '维保统计',
  },
};

/**
 * 授权统计
 */
export const License: Story = {
  args: {
    title: '授权统计',
  },
};

/**
 * 报表导出
 */
export const Export: Story = {
  args: {
    title: '报表导出',
  },
};
