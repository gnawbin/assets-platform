'use client';

import React, { useState } from 'react';
import { useRouter } from 'next/navigation';
import {
  Container,
  Paper,
  Title,
  Text,
  TextInput,
  PasswordInput,
  Button,
  Stack,
  Alert,
  Center,
  Box,
} from '@mantine/core';
import { IconAlertCircle, IconLogin } from '@tabler/icons-react';
import { invoke } from '@tauri-apps/api/core';
import { useAuthStore } from '@/store/authStore';

const LoginPage: React.FC = () => {
  const router = useRouter();
  const login = useAuthStore((state) => state.login);
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleLogin = async () => {
    if (!username.trim()) {
      setError('请输入用户名');
      return;
    }
    if (!password.trim()) {
      setError('请输入密码');
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const user = await invoke<{
        id: number;
        username: string;
        real_name: string;
        email: string | null;
        phone: string | null;
        department_id: number | null;
        status: number;
        nickname: string | null;
        avatar: string | null;
      }>('login', { username: username.trim(), password });

      login(user);
      router.push('/');
    } catch (err) {
      console.error('登录失败:', err);
      setError(typeof err === 'string' ? err : '登录失败，请检查用户名和密码');
    } finally {
      setLoading(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      handleLogin();
    }
  };

  return (
    <Box
      style={{
        minHeight: '100vh',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        background: 'linear-gradient(135deg, #667eea 0%, #764ba2 100%)',
      }}
    >
      <Container size={420}>
        <Paper withBorder shadow="xl" p="xl" radius="md">
          <Stack gap="lg">
            <Center>
              <IconLogin size={48} style={{ color: 'var(--mantine-color-blue-6)' }} />
            </Center>

            <Title order={2} ta="center">
              资产管理平台
            </Title>
            <Text c="dimmed" size="sm" ta="center">
              请输入您的账号和密码登录系统
            </Text>

            {error && (
              <Alert icon={<IconAlertCircle size={16} />} title="登录失败" color="red" variant="light">
                {error}
              </Alert>
            )}

            <TextInput
              label="用户名"
              placeholder="请输入用户名"
              required
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              onKeyDown={handleKeyDown}
              autoFocus
            />

            <PasswordInput
              label="密码"
              placeholder="请输入密码"
              required
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              onKeyDown={handleKeyDown}
            />

            <Button fullWidth size="md" onClick={handleLogin} loading={loading}>
              登 录
            </Button>
          </Stack>
        </Paper>
      </Container>
    </Box>
  );
};

export default LoginPage;
