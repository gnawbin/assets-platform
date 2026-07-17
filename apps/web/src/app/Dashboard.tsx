import React from 'react';
import {
  Card,
  Grid,
  Group,
  Text,
  Title,
  Progress,
  SimpleGrid,
  Paper,
  Stack,
  Badge,
  Button,
} from '@mantine/core';
import {
  IconDeviceDesktop,
  IconLicense,
  IconBuilding,
  IconChartBar,
  IconAlertCircle,
  IconCheck,
  IconClock,
  IconTrendingUp,
} from '@tabler/icons-react';

const Dashboard: React.FC = () => {
  // 模拟数据
  const stats = [
    { title: '总资产数', value: '1,248', icon: IconDeviceDesktop, color: 'blue', change: '+12%' },
    { title: '硬资产数', value: '856', icon: IconDeviceDesktop, color: 'green', change: '+8%' },
    { title: '软资产数', value: '392', icon: IconLicense, color: 'orange', change: '+15%' },
    { title: '部门数量', value: '24', icon: IconBuilding, color: 'violet', change: '+2%' },
  ];

  const assetStatus = [
    { label: '在用', value: 65, color: 'green' },
    { label: '库存', value: 15, color: 'blue' },
    { label: '维修', value: 8, color: 'orange' },
    { label: '闲置', value: 7, color: 'yellow' },
    { label: '报废', value: 5, color: 'red' },
  ];

  const upcomingEvents = [
    { title: '维保到期', count: 12, days: 7, icon: IconAlertCircle, color: 'red' },
    { title: 'License到期', count: 8, days: 15, icon: IconClock, color: 'orange' },
    { title: '待审批流程', count: 5, days: 3, icon: IconCheck, color: 'blue' },
  ];

  return (
    <Stack gap="lg">
      {/* 标题 */}
      <Group justify="space-between">
        <div>
          <Title order={2}>仪表盘</Title>
          <Text c="dimmed">欢迎使用IT设备资产管理系统</Text>
        </div>
        <Group>
          <Button variant="light">刷新数据</Button>
          <Button>生成报表</Button>
        </Group>
      </Group>

      {/* 统计卡片 */}
      <SimpleGrid cols={{ base: 1, sm: 2, lg: 4 }}>
        {stats.map((stat) => {
          const Icon = stat.icon;
          return (
            <Card key={stat.title} withBorder padding="lg" radius="md">
              <Group justify="space-between">
                <div>
                  <Text c="dimmed" size="xs" fw={700}>
                    {stat.title}
                  </Text>
                  <Text fw={700} size="xl">
                    {stat.value}
                  </Text>
                </div>
                <Icon size={32} color={stat.color} />
              </Group>
              <Group justify="space-between" mt="md">
                <Text size="sm" c={stat.change.startsWith('+') ? 'green' : 'red'}>
                  {stat.change} 较上月
                </Text>
                <IconTrendingUp size={16} />
              </Group>
            </Card>
          );
        })}
      </SimpleGrid>

      {/* 资产状态和即将到期 */}
      <Grid>
        <Grid.Col span={{ base: 12, lg: 8 }}>
          <Card withBorder padding="lg" radius="md">
            <Title order={3} mb="md">资产状态分布</Title>
            <Stack gap="md">
              {assetStatus.map((status) => (
                <div key={status.label}>
                  <Group justify="space-between" mb="xs">
                    <Text fw={500}>{status.label}</Text>
                    <Text fw={700}>{status.value}%</Text>
                  </Group>
                  <Progress value={status.value} color={status.color} size="lg" radius="xl" />
                </div>
              ))}
            </Stack>
          </Card>
        </Grid.Col>
        
        <Grid.Col span={{ base: 12, lg: 4 }}>
          <Card withBorder padding="lg" radius="md">
            <Title order={3} mb="md">即将到期事项</Title>
            <Stack gap="md">
              {upcomingEvents.map((event) => {
                const Icon = event.icon;
                return (
                  <Paper key={event.title} withBorder p="md" radius="md">
                    <Group justify="space-between">
                      <Group>
                        <Icon size={24} color={event.color} />
                        <div>
                          <Text fw={500}>{event.title}</Text>
                          <Text size="sm" c="dimmed">{event.count} 项</Text>
                        </div>
                      </Group>
                      <Badge color={event.color} variant="light">
                        {event.days}天内
                      </Badge>
                    </Group>
                  </Paper>
                );
              })}
            </Stack>
          </Card>
        </Grid.Col>
      </Grid>

      {/* 快速操作 */}
      <Card withBorder padding="lg" radius="md">
        <Title order={3} mb="md">快速操作</Title>
        <SimpleGrid cols={{ base: 2, sm: 4 }}>
          <Button variant="light" leftSection={<IconDeviceDesktop size={18} />}>
            新增硬资产
          </Button>
          <Button variant="light" leftSection={<IconLicense size={18} />}>
            新增软资产
          </Button>
          <Button variant="light" leftSection={<IconChartBar size={18} />}>
            查看报表
          </Button>
          <Button variant="light" leftSection={<IconBuilding size={18} />}>
            部门管理
          </Button>
        </SimpleGrid>
      </Card>

      {/* 系统信息 */}
      <Card withBorder padding="lg" radius="md">
        <Title order={3} mb="md">系统信息</Title>
        <SimpleGrid cols={{ base: 1, sm: 3 }}>
          <div>
            <Text fw={500}>系统版本</Text>
            <Text c="dimmed">v1.0.0</Text>
          </div>
          <div>
            <Text fw={500}>最后数据同步</Text>
            <Text c="dimmed">2024-01-15 14:30</Text>
          </div>
          <div>
            <Text fw={500}>数据库状态</Text>
            <Badge color="green" variant="light">正常</Badge>
          </div>
        </SimpleGrid>
      </Card>
    </Stack>
  );
};

export default Dashboard;
