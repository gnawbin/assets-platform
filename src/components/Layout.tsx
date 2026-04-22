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
      padding="md"
    >
      <AppShell.Header>
        <Group h="100%" px="md" justify="space-between">
          <Group>
            <Burger
              opened={opened}
              onClick={toggle}
              hiddenFrom="sm"
              size="sm"
            />
            <Group gap="xs">
              <Title order={4} c="blue">
                IT设备资产管理系统
              </Title>
              <Text size="sm" c="dimmed">
                - 硬资产 + 软资产
              </Text>
            </Group>
          </Group>
          
          <Group gap="md">
            <Text size="sm">当前用户: 管理员</Text>
            <Text size="sm" c="dimmed">
              部门: IT部
            </Text>
          </Group>
        </Group>
      </AppShell.Header>

      <AppShell.Navbar p="md">
        <Sidebar />
      </AppShell.Navbar>

      <AppShell.Main>
        <Box p="md">
          {children}
        </Box>
      </AppShell.Main>
    </AppShell>
  );
};

export default Layout;
