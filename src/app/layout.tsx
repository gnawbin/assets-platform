'use client';
import { MantineProvider } from '@mantine/core';
import '@mantine/core/styles.css';

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="zh-CN">
      <body style={{ margin: 0, padding: 0 }}>
        <MantineProvider>{children}</MantineProvider>
      </body>
    </html>
  );
}