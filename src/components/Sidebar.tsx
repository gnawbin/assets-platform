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
} from '@mantine/core';
import {
  IconDashboard,
  IconDeviceDesktop,
  IconLicense,
  IconListCheck,
  IconChartBar,
  IconSettings,
  IconChevronDown,
  IconBooks,
} from '@tabler/icons-react';

interface SubLink {
  label: string;
  path: string;
}

interface NavItemProps {
  icon: React.ReactNode;
  label: string;
  path?: string;
  initiallyOpened?: boolean;
  links?: SubLink[];
}

const NavItem: React.FC<NavItemProps> = ({ icon, label, path, initiallyOpened, links }) => {
  const pathname = usePathname();
  const router = useRouter();
  const hasLinks = Array.isArray(links) && links.length > 0;

  // 判断当前路由是否匹配此菜单项或其子菜单
  const isActive = path ? pathname === path : false;
  const isChildActive = hasLinks
    ? links!.some((link) => pathname === link.path)
    : false;

  // 如果子菜单中有激活项，自动展开
  const [opened, setOpened] = useState(initiallyOpened || isChildActive || false);

  useEffect(() => {
    if (isChildActive) {
      setOpened(true);
    }
  }, [isChildActive]);

  const handleClick = () => {
    if (hasLinks) {
      setOpened((o) => !o);
    } else if (path) {
      router.push(path);
    }
  };

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
          {label}
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
          {links.map((linkItem) => {
            const isLinkActive = pathname === linkItem.path;
            return (
              <NavLink
                key={linkItem.label}
                label={linkItem.label}
                active={isLinkActive}
                onClick={() => router.push(linkItem.path)}
                style={{
                  paddingLeft: 50,
                  borderRadius: 8,
                  margin: '2px 0',
                }}
              />
            );
          })}
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
          {label}
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

const Sidebar: React.FC = () => {
  const navItems: NavItemProps[] = [
    {
      icon: <IconDashboard size={18} />,
      label: '仪表盘',
      path: '/',
    },
    {
      icon: <IconBooks size={18} />,
      label: '资产台账',
      links: [
     { label: '硬资产', path: '/hardware' },
    
        { label: '软资产', path: '/software' },
       
      ],
    },
    {
      icon: <IconListCheck size={18} />,
      label: '流程管理',
      links: [
        { label: '领用审批', path: '/process/approval' },
        { label: '归还确认', path: '/process/return' },
        { label: '调拨流程', path: '/process/transfer' },
        { label: '维修流程', path: '/process/maintenance' },
        { label: '报废流程', path: '/process/scrap' },
        { label: '所有流程', path: '/process/all' },
      ],
    },
    {
      icon: <IconChartBar size={18} />,
      label: '统计分析',
      links: [
        { label: '资产统计', path: '/statistics/assets' },
        { label: '部门分布', path: '/statistics/department' },
        { label: '状态分析', path: '/statistics/status' },
        { label: '维保统计', path: '/statistics/maintenance' },
        { label: '授权统计', path: '/statistics/license' },
        { label: '报表导出', path: '/statistics/export' },
      ],
    },
    {
      icon: <IconSettings size={18} />,
      label: '系统配置',
      links: [
        { label: '数据库配置', path: '/settings/database' },
        { label: '权限管理', path: '/settings/permissions' },
        { label: '资产分类', path: '/settings/categories' },
        { label: '部门管理', path: '/settings/departments' },
        { label: '用户管理', path: '/settings/users' },
        { label: '系统日志', path: '/settings/logs' },
      ],
    },
  ];

  return (
    <Paper withBorder h="100%" w={280} style={{ overflow: 'hidden' }}>
      <ScrollArea h="100%">
        <Box p="md">
          <Box py="md">
            {navItems.map((item, index) => (
              <NavItem key={index} {...item} />
            ))}
          </Box>
        </Box>

        <Box
          p="md"
          style={{ borderTop: '1px solid #e9ecef', marginTop: 'auto' }}
        >
          <Text size="xs" c="dimmed" mb={5}>
            IT设备资产管理系统
          </Text>
          <Text size="sm" fw={500}>
            v1.0.0
          </Text>
        </Box>
      </ScrollArea>
    </Paper>
  );
};

export default Sidebar;
