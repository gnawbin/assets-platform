'use client';

import React, { useState } from 'react';
import { useRouter } from 'next/navigation';
import Link from 'next/link';
import {
    Container,
    Paper,
    Title,
    Text,
    TextInput,
    PasswordInput,
    Textarea,
    Button,
    Stack,
    Alert,
    Center,
    Box,
    Anchor,
} from '@mantine/core';
import { IconAlertCircle, IconUserPlus } from '@tabler/icons-react';
import { api } from '@/utils/api';

const RegisterPage: React.FC = () => {
    const router = useRouter();
    const [username, setUsername] = useState('');
    const [password, setPassword] = useState('');
    const [confirmPassword, setConfirmPassword] = useState('');
    const [realName, setRealName] = useState('');
    const [email, setEmail] = useState('');
    const [phone, setPhone] = useState('');
    const [departmentName, setDepartmentName] = useState('');
    const [companyName, setCompanyName] = useState('');
    const [reason, setReason] = useState('');
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const handleRegister = async () => {
        if (!username.trim()) { setError('请输入用户名'); return; }
        if (!password.trim()) { setError('请输入密码'); return; }
        if (password.length < 6) { setError('密码长度不能少于6位'); return; }
        if (password !== confirmPassword) { setError('两次密码输入不一致'); return; }
        if (!realName.trim()) { setError('请输入真实姓名'); return; }

        setLoading(true);
        setError(null);

        try {
            // Tauri 后端要求参数为驼峰命名，直接构造 payload
            const payload: Record<string, unknown> = {
                username: username.trim(),
                password,
                realName: realName.trim(),
                email: email.trim() || undefined,
                phone: phone.trim() || undefined,
                departmentName: departmentName.trim() || undefined,
                companyName: companyName.trim() || undefined,
                reason: reason.trim() || undefined,
            };
            await api.post('register', payload);
            // 注册成功，跳转到登录页
            router.push('/login?registered=true');
        } catch (err) {
            console.error('注册失败:', err);
            setError(typeof err === 'string' ? err : '注册失败，请稍后重试');
        } finally {
            setLoading(false);
        }
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === 'Enter') {
            handleRegister();
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
            <Container size={480}>
                <Paper withBorder shadow="xl" p="xl" radius="md">
                    <Stack gap="lg">
                        <Center>
                            <IconUserPlus size={48} style={{ color: 'var(--mantine-color-blue-6)' }} />
                        </Center>

                        <Title order={2} ta="center">
                            用户注册
                        </Title>
                        <Text c="dimmed" size="sm" ta="center">
                            填写以下信息提交注册申请，等待管理员审核
                        </Text>

                        {error && (
                            <Alert icon={<IconAlertCircle size={16} />} title="注册失败" color="red" variant="light">
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
                            placeholder="请输入密码（至少6位）"
                            required
                            value={password}
                            onChange={(e) => setPassword(e.target.value)}
                            onKeyDown={handleKeyDown}
                        />

                        <PasswordInput
                            label="确认密码"
                            placeholder="请再次输入密码"
                            required
                            value={confirmPassword}
                            onChange={(e) => setConfirmPassword(e.target.value)}
                            onKeyDown={handleKeyDown}
                        />

                        <TextInput
                            label="真实姓名"
                            placeholder="请输入真实姓名"
                            required
                            value={realName}
                            onChange={(e) => setRealName(e.target.value)}
                            onKeyDown={handleKeyDown}
                        />

                        <TextInput
                            label="邮箱"
                            placeholder="请输入邮箱（可选）"
                            value={email}
                            onChange={(e) => setEmail(e.target.value)}
                            onKeyDown={handleKeyDown}
                        />

                        <TextInput
                            label="手机号码"
                            placeholder="请输入手机号码（可选）"
                            value={phone}
                            onChange={(e) => setPhone(e.target.value)}
                            onKeyDown={handleKeyDown}
                        />

                        <TextInput
                            label="部门"
                            placeholder="请输入所在部门（可选）"
                            value={departmentName}
                            onChange={(e) => setDepartmentName(e.target.value)}
                            onKeyDown={handleKeyDown}
                        />

                        <TextInput
                            label="公司/单位"
                            placeholder="请输入公司或单位名称（可选）"
                            value={companyName}
                            onChange={(e) => setCompanyName(e.target.value)}
                            onKeyDown={handleKeyDown}
                        />

                        <Textarea
                            label="注册理由"
                            placeholder="请简述注册原因（可选）"
                            value={reason}
                            onChange={(e) => setReason(e.target.value)}
                            minRows={2}
                        />

                        <Button fullWidth size="md" onClick={handleRegister} loading={loading}>
                            提交注册申请
                        </Button>

                        <Text ta="center" size="sm">
                            已有账号？{' '}
                            <Anchor component={Link} href="/login">
                                立即登录
                            </Anchor>
                        </Text>
                    </Stack>
                </Paper>
            </Container>
        </Box>
    );
};

export default RegisterPage;