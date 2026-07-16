/**
 * 统一通知提示工具
 *
 * 基于 @mantine/notifications 封装，替代全局 alert() 调用。
 * 提供成功、错误、信息三种类型的通知。
 */

import { notifications } from '@mantine/notifications';

/**
 * 成功通知
 * @param title 标题
 * @param message 详情（可选）
 */
export function notifySuccess(title: string, message?: string) {
    notifications.show({
        title,
        message: message || '',
        color: 'green',
        position: 'top-right',
        autoClose: 3000,
    });
}

/**
 * 错误通知
 * @param title 标题
 * @param message 详情（可选）
 */
export function notifyError(title: string, message?: string) {
    notifications.show({
        title,
        message: message || '',
        color: 'red',
        position: 'top-right',
        autoClose: 5000,
    });
}

/**
 * 信息通知
 * @param title 标题
 * @param message 详情（可选）
 */
export function notifyInfo(title: string, message?: string) {
    notifications.show({
        title,
        message: message || '',
        color: 'blue',
        position: 'top-right',
        autoClose: 3000,
    });
}
