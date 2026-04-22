import React, { useState } from 'react';
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
} from '@tabler/icons-react';

interface NavItemProps {
  icon: React.ReactNode;
  label: string;
  initiallyOpened?: boolean;
  links?: { label: string }[];
}

const NavItem: React.FC<NavItemProps> = ({ icon, label, initiallyOpened, links }) => {
  const hasLinks = Array.isArray(links);
  const [opened, setOpened] = useState(initiallyOpened || false);
  
  const ItemContent = (
    <Group justify="space-between" gap={0}>
      <Group gap="md">
        <ThemeIcon variant="light" size={30}>
          {icon}
        </ThemeIcon>
        <Text size="sm" fw={500}>
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
          onClick={() => setOpened((o) => !o)}
          style={{ width: '100%', padding: '10px 15px' }}
        >
          {ItemContent}
        </UnstyledButton>
        <Collapse in={opened}>
          {links.map((linkItem) => (
            <NavLink
              key={linkItem.label}
              label={linkItem.label}
              style={{ paddingLeft: 50 }}
            />
          ))}
        </Collapse>
      </>
    );
  }

  return (
    <NavLink
      leftSection={
        <ThemeIcon variant="light" size={30}>
          {icon}
        </ThemeIcon>
      }
      label={label}
      style={{ padding: '10px 15px' }}
    />
  );
};

const Sidebar: React.FC = () => {
  const navItems: NavItemProps[] = [
    {
      icon: <IconDashboard size={18} />,
      label: '仪表盘',
    },
    {
      icon: <IconDeviceDesktop size={18} />,
      label: '硬资产管理',
      initiallyOpened: false,
      links: [
        { label: '计算机设备' },
        { label: '网络设备' },
        { label: '服务器设备' },
        { label: '存储设备' },
        { label: '外设及终端' },
        { label: '移动设备' },
        { label: '所有硬资产' },
      ],
    },
    {
      icon: <IconLicense size={18} />,
      label: '软资产管理',
      initiallyOpened: false,
      links: [
        { label: '办公软件' },
        { label: '开发工具' },
        { label: '安全软件' },
        { label: '所有软资产' },
      ],
    },
    {
      icon: <IconListCheck size={18} />,
      label: '流程管理',
      initiallyOpened: false,
      links: [
        { label: '领用审批' },
        { label: '归还确认' },
        { label: '调拨流程' },
        { label: '维修流程' },
        { label: '报废流程' },
        { label: '所有流程' },
      ],
    },
    {
      icon: <IconChartBar size={18} />,
      label: '统计分析',
      initiallyOpened: false,
      links: [
        { label: '资产统计' },
        { label: '部门分布' },
        { label: '状态分析' },
        { label: '维保统计' },
        { label: '授权统计' },
        { label: '报表导出' },
      ],
    },
    {
      icon: <IconSettings size={18} />,
      label: '系统配置',
      initiallyOpened: false,
      links: [
        { label: '数据库配置' },
        { label: '权限管理' },
        { label: '资产分类' },
        { label: '部门管理' },
        { label: '用户管理' },
        { label: '系统日志' },
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
        
        <Box p="md" style={{ borderTop: '1px solid #e9ecef', marginTop: 'auto' }}>
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