import type { Meta, StoryObj } from '@storybook/react';
import AssetLedger from './AssetLedger';
import React from 'react';

/**
 * 资产台账页面
 *
 * 通用的资产台账页面组件，支持传入标题来区分硬资产和软资产。
 */
const meta: Meta<typeof AssetLedger> = {
  title: '页面/AssetLedger',
  component: AssetLedger,
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          '资产台账页面组件，通过 title 属性区分硬资产和软资产。包含搜索、新增资产按钮和资产列表区域。',
      },
    },
  },
  tags: ['autodocs'],
  argTypes: {
    title: {
      control: 'select',
      options: ['硬资产', '软资产'],
      description: '台账标题',
    },
  },
};

export default meta;
type Story = StoryObj<typeof AssetLedger>;

/**
 * 硬资产台账
 *
 * 显示硬资产台账页面。
 */
export const Hardware: Story = {
  args: {
    title: '硬资产',
  },
};

/**
 * 软资产台账
 *
 * 显示软资产台账页面。
 */
export const Software: Story = {
  args: {
    title: '软资产',
  },
};
