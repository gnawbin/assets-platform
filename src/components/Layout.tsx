'use client';

import React from 'react';
import { AppShell, Burger, Group, Title, Text, Box } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import Sidebar from './Sidebar';

interface LayoutProps {
  children: React.ReactNode;
}

const Layout: React.FC<LayoutProps> = ({ children }) => {
  const [opened, { toggle }] = useDisclosure();

  return (
    <AppShell
      header={{ height: 60 }}
      navbar={{
        width: 280,
        breakpoint: 'sm',
        collapsed: { mobile: !opened },
      }}
    >
      {/* 顶部导航栏 */}
      <AppShell.Header>
        <Group h="100%" px="md">
          <Burger opened={opened} onClick={toggle} hiddenFrom="sm" size="sm" />
          <Title order={4}>资产管理平台</Title>
        </Group>
      </AppShell.Header>

      {/* 左侧边栏 */}
      <AppShell.Navbar p="md">
        <Sidebar />
      </AppShell.Navbar>

      {/* 主内容区域 */}
      <AppShell.Main>
        <Box p="lg">
          {children}
        </Box>
      </AppShell.Main>
    </AppShell>
  );
};

export default Layout;
