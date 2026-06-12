import React from 'react';
import { Card, Text, Title, Stack, Group, Button } from '@mantine/core';
import { useRouter } from 'next/navigation';
import { IconArrowLeft } from '@tabler/icons-react';

const NotFound: React.FC = () => {
  const router = useRouter();

  return (
    <Stack gap="lg" align="center" justify="center" h="60vh">
      <Title order={1} c="dimmed" style={{ fontSize: 80 }}>
        404
      </Title>
      <Title order={3}>页面未找到</Title>
      <Text c="dimmed" size="lg">
        您访问的页面不存在或已被移除
      </Text>
      <Button
        leftSection={<IconArrowLeft size={16} />}
        onClick={() => router.push('/')}
        mt="md"
      >
        返回首页
      </Button>
    </Stack>
  );
};

export default NotFound;
