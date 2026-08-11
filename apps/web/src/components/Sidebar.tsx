'use client';

import React, { useEffect, useState } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import {
  NavLink,
  ScrollArea,
  Text,
  Group,
  ThemeIcon,
  UnstyledButton,
  Collapse,
  Box,
  Paper,
  Loader,
  Center,
} from '@mantine/core';
import {
  IconDashboard,
  IconBooks,
  IconDeviceDesktop,
  IconLicense,
  IconListCheck,
  IconChartBar,
  IconSettings,
  IconHierarchy,
  IconChevronDown,
} from '@tabler/icons-react';

import { getUserMenus, type MenuItem } from '@/services/menuService';
import { useAuthStore } from '@/store/authStore';

// ======================== 图标映射 ========================

/** 数据库图标名称 → React 组件映射 */
const ICON_MAP: Record<string, React.ReactNode> = {
  IconDashboard: <IconDashboard size={18} />,
  IconBooks: <IconBooks size={18} />,
  IconDeviceDesktop: <IconDeviceDesktop size={18} />,
  IconLicense: <IconLicense size={18} />,
  IconListCheck: <IconListCheck size={18} />,
  IconChartBar: <IconChartBar size={18} />,
  IconSettings: <IconSettings size={18} />,
  IconHierarchy: <IconHierarchy size={18} />,
};


/** 根据图标名称获取图标组件 */
function getIcon(iconName?: string): React.ReactNode {
  if (!iconName) return null;
  return ICON_MAP[iconName] ?? <IconDeviceDesktop size={18} />;
}

// ======================== 导航项组件 ========================

interface NavItemProps {
  item: MenuItem;
}

const NavItem: React.FC<NavItemProps> = ({ item }) => {
  const pathname = usePathname();
  const router = useRouter();
  const hasLinks = Array.isArray(item.children) && item.children.length > 0;

  // 判断当前路由是否匹配此菜单项或其子菜单
  const isActive = item.path ? pathname === item.path : false;
  const isChildActive = hasLinks
    ? item.children!.some((link) => pathname === link.path)
    : false;

  // 如果子菜单中有激活项，自动展开
  const [opened, setOpened] = useState(isChildActive || false);

  useEffect(() => {
    if (isChildActive) {
      setOpened(true);
    }
  }, [isChildActive]);

  const handleClick = () => {
    if (hasLinks) {
      setOpened((o) => !o);
    } else if (item.path) {
      router.push(item.path);
    }
  };

  const icon = getIcon(item.icon);

  const ItemContent = (
    <Group justify="space-between" gap={0}>
      <Group gap="md">
        <ThemeIcon
          variant={isActive || isChildActive ? 'filled' : 'light'}
          size={30}
          color={isActive || isChildActive ? 'blue' : 'gray'}
        >
          {icon}
        </ThemeIcon>
        <Text
          size="sm"
          fw={500}
          c={isActive || isChildActive ? 'blue' : 'dark'}
        >
          {item.label}
        </Text>
      </Group>
      {hasLinks && (
        <IconChevronDown
          size={16}
          style={{
            transform: opened ? 'rotate(180deg)' : 'none',
            transition: 'transform 200ms ease',
          }}
        />
      )}
    </Group>
  );

  if (hasLinks) {
    return (
      <>
        <UnstyledButton
          onClick={handleClick}
          style={{
            width: '100%',
            padding: '10px 15px',
            borderRadius: 8,
            backgroundColor: isChildActive ? 'var(--mantine-color-blue-light)' : 'transparent',
            transition: 'background-color 200ms ease',
          }}
        >
          {ItemContent}
        </UnstyledButton>
        <Collapse expanded={opened}>
          {item.children!.map((child) => (
            <Box key={child.label} style={{ paddingLeft: 16 }}>
              <NavItem item={child} />
            </Box>
          ))}
        </Collapse>
      </>
    );
  }

  return (
    <NavLink
      leftSection={
        <ThemeIcon
          variant={isActive ? 'filled' : 'light'}
          size={30}
          color={isActive ? 'blue' : 'gray'}
        >
          {icon}
        </ThemeIcon>
      }
      label={
        <Text fw={500} c={isActive ? 'blue' : 'dark'}>
          {item.label}
        </Text>
      }
      active={isActive}
      onClick={handleClick}
      style={{
        padding: '10px 15px',
        borderRadius: 8,
        margin: '2px 0',
      }}
    />
  );
};

// ======================== 侧边栏组件 ========================

const Sidebar: React.FC = () => {
  const [menuItems, setMenuItems] = useState<MenuItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const { user } = useAuthStore();

  useEffect(() => {
    let mounted = true;

    async function fetchMenus() {
      // 如果 user 为 null，说明 auth 尚未初始化完成，跳过请求
      if (!user) {
        console.log('[Sidebar] user 为 null，等待 auth 初始化...');
        if (mounted) {
          setLoading(true);
        }
        return;
      }

      const userIdStr = user.id?.toString();
      console.log('[Sidebar] 开始加载菜单, user:', { id: user.id, username: user.username, is_super_admin: user.is_super_admin });
      console.log('[Sidebar] 调用 getUserMenus, userId:', userIdStr);

      try {
        setLoading(true);
        setError(null);
        // 传递当前用户ID，后端根据用户角色过滤菜单
        const data = await getUserMenus(userIdStr);
        console.log('[Sidebar] getUserMenus 返回结果:', { count: data.length, data });
        if (mounted) {
          setMenuItems(data);
        }
      } catch (err) {
        console.error('[Sidebar] getUserMenus 调用失败:', err);
        if (mounted) {
          setError(err instanceof Error ? err.message : '加载菜单失败');
        }
      } finally {
        if (mounted) {
          setLoading(false);
        }
      }
    }

    fetchMenus();

    return () => {
      mounted = false;
    };
  }, [user?.id]);

  return (
    <Paper withBorder h="100%" w={280} style={{ overflow: 'hidden' }}>
      <ScrollArea h="100%">
        <Box p="md">
          <Box py="md">
            {loading ? (
              <Center py="xl">
                <Loader size="sm" />
              </Center>
            ) : error ? (
              <Text c="red" size="sm" ta="center" py="xl">
                {error}
              </Text>
            ) : menuItems.length === 0 && !loading ? (
              <>
                <NavItem item={{ label: 'AI 工作流', path: '/knowledge/workflow', icon: 'IconHierarchy' }} />
              </>
            ) : (
              menuItems.map((item, index) => (
                <NavItem key={item.label + index} item={item} />
              ))
            )}
          </Box>
        </Box>

        <Box
          p="md"
          style={{ borderTop: '1px solid var(--mantine-color-gray-3)', marginTop: 'auto' }}
        >
          <Text size="xs" c="dimmed" mb={5}>
            IT设备资产管理系统
          </Text>
          <Text size="sm" fw={500}>
            v0.0.9
          </Text>
        </Box>
      </ScrollArea>
    </Paper>
  );
};

export default Sidebar;
