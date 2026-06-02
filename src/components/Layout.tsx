'use client';

import React from 'react';
import { useRouter } from 'next/navigation';
import { AppShell, Burger, Group, Title, Text, Box, Button, Avatar, Menu } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { IconLogout, IconUser } from '@tabler/icons-react';
import Sidebar from './Sidebar';
import { useAuthStore } from '@/store/authStore';

interface LayoutProps {
  children: React.ReactNode;
}

const Layout: React.FC<LayoutProps> = ({ children }) => {
  const [opened, { toggle }] = useDisclosure();
  const router = useRouter();
  const { user, logout } = useAuthStore();

  const handleLogout = () => {
    logout();
    router.push('/login');
  };

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
        <Group h="100%" px="md" justify="space-between">
          <Group>
            <Burger opened={opened} onClick={toggle} hiddenFrom="sm" size="sm" />
            <Title order={4}>资产管理平台</Title>
          </Group>

          {user && (
            <Menu shadow="md" width={200}>
              <Menu.Target>
                <Button variant="subtle" leftSection={<Avatar size="sm" radius="xl" />}>
                  <Text size="sm" fw={500}>
                    {user.real_name || user.username}
                  </Text>
                </Button>
              </Menu.Target>

              <Menu.Dropdown>
                <Menu.Item leftSection={<IconUser size={14} />} disabled>
                  {user.real_name || user.username}
                </Menu.Item>
                <Menu.Divider />
                <Menu.Item
                  color="red"
                  leftSection={<IconLogout size={14} />}
                  onClick={handleLogout}
                >
                  退出登录
                </Menu.Item>
              </Menu.Dropdown>
            </Menu>
          )}
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
