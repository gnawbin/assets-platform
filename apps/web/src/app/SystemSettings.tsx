import React from 'react';
import { Card, Text, Title, Stack, Group, Badge, Button } from '@mantine/core';
import { IconSettings, IconRefresh } from '@tabler/icons-react';

interface SystemSettingsProps {
  title: string;
}

const SystemSettings: React.FC<SystemSettingsProps> = ({ title }) => {
  return (
    <Stack gap="lg">
      <Group justify="space-between">
        <div>
          <Title order={2}>{title}</Title>
          <Text c="dimmed">系统配置 - {title}</Text>
        </div>
        <Group>
          <Button variant="light" leftSection={<IconRefresh size={16} />}>
            刷新
          </Button>
          <Button leftSection={<IconSettings size={16} />}>
            保存设置
          </Button>
        </Group>
      </Group>

      <Card withBorder padding="lg" radius="md">
        <Stack gap="md" align="center" py="xl">
          <Text c="dimmed" size="lg">
            {title} - 功能开发中
          </Text>
          <Badge size="lg" variant="light" color="gray">
            即将上线
          </Badge>
        </Stack>
      </Card>
    </Stack>
  );
};

export default SystemSettings;
