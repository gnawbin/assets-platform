'use client';
import { useEffect } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import { MantineProvider } from '@mantine/core';
import '@mantine/core/styles.css';
import { useAuthStore } from '@/store/authStore';
import { initTelemetry } from '@/utils/telemetry';
import { logger } from '@/utils/logger';

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
        <MantineProvider>{children}</MantineProvider>
      </body>
    </html>
  );
}
