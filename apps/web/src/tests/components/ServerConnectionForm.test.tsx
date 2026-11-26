/**
 * ServerConnectionForm 组件测试
 *
 * 覆盖用户在「服务端连接」页 / 首屏 Gate 上真正会走到的分支：
 * - 渲染当前生效地址
 * - 「测试连接」成功 / 失败（走 `probeServer` → mock fetch）
 * - 保存 https 地址（写入 localStorage 并回调）
 * - 远程 http 地址的二次确认（默认拦截，确认后放行）
 * - 「恢复默认」清空已保存地址
 */

import React from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { ServerConnectionForm } from '@/components/ServerConnectionForm';
import { API_BASE_STORAGE_KEY, setApiBase } from '@/utils/apiBase';

/** 统一的渲染入口：Mantine 组件依赖 ThemeProvider 上下文 */
function renderForm(props: React.ComponentProps<typeof ServerConnectionForm> = {}) {
    return render(
        <MantineProvider>
            <ServerConnectionForm {...props} />
        </MantineProvider>,
    );
}

/** 覆写输入框内容（Mantine TextInput 把 data-testid 透传到原生 input） */
function typeAddress(value: string): void {
    fireEvent.change(screen.getByTestId('server-address-input'), {
        target: { value },
    });
}

describe('ServerConnectionForm', () => {
    const originalFetch = global.fetch;

    beforeEach(() => {
        window.localStorage.clear();
    });

    afterEach(() => {
        global.fetch = originalFetch;
        window.localStorage.clear();
    });

    it('默认展示当前生效的服务器地址', () => {
        setApiBase('https://demo.example.com');

        renderForm();

        expect(screen.getByTestId('server-address-input')).toHaveValue(
            'https://demo.example.com',
        );
    });

    it('测试连接成功时展示延迟与 db 状态', async () => {
        global.fetch = jest.fn().mockResolvedValue({
            ok: true,
            status: 200,
            text: async () => JSON.stringify({ status: 'ok', db: 'up', service: 'assets-api' }),
        }) as unknown as typeof fetch;

        renderForm();
        typeAddress('http://127.0.0.1:3001');
        fireEvent.click(screen.getByRole('button', { name: /测试连接/ }));

        expect(await screen.findByText('连接成功')).toBeInTheDocument();
        expect(screen.getByText('db: up')).toBeInTheDocument();
        expect(screen.getByText('assets-api')).toBeInTheDocument();
    });

    it('测试连接失败时给出可操作的排查提示', async () => {
        global.fetch = jest.fn().mockResolvedValue({
            ok: false,
            status: 503,
            text: async () => '',
        }) as unknown as typeof fetch;

        renderForm();
        typeAddress('http://127.0.0.1:3001');
        fireEvent.click(screen.getByRole('button', { name: /测试连接/ }));

        expect(await screen.findByText(/服务器返回 503/)).toBeInTheDocument();
    });

    it('保存 https 地址后写入 localStorage 并触发 onSaved', async () => {
        const onSaved = jest.fn();
        renderForm({ onSaved });

        typeAddress('https://demo.example.com');
        fireEvent.click(screen.getByRole('button', { name: /保存并重载/ }));

        await waitFor(() => {
            expect(window.localStorage.getItem(API_BASE_STORAGE_KEY)).toBe(
                'https://demo.example.com/api',
            );
        });
        expect(onSaved).toHaveBeenCalledWith('https://demo.example.com/api');
    });

    it('远程 http 地址需要二次确认后才保存', async () => {
        const onSaved = jest.fn();
        renderForm({ onSaved });

        typeAddress('http://10.0.0.5:3001');
        fireEvent.click(screen.getByRole('button', { name: /保存并重载/ }));

        // 默认拦截：未写入 localStorage，先弹确认
        expect(await screen.findByText('确认使用明文 HTTP？')).toBeInTheDocument();
        expect(window.localStorage.getItem(API_BASE_STORAGE_KEY)).toBeNull();
        expect(onSaved).not.toHaveBeenCalled();

        fireEvent.click(screen.getByRole('button', { name: '仍要保存' }));

        await waitFor(() => {
            expect(window.localStorage.getItem(API_BASE_STORAGE_KEY)).toBe(
                'http://10.0.0.5:3001/api',
            );
        });
        expect(onSaved).toHaveBeenCalledWith('http://10.0.0.5:3001/api');
    });

    it('「恢复默认」清空已保存地址', async () => {
        setApiBase('https://demo.example.com');
        const onSaved = jest.fn();
        renderForm({ onSaved });

        fireEvent.click(screen.getByRole('button', { name: /恢复默认/ }));

        await waitFor(() => {
            expect(window.localStorage.getItem(API_BASE_STORAGE_KEY)).toBeNull();
        });
        expect(onSaved).toHaveBeenCalledWith('');
    });
});
