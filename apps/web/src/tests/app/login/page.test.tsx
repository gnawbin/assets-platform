/**
 * LoginPage 登录页测试
 *
 * 重点覆盖「后端没启动」与「凭证错误」两类失败的文案区分：
 * 1. 传输层失败（`TypeError: Failed to fetch`）→ 提示「无法连接服务器 …」并引导去改地址，
 *    不能被误报成「请检查用户名和密码」（真实事故：`pnpm dev` 起的是 Tauri 桌面壳，
 *    3001 没有监听，登录页只显示 `Failed to fetch`）；
 * 2. 后端返回的业务错误 → 保持原有「请检查用户名和密码」，不泄露原始异常；
 * 3. 登录成功 → 写入登录态并跳转首页。
 */

import React from 'react';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import LoginPage from '@/app/login/page';

// ======================== Mock ========================

const mockPush = jest.fn();
const mockStoreLogin = jest.fn();
const mockLoginApi = jest.fn();

jest.mock('next/navigation', () => ({
    useRouter: () => ({ push: mockPush, replace: jest.fn(), refresh: jest.fn() }),
}));

jest.mock('next/link', () => ({
    __esModule: true,
    default: ({ children, href }: { children: React.ReactNode; href: string }) => (
        <a href={href}>{children}</a>
    ),
}));

jest.mock('@/store/authStore', () => ({
    useAuthStore: (selector: (state: { login: jest.Mock }) => unknown) =>
        selector({ login: mockStoreLogin }),
}));

jest.mock('@/services/authService', () => ({
    login: (...args: unknown[]) => mockLoginApi(...args),
}));

// ======================== 辅助函数 ========================

type WindowWithApiBase = Window & { __API_BASE__?: unknown };

function renderPage() {
    return render(
        <MantineProvider>
            <LoginPage />
        </MantineProvider>,
    );
}

/** 填表并点击「登 录」 */
function submit(username = 'admin', password = 'admin123') {
    fireEvent.change(screen.getByPlaceholderText('请输入用户名'), { target: { value: username } });
    fireEvent.change(screen.getByPlaceholderText('请输入密码'), { target: { value: password } });
    fireEvent.click(screen.getByRole('button', { name: /登\s*录/ }));
}

describe('LoginPage', () => {
    beforeEach(() => {
        (window as WindowWithApiBase).__API_BASE__ = 'http://127.0.0.1:3001';
        localStorage.clear();
    });

    afterEach(() => {
        delete (window as WindowWithApiBase).__API_BASE__;
    });

    it('后端未启动（Failed to fetch）时提示连不上服务器，而不是用户名密码错误', async () => {
        mockLoginApi.mockRejectedValue(new TypeError('Failed to fetch'));

        renderPage();
        submit();

        expect(await screen.findByText(/无法连接服务器 http:\/\/127\.0\.0\.1:3001/))
            .toBeInTheDocument();
        expect(screen.getByText(/请确认后端服务已启动/)).toBeInTheDocument();
        expect(screen.queryByText(/请检查用户名和密码/)).not.toBeInTheDocument();
        expect(mockStoreLogin).not.toHaveBeenCalled();
        expect(mockPush).not.toHaveBeenCalled();
    });

    it('后端返回业务错误时保持原有提示（不暴露原始异常）', async () => {
        mockLoginApi.mockRejectedValue(new Error('用户名或密码错误'));

        renderPage();
        submit();

        expect(await screen.findByText('登录失败，请检查用户名和密码')).toBeInTheDocument();
        expect(screen.queryByText(/无法连接服务器/)).not.toBeInTheDocument();
        expect(mockPush).not.toHaveBeenCalled();
    });

    it('登录成功时写入登录态并跳转首页', async () => {
        const result = {
            token: 'token-abc',
            refresh_token: 'refresh-abc',
            expires_in: 604800,
            user_id: 1,
            username: 'admin',
        };
        mockLoginApi.mockResolvedValue(result);

        renderPage();
        submit(' admin ', 'admin123');

        await waitFor(() => {
            expect(mockPush).toHaveBeenCalledWith('/');
        });
        expect(mockLoginApi).toHaveBeenCalledWith('admin', 'admin123');
        expect(mockStoreLogin).toHaveBeenCalledWith(result);
    });

    it('用户名为空时本地校验，不发请求', async () => {
        renderPage();
        submit('   ', 'admin123');

        expect(await screen.findByText('请输入用户名')).toBeInTheDocument();
        expect(mockLoginApi).not.toHaveBeenCalled();
    });
});
