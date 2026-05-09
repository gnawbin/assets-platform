import type { Meta, StoryObj } from '@storybook/react';
import ProcessManagement from './ProcessManagement';
import React from 'react';

/**
 * 流程管理页面
 *
 * 通用的流程管理页面组件，支持传入标题来区分不同的流程模块。
 */
const meta: Meta<typeof ProcessManagement> = {
  title: '页面/ProcessManagement',
  component: ProcessManagement,
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          '流程管理页面组件，通过 title 属性区分不同的流程模块（领用审批、归还确认、调拨流程、维修流程、报废流程、所有流程）。',
      },
    },
  },
  tags: ['autodocs'],
  argTypes: {
    title: {
      control: 'select',
      options: ['领用审批', '归还确认', '调拨流程', '维修流程', '报废流程', '所有流程'],
      description: '流程模块标题',
    },
  },
};

export default meta;
type Story = StoryObj<typeof ProcessManagement>;

/**
 * 领用审批
 */
export const Approval: Story = {
  args: {
    title: '领用审批',
  },
};

/**
 * 归还确认
 */
export const Return: Story = {
  args: {
    title: '归还确认',
  },
};

/**
 * 调拨流程
 */
export const Transfer: Story = {
  args: {
    title: '调拨流程',
  },
};

/**
 * 维修流程
 */
export const Maintenance: Story = {
  args: {
    title: '维修流程',
  },
};

/**
 * 报废流程
 */
export const Scrap: Story = {
  args: {
    title: '报废流程',
  },
};

/**
 * 所有流程
 */
export const All: Story = {
  args: {
    title: '所有流程',
  },
};
