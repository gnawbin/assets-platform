'use client';
import { useEffect } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import { MantineProvider } from '@mantine/core';
import '@mantine/core/styles.css';
import { Notifications } from '@mantine/notifications';
import '@mantine/notifications/styles.css';
import { useAuthStore } from '@/store/authStore';
import { useAutoLogout } from '@/hooks/useAutoLogout';
import { initTelemetry } from '@/utils/telemetry';
import { logger } from '@/utils/logger';
import { ServerConfigGate } from '@/components/ServerConfigGate';

/** 未登录也允许访问的路径：登录 / 注册 / 服务端连接配置 */
const PUBLIC_PATHS = ['/login', '/register', '/settings/server'];

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  const pathname = usePathname();
  const router = useRouter();
  const { isLoggedIn, isInitializing, init } = useAuthStore();

  // 用户无操作 3 分钟自动登出
  useAutoLogout();

  // 仅在应用启动时初始化一次 OpenTelemetry
  useEffect(() => {
    initTelemetry();
  }, []);

  useEffect(() => {
    logger.info('应用启动', { page: pathname });
  }, [pathname]);

  useEffect(() => {
    init().catch((err) => {
      logger.error('初始化认证状态失败', err);
    });
  }, [init]);

  useEffect(() => {
    // 初始化完成前不做任何路由跳转，避免竞态条件
    if (isInitializing) {
      return;
    }

    // 如果未登录且不在公开页面，重定向到登录页
    if (!isLoggedIn && !PUBLIC_PATHS.includes(pathname)) {
      logger.info('未登录，重定向到登录页');
      router.push('/login');
    }
    // 如果已登录且在登录页面，重定向到首页
    if (isLoggedIn && pathname === '/login') {
      logger.info('已登录，重定向到首页');
      router.push('/');
    }
  }, [isLoggedIn, isInitializing, pathname, router]);

  return (
    <html lang="zh-CN">
      <body style={{ margin: 0, padding: 0 }}>
        <MantineProvider>
          <Notifications />
          {/* 服务器不可达时先让用户填地址，避免首屏全是网络错误 */}
          <ServerConfigGate>{children}</ServerConfigGate>
        </MantineProvider>
      </body>
    </html>
  );
}
