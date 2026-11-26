/**
 * ServerConfigGate 组件测试
 *
 * Gate 的唯一职责：**只在「兜底地址 + 探活失败」时阻断首屏**。
 * 因此这里覆盖四种组合，确保 nginx 同源部署、用户已保存地址等正常场景不会被打扰：
 * 1. 用户保存过地址（非兜底）→ 直接放行，不发探活请求
 * 2. 兜底地址 + 探活失败 → 展示配置表单
 * 3. 兜底地址 + 探活成功 → 放行并写入 sessionStorage 缓存
 * 4. 兜底地址 + 5 分钟内的缓存 → 放行且不重复探活
 */

import React from 'react';
import { render, screen, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { ServerConfigGate } from '@/components/ServerConfigGate';
import { clearApiBase, setApiBase } from '@/utils/apiBase';

type WindowWithTauri = Window & { __TAURI_INTERNALS__?: unknown };

/** 模拟桌面（Tauri）运行时：基址会落到 127.0.0.1:3001 兜底 */
function enableTauriRuntime(): void {
    (window as WindowWithTauri).__TAURI_INTERNALS__ = {};
}

function renderGate() {
    return render(
        <MantineProvider>
            <ServerConfigGate>
                <div data-testid="app-content">应用内容</div>
            </ServerConfigGate>
        </MantineProvider>,
    );
}

describe('ServerConfigGate', () => {
    const originalFetch = global.fetch;

    beforeEach(() => {
        window.localStorage.clear();
        window.sessionStorage.clear();
        delete (window as WindowWithTauri).__TAURI_INTERNALS__;
    });

    afterEach(() => {
        global.fetch = originalFetch;
        clearApiBase();
        window.localStorage.clear();
        window.sessionStorage.clear();
        delete (window as WindowWithTauri).__TAURI_INTERNALS__;
    });

    it('用户已保存地址时直接放行，不做探活', async () => {
        setApiBase('https://demo.example.com');
        const fetchMock = jest.fn();
        global.fetch = fetchMock as unknown as typeof fetch;

        renderGate();

        expect(await screen.findByTestId('app-content')).toBeInTheDocument();
        expect(fetchMock).not.toHaveBeenCalled();
    });

    it('兜底地址探活失败时阻断并展示配置表单', async () => {
        enableTauriRuntime();
        global.fetch = jest
            .fn()
            .mockRejectedValue(new TypeError('Failed to fetch')) as unknown as typeof fetch;

        renderGate();

        expect(await screen.findByTestId('server-config-gate')).toBeInTheDocument();
        expect(screen.getByText('先连接服务器')).toBeInTheDocument();
        expect(screen.getByTestId('server-address-input')).toHaveValue(
            'http://127.0.0.1:3001',
        );
        expect(screen.queryByTestId('app-content')).not.toBeInTheDocument();
    });

    it('兜底地址探活成功时放行并缓存结果', async () => {
        enableTauriRuntime();
        global.fetch = jest.fn().mockResolvedValue({
            ok: true,
            status: 200,
            text: async () => JSON.stringify({ status: 'ok', db: 'up' }),
        }) as unknown as typeof fetch;

        renderGate();

        expect(await screen.findByTestId('app-content')).toBeInTheDocument();
        await waitFor(() => {
            expect(window.sessionStorage.getItem('api_base_probe_ok')).toContain(
                'http://127.0.0.1:3001',
            );
        });
    });

    it('命中 5 分钟内的探活缓存时不再发请求', async () => {
        enableTauriRuntime();
        window.sessionStorage.setItem(
            'api_base_probe_ok',
            JSON.stringify({ origin: 'http://127.0.0.1:3001', at: Date.now() }),
        );
        const fetchMock = jest.fn();
        global.fetch = fetchMock as unknown as typeof fetch;

        renderGate();

        expect(await screen.findByTestId('app-content')).toBeInTheDocument();
        expect(fetchMock).not.toHaveBeenCalled();
    });
});
