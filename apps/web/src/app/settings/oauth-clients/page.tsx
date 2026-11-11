'use client';

/**
 * OAuth 2.1 客户端管理页（设计文档 §7 / Phase 2）
 *
 * - 列表：客户端 ID、类型、回调地址、授权范围、令牌有效期、状态
 * - 创建：名称 + 类型（confidential / public）+ 回调地址 + scope 模板，成功后**只显示一次密钥**
 * - 编辑 / 启用禁用 / 删除（禁用与删除会同时撤销该客户端已签发的全部令牌）
 *
 * 依赖 HTTP 通道（`/api/oauth-clients`），Tauri 桌面通道没有对应命令，因此非 HTTP
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
  Switch,
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
  IconApps,
  IconCheck,
  IconCopy,
  IconEdit,
  IconPlus,
  IconRefresh,
  IconTrash,
} from '@tabler/icons-react';
import Layout from '@/components/Layout';
import { useApi } from '@/hooks/useApi';
import { notifyError, notifySuccess } from '@/utils/notify';
import { getAdapterType } from '@/utils/adapters';
import {
  createOauthClient,
  deleteOauthClient,
  getOauthClients,
  updateOauthClient,
  OAUTH_CLIENT_TYPE_CONFIDENTIAL,
  OAUTH_CLIENT_TYPE_PUBLIC,
  OAUTH_SCOPE_TEMPLATES,
  type CreatedOauthClient,
  type OauthClientView,
} from '@/services/oauthClientService';

/** 秒 → 人类可读（仅用于展示） */
function formatTtl(seconds: number): string {
  if (seconds >= 86400) return `${Math.round(seconds / 86400)} 天`;
  if (seconds >= 3600) return `${Math.round(seconds / 3600)} 小时`;
  return `${Math.round(seconds / 60)} 分钟`;
}

/** 类型展示 */
function clientTypeLabel(clientType: number): string {
  return clientType === OAUTH_CLIENT_TYPE_CONFIDENTIAL ? 'confidential' : 'public';
}

const TEMPLATE_OPTIONS = OAUTH_SCOPE_TEMPLATES.map((template) => ({
  value: template.key,
  label: `${template.label}（${template.description}）`,
}));

const CLIENT_TYPE_OPTIONS = [
  {
    value: String(OAUTH_CLIENT_TYPE_PUBLIC),
    label: 'public（桌面 / SPA，无密钥，强制 PKCE）',
  },
  {
    value: String(OAUTH_CLIENT_TYPE_CONFIDENTIAL),
    label: 'confidential（服务端应用，client_secret_basic）',
  },
];

/** 单行表格项 */
interface OauthClientRowProps {
    client: OauthClientView;
    disabled: boolean;
    onEdit: (client: OauthClientView) => void;
    onDelete: (client: OauthClientView) => void;
    onToggle: (client: OauthClientView, enable: boolean) => void;
}

const OauthClientRow: React.FC<OauthClientRowProps> = ({
    client,
    disabled,
    onEdit,
    onDelete,
    onToggle,
}) => (
    <Table.Tr>
        <Table.Td>
            <Group gap={6} wrap="nowrap">
                <IconApps size={16} />
                <div>
                    <Text fw={500}>{client.client_name}</Text>
                    <Text size="xs" c="dimmed">
                        {clientTypeLabel(client.client_type)}
                    </Text>
                </div>
            </Group>
        </Table.Td>
        <Table.Td>
            <Code>{client.client_id}</Code>
        </Table.Td>
        <Table.Td>
            <Stack gap={2}>
                {client.redirect_uris.slice(0, 2).map((uri) => (
                    <Text key={uri} size="xs" c="dimmed" style={{ wordBreak: 'break-all' }}>
                        {uri}
                    </Text>
                ))}
                {client.redirect_uris.length > 2 && (
                    <Text size="xs" c="dimmed">
                        等 {client.redirect_uris.length} 条
                    </Text>
                )}
            </Stack>
        </Table.Td>
        <Table.Td>
            <Group gap={4}>
                {client.allowed_scopes.slice(0, 2).map((scope) => (
                    <Badge key={scope} variant="light" size="sm">
                        {scope}
                    </Badge>
                ))}
                {client.allowed_scopes.length > 2 && (
                    <Tooltip label={client.allowed_scopes.join(' ')}>
                        <Badge variant="outline" size="sm">
                            +{client.allowed_scopes.length - 2}
                        </Badge>
                    </Tooltip>
                )}
            </Group>
        </Table.Td>
        <Table.Td>
            <Text size="xs">access {formatTtl(client.access_token_ttl)}</Text>
            <Text size="xs" c="dimmed">
                refresh {formatTtl(client.refresh_token_ttl)}
            </Text>
        </Table.Td>
        <Table.Td>
            <Badge color={client.enable ? 'teal' : 'gray'} variant="light">
                {client.enable ? '启用' : '已禁用'}
            </Badge>
        </Table.Td>
        <Table.Td>
            <Group gap={4} wrap="nowrap">
                <Tooltip label={client.enable ? '禁用并撤销全部令牌' : '启用'}>
                    <Switch
                        size="sm"
                        checked={client.enable}
                        disabled={disabled}
                        onChange={(event) => onToggle(client, event.currentTarget.checked)}
                    />
                </Tooltip>
                <Tooltip label="编辑">
                    <ActionIcon variant="subtle" onClick={() => onEdit(client)} disabled={disabled}>
                        <IconEdit size={16} />
                    </ActionIcon>
                </Tooltip>
                <Tooltip label="删除（同时撤销全部令牌）">
                    <ActionIcon
                        variant="subtle"
                        color="red"
                        onClick={() => onDelete(client)}
                        disabled={disabled}
                    >
                        <IconTrash size={16} />
                    </ActionIcon>
                </Tooltip>
            </Group>
        </Table.Td>
    </Table.Tr>
);

/**
 * 把多行文本拆成去空的字符串数组（回调地址 / 权限点共用）
 */
function splitLines(value: string): string[] {
    return value
        .split('\n')
        .map((line) => line.trim())
        .filter(Boolean);
}

const OauthClientsSettingsPage: React.FC = () => {
    // Tauri 通道没有 OAuth 客户端命令，只有 HTTP 通道可用
    const isHttpMode = typeof window !== 'undefined' && getAdapterType() === 'http';

    const {
        data: clients,
        loading,
        error,
        execute: fetchClients,
        reset: resetError,
    } = useApi(getOauthClients);
    const { execute: doCreate, loading: creating } = useApi(createOauthClient);
    const { execute: doUpdate, loading: updating } = useApi(updateOauthClient);
    // 删除接口没有响应体，无法用 useApi 的返回值区分成功/失败，改为直接调用 + 本地状态
    const [deleting, setDeleting] = useState(false);

    const [createOpened, { open: openCreate, close: closeCreate }] = useDisclosure(false);
    const [secretOpened, { open: openSecret, close: closeSecret }] = useDisclosure(false);
    const [editTarget, setEditTarget] = useState<OauthClientView | null>(null);
    const [deleteTarget, setDeleteTarget] = useState<OauthClientView | null>(null);
    const [createdSecret, setCreatedSecret] = useState<CreatedOauthClient | null>(null);

    const [form, setForm] = useState({
        client_name: '',
        client_type: String(OAUTH_CLIENT_TYPE_PUBLIC),
        redirect_uris: 'http://127.0.0.1/callback',
        template: OAUTH_SCOPE_TEMPLATES[0].key,
        scopes: OAUTH_SCOPE_TEMPLATES[0].scopes.join('\n'),
        access_token_ttl: 900,
        refresh_token_ttl: 2592000,
    });

    const [editForm, setEditForm] = useState({
        client_name: '',
        redirect_uris: '',
        scopes: '',
        enable: true,
    });

    useEffect(() => {
        if (isHttpMode) {
            fetchClients();
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [isHttpMode]);

    const handleTemplateChange = (value: string | null) => {
        const template = OAUTH_SCOPE_TEMPLATES.find((item) => item.key === value);
        setForm((prev) => ({
            ...prev,
            template: value || prev.template,
            scopes: template ? template.scopes.join('\n') : prev.scopes,
        }));
    };

    const handleCreate = async () => {
        const clientName = form.client_name.trim();
        const redirectUris = splitLines(form.redirect_uris);
        const scopes = splitLines(form.scopes);

        if (!clientName) {
            notifyError('请填写客户端名称');
            return;
        }
        if (redirectUris.length === 0) {
            notifyError('至少需要一个回调地址');
            return;
        }
        if (scopes.length === 0) {
            notifyError('至少需要一个授权范围（scope）');
            return;
        }

        const created = await doCreate({
            client_name: clientName,
            client_type: Number(form.client_type),
            redirect_uris: redirectUris,
            allowed_scopes: scopes,
            access_token_ttl: form.access_token_ttl,
            refresh_token_ttl: form.refresh_token_ttl,
        });

        if (!created) {
            notifyError('创建失败：请检查回调地址与 scope 是否合规');
            return;
        }

        setCreatedSecret(created);
        closeCreate();
        openSecret();
        notifySuccess('OAuth 客户端已创建，请立即保存密钥');
        await fetchClients();
    };


    const handleOpenEdit = (client: OauthClientView) => {
        setEditForm({
            client_name: client.client_name,
            redirect_uris: client.redirect_uris.join('\n'),
            scopes: client.allowed_scopes.join('\n'),
            enable: client.enable,
        });
        setEditTarget(client);
    };

    const handleUpdate = async () => {
        if (!editTarget) return;
        const clientName = editForm.client_name.trim();
        const redirectUris = splitLines(editForm.redirect_uris);
        const scopes = splitLines(editForm.scopes);

        if (!clientName) {
            notifyError('请填写客户端名称');
            return;
        }
        if (redirectUris.length === 0) {
            notifyError('至少需要一个回调地址');
            return;
        }
        if (scopes.length === 0) {
            notifyError('至少需要一个授权范围（scope）');
            return;
        }

        const updated = await doUpdate(editTarget.id, {
            client_name: clientName,
            redirect_uris: redirectUris,
            allowed_scopes: scopes,
            enable: editForm.enable,
        });

        if (!updated) {
            notifyError('保存失败');
            return;
        }

        notifySuccess('已保存');
        setEditTarget(null);
        await fetchClients();
    };

    const handleToggle = async (client: OauthClientView, enable: boolean) => {
        const updated = await doUpdate(client.id, { enable });
        if (!updated) {
            notifyError('状态更新失败');
            return;
        }
        notifySuccess(enable ? '已启用' : '已禁用，该客户端已签发的令牌全部失效');
        await fetchClients();
    };

    const handleDelete = async () => {
        if (!deleteTarget) return;
        setDeleting(true);
        try {
            await deleteOauthClient(deleteTarget.id);
            notifySuccess('已删除，该客户端的令牌全部失效');
            setDeleteTarget(null);
            await fetchClients();
        } catch {
            // 失败提示由 HTTP 适配器统一弹出（showError 默认 true），这里不重复提示
        } finally {
            setDeleting(false);
        }
    };


    return (
        <Layout>
            <Stack gap="lg">
                <Group justify="space-between">
                    <div>
                        <Title order={2}>OAuth 客户端</Title>
                        <Text c="dimmed">
                            供第三方 / 内部系统以 OAuth 2.1 授权码 + PKCE 方式代表用户调用 API；
                            密钥只在创建时显示一次，数据库只保存 SHA-256 哈希
                        </Text>
                    </div>
                    <Group gap="xs">
                        <Button
                            variant="light"
                            leftSection={<IconRefresh size={16} />}
                            onClick={() => fetchClients()}
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
                            新建客户端
                        </Button>
                    </Group>
                </Group>

                {!isHttpMode && (
                    <Alert icon={<IconAlertCircle size={16} />} title="当前为桌面模式" color="yellow">
                        OAuth 客户端管理接口只存在于 HTTP 服务（`/api/oauth-clients`）。请通过 Web
                        形态访问，或在桌面端配置好 `JWT_SECRET` 后使用 HTTP 端口。
                    </Alert>
                )}

                <Alert icon={<IconAlertCircle size={16} />} color="blue" title="接入提示">
                    授权端点为 `GET /oauth/authorize`（携带登录 JWT 取同意信息）与
                    `POST /oauth/authorize`（用户确认后返回带 code 的回调地址）；令牌端点为
                    `POST /oauth/token`，元数据见 `/.well-known/oauth-authorization-server`。
                    端点需设置环境变量 `OAUTH_ENABLED=true` 后才会挂载。
                </Alert>

                {error && (
                    <Alert
                        icon={<IconAlertCircle size={16} />}
                        title="加载失败"
                        color="red"
                        withCloseButton
                        onClose={resetError}
                    >
                        {error}
                    </Alert>
                )}

                <Card withBorder padding="lg" radius="md" pos="relative">
                    <LoadingOverlay visible={loading && !clients} />

                    <Table striped highlightOnHover>
                        <Table.Thead>
                            <Table.Tr>
                                <Table.Th>名称</Table.Th>
                                <Table.Th>client_id</Table.Th>
                                <Table.Th>回调地址</Table.Th>
                                <Table.Th>授权范围</Table.Th>
                                <Table.Th>令牌有效期</Table.Th>
                                <Table.Th>状态</Table.Th>
                                <Table.Th>操作</Table.Th>
                            </Table.Tr>
                        </Table.Thead>
                        <Table.Tbody>
                            {!clients || clients.length === 0 ? (
                                <Table.Tr>
                                    <Table.Td colSpan={7}>
                                        <Text ta="center" c="dimmed" py="xl">
                                            暂无 OAuth 客户端
                                        </Text>
                                    </Table.Td>
                                </Table.Tr>
                            ) : (
                                clients.map((client) => (
                                    <OauthClientRow
                                        key={client.id}
                                        client={client}
                                        disabled={!isHttpMode}
                                        onEdit={handleOpenEdit}
                                        onDelete={setDeleteTarget}
                                        onToggle={handleToggle}
                                    />
                                ))
                            )}
                        </Table.Tbody>
                    </Table>
                </Card>
            </Stack>


            {/* 创建客户端 */}
            <Modal opened={createOpened} onClose={closeCreate} title="新建 OAuth 客户端" size="lg">
                <Stack gap="sm">
                    <TextInput
                        label="客户端名称"
                        placeholder="例如：ERP 系统"
                        required
                        value={form.client_name}
                        onChange={(event) =>
                            setForm((prev) => ({ ...prev, client_name: event.currentTarget.value }))
                        }
                    />
                    <Select
                        label="客户端类型"
                        data={CLIENT_TYPE_OPTIONS}
                        value={form.client_type}
                        onChange={(value) =>
                            setForm((prev) => ({
                                ...prev,
                                client_type: value || prev.client_type,
                            }))
                        }
                        allowDeselect={false}
                    />
                    <Textarea
                        label="回调地址（每行一个）"
                        description="https，或 http + 回环地址（127.0.0.1 / ::1，按 RFC 8252 忽略端口）"
                        autosize
                        minRows={2}
                        maxRows={5}
                        value={form.redirect_uris}
                        onChange={(event) =>
                            setForm((prev) => ({ ...prev, redirect_uris: event.currentTarget.value }))
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
                        label="授权范围（每行一个 scope）"
                        description="必须属于当前账号已有权限，且不能包含 system:* / upload:*"
                        autosize
                        minRows={4}
                        maxRows={8}
                        value={form.scopes}
                        onChange={(event) =>
                            setForm((prev) => ({ ...prev, scopes: event.currentTarget.value }))
                        }
                    />
                    <Group grow>
                        <NumberInput
                            label="访问令牌有效期（秒）"
                            min={60}
                            max={86400}
                            value={form.access_token_ttl}
                            onChange={(value) =>
                                setForm((prev) => ({
                                    ...prev,
                                    access_token_ttl: Number(value) || 900,
                                }))
                            }
                        />
                        <NumberInput
                            label="刷新令牌有效期（秒）"
                            min={300}
                            max={7776000}
                            value={form.refresh_token_ttl}
                            onChange={(value) =>
                                setForm((prev) => ({
                                    ...prev,
                                    refresh_token_ttl: Number(value) || 2592000,
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

            {/* 一次性密钥 */}
            <Modal
                opened={secretOpened}
                onClose={closeSecret}
                title="请立即保存密钥（只显示这一次）"
                size="lg"
            >
                <Stack gap="sm">
                    <Alert icon={<IconAlertCircle size={16} />} color="orange">
                        关闭本窗口后将无法再次查看完整密钥。若已丢失，请删除客户端后重新创建。
                        public 客户端没有密钥，请使用 PKCE（S256）换取令牌。
                    </Alert>
                    <Text size="sm" c="dimmed">
                        client_id
                    </Text>
                    <Code block>{createdSecret?.client_id ?? ''}</Code>
                    <Text size="sm" c="dimmed">
                        client_secret
                    </Text>
                    <Code block>{createdSecret?.client_secret ?? '（public 客户端无密钥）'}</Code>
                    <Group justify="flex-end">
                        <CopyButton
                            value={[
                                `client_id=${createdSecret?.client_id ?? ''}`,
                                `client_secret=${createdSecret?.client_secret ?? ''}`,
                            ].join('\n')}
                            timeout={2000}
                        >
                            {({ copied, copy }) => (
                                <Button
                                    variant="light"
                                    color={copied ? 'teal' : 'blue'}
                                    leftSection={
                                        copied ? <IconCheck size={16} /> : <IconCopy size={16} />
                                    }
                                    onClick={copy}
                                >
                                    {copied ? '已复制' : '复制凭据'}
                                </Button>
                            )}
                        </CopyButton>
                    </Group>
                </Stack>
            </Modal>


            {/* 编辑客户端 */}
            <Modal
                opened={!!editTarget}
                onClose={() => setEditTarget(null)}
                title={`编辑客户端：${editTarget?.client_name ?? ''}`}
                size="lg"
            >
                <Stack gap="sm">
                    <TextInput
                        label="客户端名称"
                        required
                        value={editForm.client_name}
                        onChange={(event) =>
                            setEditForm((prev) => ({
                                ...prev,
                                client_name: event.currentTarget.value,
                            }))
                        }
                    />
                    <Textarea
                        label="回调地址（每行一个）"
                        description="修改后旧的回调地址立即失效"
                        autosize
                        minRows={2}
                        maxRows={5}
                        value={editForm.redirect_uris}
                        onChange={(event) =>
                            setEditForm((prev) => ({
                                ...prev,
                                redirect_uris: event.currentTarget.value,
                            }))
                        }
                    />
                    <Textarea
                        label="授权范围（每行一个 scope）"
                        description="收窄后，已签发令牌的 scope 不受影响（下次授权生效）"
                        autosize
                        minRows={4}
                        maxRows={8}
                        value={editForm.scopes}
                        onChange={(event) =>
                            setEditForm((prev) => ({ ...prev, scopes: event.currentTarget.value }))
                        }
                    />
                    <Switch
                        label="启用"
                        description="关闭后该客户端无法再授权或换取令牌，且已签发令牌全部失效"
                        checked={editForm.enable}
                        onChange={(event) =>
                            setEditForm((prev) => ({
                                ...prev,
                                enable: event.currentTarget.checked,
                            }))
                        }
                    />
                    <Group justify="flex-end" mt="sm">
                        <Button variant="default" onClick={() => setEditTarget(null)}>
                            取消
                        </Button>
                        <Button onClick={handleUpdate} loading={updating}>
                            保存
                        </Button>
                    </Group>
                </Stack>
            </Modal>

            {/* 删除确认 */}
            <Modal
                opened={!!deleteTarget}
                onClose={() => setDeleteTarget(null)}
                title="删除 OAuth 客户端"
            >
                <Stack gap="sm">
                    <Text>
                        确定删除 <b>{deleteTarget?.client_name}</b>（{deleteTarget?.client_id}）吗？
                        删除后该客户端已签发的 access / refresh 令牌会立即失效，行记录保留用于审计。
                    </Text>
                    <Group justify="flex-end">
                        <Button variant="default" onClick={() => setDeleteTarget(null)}>
                            取消
                        </Button>
                        <Button color="red" onClick={handleDelete} loading={deleting}>
                            确认删除
                        </Button>
                    </Group>
                </Stack>
            </Modal>
        </Layout>
    );
};

export default OauthClientsSettingsPage;

