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
  // 优先级：环境变量 > 自动检测 > 默认值
  useEffect(() => {
    const envAdapter = process.env.NEXT_PUBLIC_API_ADAPTER;
    if (envAdapter === 'tauri' || envAdapter === 'http') {
      logger.info(`[Adapter] 使用环境变量配置: ${envAdapter}`);
      setAdapter(envAdapter);
      return;
    }

    // 未设置环境变量时，自动检测
    const isTauri = typeof window !== 'undefined' && window.__TAURI__ !== undefined;
    if (isTauri) {
      logger.info('[Adapter] 检测到 Tauri 环境，使用 Tauri 适配器');
      setAdapter('tauri');
    } else {
      logger.info('[Adapter] 未检测到 Tauri 环境，默认使用 Tauri 适配器');
      setAdapter('tauri');
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
