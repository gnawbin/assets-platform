'use client';
import { useEffect } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import { MantineProvider } from '@mantine/core';
import '@mantine/core/styles.css';
import { Notifications } from '@mantine/notifications';
import '@mantine/notifications/styles.css';
import { useAuthStore } from '@/store/authStore';
import { initTelemetry } from '@/utils/telemetry';
import { logger } from '@/utils/logger';
import { setAdapter } from '@/utils/adapters';

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  const pathname = usePathname();
  const router = useRouter();
  const { isLoggedIn, init } = useAuthStore();

  // 仅在应用启动时初始化一次 OpenTelemetry
  useEffect(() => {
    initTelemetry();
  }, []);

  // 自动检测运行环境并设置适配器
  // 优先级：环境变量 NEXT_PUBLIC_API_ADAPTER > 自动检测 > 默认 HTTP
  useEffect(() => {
    const envAdapter = process.env.NEXT_PUBLIC_API_ADAPTER;
    if (envAdapter === 'tauri' || envAdapter === 'http') {
      logger.info(`[Adapter] 使用环境变量配置: ${envAdapter}`);
      setAdapter(envAdapter);
      return;
    }

    // 未设置环境变量时，自动检测
    // Tauri v2 使用 __TAURI_INTERNALS__ 作为运行环境标记，兼容 v1 的 __TAURI__
    const isTauri = typeof window !== 'undefined' && (
      (window as Window).__TAURI_INTERNALS__ !== undefined ||
      (window as Window).__TAURI__ !== undefined
    );
    if (isTauri) {
      logger.info('[Adapter] 检测到 Tauri 环境，使用 Tauri 适配器');
      setAdapter('tauri');
    } else {
      logger.info('[Adapter] 未检测到 Tauri 环境，使用 HTTP 适配器');
      setAdapter('http');
    }
  }, []);



  useEffect(() => {
    logger.info('应用启动', { page: pathname });
  }, [pathname]);

  useEffect(() => {
    init();
  }, [init]);

  useEffect(() => {
    // 如果未登录且不在登录页面，重定向到登录页
    if (!isLoggedIn && pathname !== '/login') {
      logger.info('未登录，重定向到登录页');
      router.push('/login');
    }
    // 如果已登录且在登录页面，重定向到首页
    if (isLoggedIn && pathname === '/login') {
      logger.info('已登录，重定向到首页');
      router.push('/');
    }
  }, [isLoggedIn, pathname, router]);

  return (
    <html lang="zh-CN">
      <body style={{ margin: 0, padding: 0 }}>
        <MantineProvider>
          <Notifications />
          {children}
        </MantineProvider>
      </body>
    </html>
  );
}
