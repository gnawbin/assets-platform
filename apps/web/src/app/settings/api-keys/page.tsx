'use client';

/**
 * API Key 管理页（设计文档 §5.2 / §6.6）
 *
 * - 列表：定位段、权限点、有效期、最近使用、状态
 * - 创建：名称 + scope 模板（或手填）+ 有效期 + 限流，成功后**只显示一次明文**
 * - 轮换 / 吊销
 *
 * 依赖 HTTP 通道（`/api/api-keys`），Tauri 桌面通道没有对应命令，因此非 HTTP
 * 模式下页面顶部给出提示并禁用操作（设计文档 §16 T-9：通道隔离）。
 */

import React, { useEffect, useState } from 'react';
import {
    ActionIcon,
    Alert,
    Badge,
    Button,
    Card,
    Code,
    CopyButton,
    Group,
    LoadingOverlay,
    Modal,
    NumberInput,
    Select,
    Stack,
    Table,
    Text,
    Textarea,
    TextInput,
    Title,
    Tooltip,
} from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import {
    IconAlertCircle,
    IconCheck,
    IconCopy,
    IconKey,
    IconPlus,
    IconRefresh,
    IconTrash,
} from '@tabler/icons-react';
import Layout from '@/components/Layout';
import { useApi } from '@/hooks/useApi';
import { notifyError, notifySuccess } from '@/utils/notify';
import { getAdapterType } from '@/utils/adapters';
import {
    API_KEY_SCOPE_TEMPLATES,
    createApiKey,
    getApiKeys,
    revokeApiKey,
    rotateApiKey,
    type ApiKeyView,
    type CreatedApiKey,
} from '@/services/apiKeyService';

/** 时间展示（空值返回短横线） */
function formatTime(value: string | null | undefined): string {
    if (!value) return '-';
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return value;
    return date.toLocaleString('zh-CN', { hour12: false });
}

/** 是否已过期（仅用于展示提醒） */
function isExpired(value: string | null): boolean {
    if (!value) return false;
    const date = new Date(value);
    return !Number.isNaN(date.getTime()) && date.getTime() < Date.now();
}

const TEMPLATE_OPTIONS = API_KEY_SCOPE_TEMPLATES.map((template) => ({
    value: template.key,
    label: `${template.label}（${template.description}）`,
}));

/** 单行表格项（拆成子组件，避免主组件过长） */
interface ApiKeyRowProps {
    apiKey: ApiKeyView;
    rotating: boolean;
    disabled: boolean;
    onRotate: (key: ApiKeyView) => void;
    onRevoke: (key: ApiKeyView) => void;
}

const ApiKeyRow: React.FC<ApiKeyRowProps> = ({
    apiKey,
    rotating,
    disabled,
    onRotate,
    onRevoke,
}) => {
    const revoked = !!apiKey.revoked_at || !apiKey.enable;
    const expired = !revoked && isExpired(apiKey.expires_at);

    return (
        <Table.Tr>
            <Table.Td>
                <Group gap={6} wrap="nowrap">
                    <IconKey size={16} />
                    <div>
                        <Text fw={500}>{apiKey.name}</Text>
                        {apiKey.description && (
                            <Text size="xs" c="dimmed">
                                {apiKey.description}
                            </Text>
                        )}
                    </div>
                </Group>
            </Table.Td>
            <Table.Td>
                <Code>{`ak_***_${apiKey.key_prefix}_***`}</Code>
            </Table.Td>
            <Table.Td>
                <Group gap={4}>
                    {apiKey.scopes.slice(0, 2).map((scope) => (
                        <Badge key={scope} variant="light" size="sm">
                            {scope}
                        </Badge>
                    ))}
                    {apiKey.scopes.length > 2 && (
                        <Tooltip label={apiKey.scopes.join('、')}>
                            <Badge variant="outline" size="sm">
                                +{apiKey.scopes.length - 2}
                            </Badge>
                        </Tooltip>
                    )}
                </Group>
            </Table.Td>
            <Table.Td>
                <Text size="sm">{formatTime(apiKey.expires_at)}</Text>
                <Text size="xs" c="dimmed">
                    限流 {apiKey.rate_limit_per_min}/分钟
                </Text>
            </Table.Td>
            <Table.Td>
                <Text size="sm">{formatTime(apiKey.last_used_at)}</Text>
            </Table.Td>
            <Table.Td>
                {revoked ? (
                    <Badge color="red">已吊销</Badge>
                ) : expired ? (
                    <Badge color="orange">已过期</Badge>
                ) : (
                    <Badge color="green">启用</Badge>
                )}
            </Table.Td>
            <Table.Td>
                <Group gap="xs">
                    <Tooltip label="轮换（旧 key 24 小时后过期）">
                        <ActionIcon
                            variant="subtle"
                            color="blue"
                            onClick={() => onRotate(apiKey)}
                            loading={rotating}
                            disabled={revoked || disabled}
                        >
                            <IconRefresh size={16} />
                        </ActionIcon>
                    </Tooltip>
                    <Tooltip label="吊销（立即生效）">
                        <ActionIcon
                            variant="subtle"
                            color="red"
                            onClick={() => onRevoke(apiKey)}
                            disabled={revoked || disabled}
                        >
                            <IconTrash size={16} />
                        </ActionIcon>
                    </Tooltip>
                </Group>
            </Table.Td>
        </Table.Tr>
    );
};

const ApiKeysSettingsPage: React.FC = () => {
    // Tauri 通道没有 API Key 命令，只有 HTTP 通道可用
    const isHttpMode = typeof window !== 'undefined' && getAdapterType() === 'http';

    const { data: keys, loading, error, execute: fetchKeys } = useApi(getApiKeys);
    const { execute: doCreate, loading: creating } = useApi(createApiKey);
    const { execute: doRotate, loading: rotating } = useApi(rotateApiKey);
    const { execute: doRevoke, loading: revoking } = useApi(revokeApiKey);

    const [createOpened, { open: openCreate, close: closeCreate }] = useDisclosure(false);
    const [secretOpened, { open: openSecret, close: closeSecret }] = useDisclosure(false);
    const [revokeTarget, setRevokeTarget] = useState<ApiKeyView | null>(null);
    const [createdSecret, setCreatedSecret] = useState<CreatedApiKey | null>(null);

    const [form, setForm] = useState({
        name: '',
        description: '',
        template: API_KEY_SCOPE_TEMPLATES[0].key,
        scopes: API_KEY_SCOPE_TEMPLATES[0].scopes.join('\n'),
        expires_in_days: 90,
        rate_limit_per_min: 600,
    });

    useEffect(() => {
        if (isHttpMode) {
            fetchKeys();
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [isHttpMode]);

    const handleTemplateChange = (value: string | null) => {
        const template = API_KEY_SCOPE_TEMPLATES.find((item) => item.key === value);
        setForm((prev) => ({
            ...prev,
            template: value || prev.template,
            scopes: template ? template.scopes.join('\n') : prev.scopes,
        }));
    };

    const handleCreate = async () => {
        const name = form.name.trim();
        const scopes = form.scopes
            .split('\n')
            .map((scope) => scope.trim())
            .filter(Boolean);

        if (!name) {
            notifyError('请填写 API Key 名称');
            return;
        }
        if (scopes.length === 0) {
            notifyError('至少需要一个权限点');
            return;
        }

        const created = await doCreate({
            name,
            description: form.description.trim() || null,
            scopes,
            expires_in_days: form.expires_in_days,
            rate_limit_per_min: form.rate_limit_per_min,
        });

        if (!created) {
            notifyError('创建失败：请检查权限点是否超出当前账号权限');
            return;
        }

        setCreatedSecret(created);
        closeCreate();
        openSecret();
        notifySuccess('API Key 已创建，请立即保存明文');
        await fetchKeys();
    };

    const handleRotate = async (key: ApiKeyView) => {
        const created = await doRotate(key.id, form.expires_in_days);
        if (!created) {
            notifyError('轮换失败');
            return;
        }
        setCreatedSecret(created);
        openSecret();
        notifySuccess('已轮换，旧 key 将在 24 小时后过期');
        await fetchKeys();
    };

    const handleRevoke = async () => {
        if (!revokeTarget) return;
        const result = await doRevoke(revokeTarget.id);
        if (result === null && !revoking) {
            // 吊销成功时 data 为 null（无响应体），失败时 error 由 useApi 记录
            notifySuccess('已吊销');
        }
        setRevokeTarget(null);
        await fetchKeys();
    };

    return (
        <Layout>
            <Stack gap="lg">
                <Group justify="space-between">
                    <div>
                        <Title order={2}>API Key 管理</Title>
                        <Text c="dimmed">
                            为外部系统（ERP / OA / 运维平台）签发凭证；明文只在创建或轮换时显示一次，
                            数据库只保存 SHA-256 哈希
                        </Text>
                    </div>
                    <Group gap="xs">
                        <Button
                            variant="light"
                            leftSection={<IconRefresh size={16} />}
                            onClick={() => fetchKeys()}
                            loading={loading}
                            disabled={!isHttpMode}
                        >
                            刷新
                        </Button>
                        <Button
                            leftSection={<IconPlus size={16} />}
                            onClick={openCreate}
                            disabled={!isHttpMode}
                        >
                            创建 Key
                        </Button>
                    </Group>
                </Group>

                {!isHttpMode && (
                    <Alert icon={<IconAlertCircle size={16} />} title="当前为桌面模式" color="yellow">
                        API Key 管理接口只存在于 HTTP 服务（`/api/api-keys`）。请通过 Web 形态访问，
                        或在桌面端配置好 `JWT_SECRET` 后使用 HTTP 端口。
                    </Alert>
                )}

                {error && (
                    <Alert
                        icon={<IconAlertCircle size={16} />}
                        title="加载失败"
                        color="red"
                        withCloseButton
                        onClose={() => fetchKeys()}
                    >
                        {error}
                    </Alert>
                )}

                <Card withBorder padding="lg" radius="md" pos="relative">
                    <LoadingOverlay visible={loading && !keys} />

                    <Table striped highlightOnHover>
                        <Table.Thead>
                            <Table.Tr>
                                <Table.Th>名称</Table.Th>
                                <Table.Th>定位段</Table.Th>
                                <Table.Th>权限点</Table.Th>
                                <Table.Th>有效期</Table.Th>
                                <Table.Th>最近使用</Table.Th>
                                <Table.Th>状态</Table.Th>
                                <Table.Th>操作</Table.Th>
                            </Table.Tr>
                        </Table.Thead>
                        <Table.Tbody>
                            {!keys || keys.length === 0 ? (
                                <Table.Tr>
                                    <Table.Td colSpan={7}>
                                        <Text ta="center" c="dimmed" py="xl">
                                            暂无 API Key
                                        </Text>
                                    </Table.Td>
                                </Table.Tr>
                            ) : (
                                keys.map((key) => (
                                    <ApiKeyRow
                                        key={key.id}
                                        apiKey={key}
                                        rotating={rotating}
                                        disabled={!isHttpMode}
                                        onRotate={handleRotate}
                                        onRevoke={setRevokeTarget}
                                    />
                                ))
                            )}
                        </Table.Tbody>
                    </Table>
                </Card>
            </Stack>


            <Modal opened={createOpened} onClose={closeCreate} title="创建 API Key" size="lg">
                <Stack gap="sm">
                    <TextInput
                        label="名称"
                        placeholder="例如：ERP 集成"
                        required
                        value={form.name}
                        onChange={(event) =>
                            setForm((prev) => ({ ...prev, name: event.currentTarget.value }))
                        }
                    />
                    <TextInput
                        label="用途说明"
                        placeholder="例如：财务系统只读对接"
                        value={form.description}
                        onChange={(event) =>
                            setForm((prev) => ({
                                ...prev,
                                description: event.currentTarget.value,
                            }))
                        }
                    />
                    <Select
                        label="权限模板"
                        data={TEMPLATE_OPTIONS}
                        value={form.template}
                        onChange={handleTemplateChange}
                        allowDeselect={false}
                    />
                    <Textarea
                        label="权限点（每行一个）"
                        description="必须属于当前账号已有权限，且不能包含 system:* 管理点"
                        autosize
                        minRows={4}
                        maxRows={8}
                        value={form.scopes}
                        onChange={(event) =>
                            setForm((prev) => ({
                                ...prev,
                                scopes: event.currentTarget.value,
                            }))
                        }
                    />
                    <Group grow>
                        <NumberInput
                            label="有效期（天）"
                            min={1}
                            max={365}
                            value={form.expires_in_days}
                            onChange={(value) =>
                                setForm((prev) => ({
                                    ...prev,
                                    expires_in_days: Number(value) || 90,
                                }))
                            }
                        />
                        <NumberInput
                            label="限流（次/分钟）"
                            min={1}
                            max={100000}
                            value={form.rate_limit_per_min}
                            onChange={(value) =>
                                setForm((prev) => ({
                                    ...prev,
                                    rate_limit_per_min: Number(value) || 600,
                                }))
                            }
                        />
                    </Group>
                    <Group justify="flex-end" mt="sm">
                        <Button variant="default" onClick={closeCreate}>
                            取消
                        </Button>
                        <Button onClick={handleCreate} loading={creating}>
                            创建
                        </Button>
                    </Group>
                </Stack>
            </Modal>

            <Modal
                opened={secretOpened}
                onClose={closeSecret}
                title="请立即保存密钥（只显示这一次）"
                size="lg"
            >
                <Stack gap="sm">
                    <Alert icon={<IconAlertCircle size={16} />} color="orange">
                        关闭本窗口后将无法再次查看完整密钥。若已丢失，请直接轮换或吊销后重新创建。
                    </Alert>
                    <Code block>{createdSecret?.api_key ?? ''}</Code>
                    <Group justify="space-between">
                        <Text size="sm" c="dimmed">
                            定位段：{createdSecret?.key_prefix ?? '-'}　有效期至：
                            {formatTime(createdSecret?.expires_at)}
                        </Text>
                        <CopyButton value={createdSecret?.api_key ?? ''} timeout={2000}>
                            {({ copied, copy }) => (
                                <Button
                                    variant="light"
                                    color={copied ? 'teal' : 'blue'}
                                    leftSection={
                                        copied ? <IconCheck size={16} /> : <IconCopy size={16} />
                                    }
                                    onClick={copy}
                                >
                                    {copied ? '已复制' : '复制密钥'}
                                </Button>
                            )}
                        </CopyButton>
                    </Group>
                </Stack>
            </Modal>

            <Modal
                opened={!!revokeTarget}
                onClose={() => setRevokeTarget(null)}
                title="吊销 API Key"
            >
                <Stack gap="sm">
                    <Text>
                        确定吊销 <b>{revokeTarget?.name}</b>（{revokeTarget?.key_prefix}）吗？
                        吊销后立即生效且不可恢复，行记录会保留用于审计。
                    </Text>
                    <Group justify="flex-end">
                        <Button variant="default" onClick={() => setRevokeTarget(null)}>
                            取消
                        </Button>
                        <Button color="red" onClick={handleRevoke} loading={revoking}>
                            确认吊销
                        </Button>
                    </Group>
                </Stack>
            </Modal>
        </Layout>
    );
};

export default ApiKeysSettingsPage;

