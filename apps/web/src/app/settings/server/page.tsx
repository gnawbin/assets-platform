'use client';

/**
 * 服务端连接（地址配置 + 连接诊断）
 *
 * 为什么**不套 `Layout`**：本页必须在**未登录**状态下也能访问 ——
 * 用户装好桌面端 / 打开静态托管的 Web 版时，第一件事就是填后端地址，
 * 此时还没有令牌，`Layout`（侧边栏 + 用户菜单）既无数据可用，也会被
 * `app/layout.tsx` 的未登录重定向挡在门外（该路径已在重定向白名单里）。
 *
 * 诊断项（对齐《桌面与Web全HTTP改造清单.md》§5.4）：
 * ① `GET /healthz` 可达性与 db 状态 ② `/api-docs/openapi.json` 的 info.version
 * ③ 往返延迟 ④ 当前页面 Origin ⑤ 解析后的 API 基址与来源
 */

import React, { useCallback, useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import Link from 'next/link';
import {
    Alert,
    Badge,
    Button,
    Card,
    Center,
    Code,
    Container,
    Group,
    Loader,
    SimpleGrid,
    Stack,
    Table,
    Text,
    Title,
} from '@mantine/core';
import {
    IconAlertTriangle,
    IconArrowLeft,
    IconCheck,
    IconCopy,
    IconRefresh,
} from '@tabler/icons-react';
import {
    describeApiBase,
    probeServer,
    type ApiBaseSource,
    type ProbeResult,
} from '@/utils/apiBase';
import { ServerConnectionForm } from '@/components/ServerConnectionForm';

/** 地址来源的中文说明 */
const SOURCE_TEXT: Record<ApiBaseSource, string> = {
    injected: '宿主注入（window.__API_BASE__）',
    storage: '本页保存（localStorage）',
    env: '构建期环境变量 NEXT_PUBLIC_API_BASE_URL',
    'tauri-default': '桌面内置服务端兜底（127.0.0.1:3001）',
    'dev-default': '开发模式兜底（localhost:3001）',
    'same-origin': '浏览器同源反代（location.origin + /api）',
    none: '未确定',
};

/** 由部署固定、不建议在页面上覆盖的来源 */
const DEPLOYMENT_SOURCES: ApiBaseSource[] = ['env', 'injected', 'same-origin'];

interface Diagnostics {
    probe: ProbeResult;
    openapiVersion: string | null;
    origin: string | null;
}

/** `describeApiBase()` 的快照类型 */
type ApiSnapshot = ReturnType<typeof describeApiBase>;

export default function ServerSettingsPage() {
    const router = useRouter();
    // 初始为 null：静态导出会预渲染本页，首帧与服务端保持一致，避免水合不一致
    const [snapshot, setSnapshot] = useState<ApiSnapshot | null>(null);
    const [diagnostics, setDiagnostics] = useState<Diagnostics | null>(null);
    const [loading, setLoading] = useState(false);

    const runDiagnostics = useCallback(async () => {
        const current = describeApiBase();
        setSnapshot(current);
        if (!current.origin) {
            setDiagnostics(null);
            return;
        }
        setLoading(true);
        try {
            const probe = await probeServer(current.origin);
            let openapiVersion: string | null = null;
            if (probe.ok) {
                try {
                    const res = await fetch(`${current.origin}/api-docs/openapi.json`);
                    if (res.ok) {
                        const doc = await res.json();
                        openapiVersion = doc?.info?.version ?? null;
                    }
                } catch {
                    // 文档未挂载 / 被拦截：不是错误，诊断里显示「未启用」
                }
            }
            setDiagnostics({ probe, openapiVersion, origin: current.origin });
        } finally {
            setLoading(false);
        }
    }, []);

    // 首屏自动跑一次诊断
    useEffect(() => {
        void runDiagnostics();
    }, [runDiagnostics]);

    const handleCopy = async () => {
        const lines = [
            `基址: ${snapshot?.base ?? '未配置'}`,
            `来源: ${snapshot ? SOURCE_TEXT[snapshot.source] : '-'}`,
            `页面 Origin: ${typeof window === 'undefined' ? '-' : window.location.origin}`,
            `healthz: ${
                diagnostics
                    ? `${diagnostics.probe.status ?? '无响应'} / ${diagnostics.probe.latencyMs}ms`
                    : '未探测'
            }`,
            `db: ${diagnostics?.probe.body?.db ?? '-'}`,
            `openapi version: ${diagnostics?.openapiVersion ?? '-'}`,
            `user-agent: ${typeof navigator === 'undefined' ? '-' : navigator.userAgent}`,
        ];
        try {
            await navigator.clipboard.writeText(lines.join('\n'));
        } catch {
            // 剪贴板不可用（非安全上下文）时静默失败
        }
    };

    const isDeploymentFixed = !!snapshot && DEPLOYMENT_SOURCES.includes(snapshot.source);

    return (
        <Container size="md" py="xl" data-testid="server-settings-page">
            <Stack gap="lg">
                <Group justify="space-between" align="flex-start">
                    <div>
                        <Title order={2}>服务端连接</Title>
                        <Text c="dimmed" size="sm">
                            配置后端地址并检查连通性。当前地址：
                            <Code>{snapshot?.base ?? '解析中…'}</Code>
                        </Text>
                    </div>
                    <Button
                        variant="subtle"
                        color="gray"
                        leftSection={<IconArrowLeft size={16} />}
                        onClick={() => router.back()}
                    >
                        返回
                    </Button>
                </Group>

                {snapshot && isDeploymentFixed && (
                    <Alert color="blue" variant="light">
                        当前地址由部署环境固定（{SOURCE_TEXT[snapshot.source]}）。在此处保存只覆盖当前浏览器，
                        不会影响其它用户；如需全局修改，请调整构建参数或反向代理配置。
                    </Alert>
                )}

                <Card withBorder radius="md" padding="lg">
                    {snapshot ? (
                        <ServerConnectionForm mode="page" />
                    ) : (
                        <Center py="xl">
                            <Loader size="sm" />
                        </Center>
                    )}
                </Card>

                <Card withBorder radius="md" padding="lg">
                    <Stack gap="md">
                        <Group justify="space-between">
                            <Title order={4}>连接诊断</Title>
                            <Group gap="xs">
                                <Button
                                    size="xs"
                                    variant="light"
                                    leftSection={<IconRefresh size={14} />}
                                    loading={loading}
                                    onClick={() => void runDiagnostics()}
                                >
                                    重新检测
                                </Button>
                                <Button
                                    size="xs"
                                    variant="subtle"
                                    color="gray"
                                    leftSection={<IconCopy size={14} />}
                                    onClick={() => void handleCopy()}
                                >
                                    复制诊断信息
                                </Button>
                            </Group>
                        </Group>
                        <Table withTableBorder striped verticalSpacing="xs">
                            <Table.Tbody>
                                <Table.Tr>
                                    <Table.Td w={140}>解析后的基址</Table.Td>
                                    <Table.Td>
                                        <Code>{snapshot?.base ?? '解析中…'}</Code>
                                    </Table.Td>
                                </Table.Tr>
                                <Table.Tr>
                                    <Table.Td>地址来源</Table.Td>
                                    <Table.Td>
                                        {snapshot ? SOURCE_TEXT[snapshot.source] : '解析中…'}
                                    </Table.Td>
                                </Table.Tr>
                                <Table.Tr>
                                    <Table.Td>页面 Origin</Table.Td>
                                    <Table.Td>
                                        <Code>
                                            {typeof window === 'undefined'
                                                ? '-'
                                                : window.location.origin}
                                        </Code>
                                    </Table.Td>
                                </Table.Tr>
                                <Table.Tr>
                                    <Table.Td>/healthz</Table.Td>
                                    <Table.Td>
                                        {!diagnostics ? (
                                            <Text size="sm" c="dimmed">
                                                {loading ? '检测中…' : '未探测'}
                                            </Text>
                                        ) : diagnostics.probe.ok ? (
                                            <Group gap={6}>
                                                <Badge
                                                    color="teal"
                                                    leftSection={<IconCheck size={12} />}
                                                >
                                                    可达
                                                </Badge>
                                                <Badge variant="light">
                                                    {diagnostics.probe.latencyMs} ms
                                                </Badge>
                                                <Badge
                                                    variant="light"
                                                    color={
                                                        diagnostics.probe.body?.db === 'up'
                                                            ? 'teal'
                                                            : 'red'
                                                    }
                                                >
                                                    db: {diagnostics.probe.body?.db ?? '未知'}
                                                </Badge>
                                            </Group>
                                        ) : (
                                            <Badge
                                                color="red"
                                                leftSection={<IconAlertTriangle size={12} />}
                                            >
                                                {diagnostics.probe.errorKind === 'timeout'
                                                    ? '超时'
                                                    : diagnostics.probe.errorKind === 'network'
                                                      ? '网络不可达 / CORS'
                                                      : `HTTP ${diagnostics.probe.status}`}
                                            </Badge>
                                        )}
                                    </Table.Td>
                                </Table.Tr>
                                <Table.Tr>
                                    <Table.Td>服务版本</Table.Td>
                                    <Table.Td>
                                        {diagnostics?.openapiVersion ? (
                                            <Code>{diagnostics.openapiVersion}</Code>
                                        ) : (
                                            <Text size="sm" c="dimmed">
                                                文档未启用（/api-docs/openapi.json 不可用）
                                            </Text>
                                        )}
                                    </Table.Td>
                                </Table.Tr>
                            </Table.Tbody>
                        </Table>

                        {diagnostics && !diagnostics.probe.ok && (
                            <Alert color="red" variant="light">
                                服务异常排查顺序：① 端口是否正确（默认 3001）② 服务是否已启动
                                ③ 防火墙 / 安全组 ④ 反向代理是否透传 /healthz ⑤ CORS 是否放行当前
                                Origin。
                            </Alert>
                        )}

                    </Stack>
                </Card>
                <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="md">
                    <Card withBorder radius="md" padding="md">
                        <Stack gap={4}>
                            <Text fw={500} size="sm">
                                未登录也能配置
                            </Text>
                            <Text size="xs" c="dimmed">
                                本页不依赖登录态（/healthz 免鉴权）。桌面端首次启动、或浏览器打开静态站点时
                                都可以直接进入，填写地址后再去登录。
                            </Text>
                        </Stack>
                    </Card>
                    <Card withBorder radius="md" padding="md">
                        <Stack gap={4}>
                            <Text fw={500} size="sm">
                                保存会清空登录态
                            </Text>
                            <Text size="xs" c="dimmed">
                                换服务器后旧令牌无意义，因此保存地址会清掉本地令牌、用户与租户缓存，随后整页
                                重载。登录页底部也提供了本入口：<Link href="/login">去登录页</Link>。
                            </Text>
                        </Stack>
                    </Card>
                </SimpleGrid>

            </Stack>
        </Container>
    );
}
