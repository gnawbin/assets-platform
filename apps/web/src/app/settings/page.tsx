import React from 'react';
import Link from 'next/link';
import Layout from '@/components/Layout';
import {
  Title,
  Text,
  Card,
  SimpleGrid,
  Group,
  ThemeIcon,
  Stack,
} from '@mantine/core';
import {
  IconUsers,
  IconBuildingCommunity,
  IconCategory,
  IconShield,
  IconSettings,
  IconBuildingStore,
  IconFileText,
  IconUserCheck,
  IconHash,
} from '@tabler/icons-react';

const settingsItems = [
  {
    title: '用户管理',
    description: '管理系统用户账号、角色分配和权限控制',
    icon: IconUsers,
    color: 'blue',
    href: '/settings/users',
  },
  {
    title: '部门管理',
    description: '管理组织架构和部门层级关系',
    icon: IconBuildingCommunity,
    color: 'teal',
    href: '/settings/departments',
  },
  {
    title: '资产分类',
    description: '管理固定资产和无形资产的分类体系',
    icon: IconCategory,
    color: 'violet',
    href: '/settings/categories',
  },
  {
    title: '权限管理',
    description: '管理角色权限和菜单配置',
    icon: IconShield,
    color: 'grape',
    href: '/settings/permissions',
  },
  {
    title: '租户管理',
    description: '管理多租户配置和 Schema 隔离',
    icon: IconBuildingStore,
    color: 'orange',
    href: '/settings/tenants',
  },
  {
    title: '注册审核',
    description: '审核用户注册申请，通过后自动创建用户并分配租户',
    icon: IconUserCheck,
    color: 'lime',
    href: '/settings/registrations',
  },
  {
    title: '流程设计',
    description: '配置审批流程和节点设置',
    icon: IconFileText,
    color: 'cyan',
    href: '/settings/process-design',
  },
  {
    title: '编号规则',
    description: '配置单据编号生成规则',
    icon: IconHash,
    color: 'indigo',
    href: '/settings/numbering',
  },
  {
    title: '操作日志',
    description: '查看系统操作记录和审计日志',
    icon: IconSettings,
    color: 'gray',
    href: '/settings/logs',
  },
];

const SettingsPage: React.FC = () => {
  return (
    <Layout>
      <Stack gap="lg">
        <div>
          <Title order={2}>系统配置</Title>
          <Text c="dimmed">管理系统各项配置和基础数据</Text>
        </div>

        <SimpleGrid cols={{ base: 1, sm: 2, md: 3, lg: 4 }} spacing="lg">
          {settingsItems.map((item) => (
            <Card
              key={item.title}
              component={Link}
              href={item.href}
              withBorder
              padding="lg"
              radius="md"
              style={{
                cursor: 'pointer',
                textDecoration: 'none',
                transition: 'transform 0.1s, box-shadow 0.1s',
              }}
              styles={{
                root: {
                  '&:hover': {
                    transform: 'translateY(-2px)',
                    boxShadow: '0 4px 12px rgba(0,0,0,0.1)',
                  },
                },
              }}
            >
              <Group>
                <ThemeIcon size="lg" radius="md" color={item.color}>
                  <item.icon size={20} />
                </ThemeIcon>
                <div>
                  <Text fw={500} size="sm">
                    {item.title}
                  </Text>
                  <Text size="xs" c="dimmed" lineClamp={2}>
                    {item.description}
                  </Text>
                </div>
              </Group>
            </Card>
          ))}
        </SimpleGrid>
      </Stack>
    </Layout>
  );
};

export default SettingsPage;
