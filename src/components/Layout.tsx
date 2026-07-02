'use client';

import React from 'react';
import { useRouter } from 'next/navigation';
import {
  AppShell,
  Burger,
  Group,
  Title,
  Text,
  Box,
  Button,
  Avatar,
  Menu,
  Radio,
  Stack,
} from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { IconLogout, IconUser, IconBuildingStore } from '@tabler/icons-react';
import Sidebar from './Sidebar';
import { useAuthStore } from '@/store/authStore';
import { api } from '@/utils/api';

interface LayoutProps {
  children: React.ReactNode;
}

const Layout: React.FC<LayoutProps> = ({ children }) => {
  const [opened, { toggle }] = useDisclosure();
  const router = useRouter();
  const { user, logout, availableTenants, selectedTenantId, switchTenant } = useAuthStore();

  const handleLogout = () => {
    logout();
    router.push('/login');
  };

  const handleSwitchTenant = async (value: string) => {
    const tenantId = Number(value);
    if (!isNaN(tenantId)) {
      try {
        // 调用后端 API 切换租户（更新服务端 USER_TENANT_CACHE）
        // Tauri 命令需要 user_id + tenant_id，HTTP API 从 JWT 获取 user_id
        await api.post('switch_tenant', {
          user_id: String(user?.id ?? ''),
          tenant_id: value,
        });
        // 更新前端本地状态
        switchTenant(tenantId);
        window.location.reload();
      } catch (err) {
        console.error('切换租户失败:', err);
      }
    }
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

          <Group gap="xs">
            {/* 租户切换器：有可用租户时显示，方便查看当前租户 */}
            {availableTenants.length > 0 && (
              <Menu shadow="md" width={220}>
                <Menu.Target>
                  <Button
                    variant="light"
                    size="sm"
                    leftSection={<IconBuildingStore size={16} />}
                  >
                    {availableTenants.find(t => t.id === selectedTenantId)?.tenant_name || '切换租户'}
                  </Button>
                </Menu.Target>
                <Menu.Dropdown>
                  <Radio.Group
                    value={String(selectedTenantId)}
                    onChange={handleSwitchTenant}
                  >
                    <Stack gap={4} px="sm" py="xs">
                      {availableTenants.map((t) => (
                        <Radio
                          key={t.id}
                          value={String(t.id)}
                          label={t.tenant_name}
                          size="sm"
                        />
                      ))}
                    </Stack>
                  </Radio.Group>
                </Menu.Dropdown>
              </Menu>
            )}

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