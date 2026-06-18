'use client';
import React, { useEffect, useState } from 'react';
import Layout from '@/components/Layout';
import {
    Title,
    Text,
    Card,
    Stack,
    Group,
    Button,
    Modal,
    TextInput,
    Switch,
    Loader,
    Alert,
    SimpleGrid,
    Box,
    Paper,
    ScrollArea,
    ActionIcon,
    Tooltip,
    Divider,
    Badge,
} from '@mantine/core';
import {
    IconAlertCircle,
    IconTrash,
    IconEdit,
    IconPlus,
    IconBuildingStore,
    IconBuildingCommunity,
    IconRefresh,
    IconChevronRight,
    IconChevronDown,
} from '@tabler/icons-react';
import { notifySuccess, notifyError } from '@/utils/notify';
import { useApi } from '@/hooks/useApi';
import {
    getTenants,
    insertTenant,
    updateTenant,
    deleteTenant,
    type Tenant,
} from '@/services/tenantService';

// 树节点接口
interface TreeNode {
    id: number;
    tenant_name: string;
    parent_id: number | null;
    is_leaf: boolean;
    schema_name: string | null;
    enable: boolean;
    create_at: string | null;
    updated_at: string | null;
    children: TreeNode[];
    expanded: boolean;
}

const TenantsPage: React.FC = () => {
    const [tenants, setTenants] = useState<Tenant[]>([]);
    const [treeData, setTreeData] = useState<TreeNode[]>([]);

    // 使用 useApi 管理数据获取
    const {
        data: fetchedTenants,
        loading,
        error,
        execute: fetchTenants,
    } = useApi(getTenants);

    // 选中的租户
    const [selectedTenant, setSelectedTenant] = useState<Tenant | null>(null);

    // 新增/编辑弹窗
    const [formModalOpen, setFormModalOpen] = useState(false);
    const [formMode, setFormMode] = useState<'add' | 'edit'>('add');
    const [formParentId, setFormParentId] = useState<number | null>(null);
    const [formName, setFormName] = useState('');
    const [formSchemaName, setFormSchemaName] = useState('');
    const [formIsLeaf, setFormIsLeaf] = useState(true);
    const [formEnable, setFormEnable] = useState(true);

    // 使用 useApi 管理增删改操作
    const { execute: doInsertTenant, loading: adding } = useApi(insertTenant);
    const { execute: doUpdateTenant, loading: editing } = useApi(updateTenant);
    const { execute: doDeleteTenant, loading: deleting } = useApi(deleteTenant);

    // 删除确认弹窗
    const [deleteModalOpen, setDeleteModalOpen] = useState(false);

    // 当 fetchedTenants 变化时更新本地状态
    useEffect(() => {
        if (fetchedTenants) {
            setTenants(fetchedTenants);
            buildTree(fetchedTenants);
        }
    }, [fetchedTenants]);

    useEffect(() => {
        fetchTenants();
    }, []);

    // 构建树结构
    const buildTree = (items: Tenant[]) => {
        const map = new Map<number, TreeNode>();
        const roots: TreeNode[] = [];

        // 先创建所有节点
        items.forEach((item) => {
            map.set(item.id, {
                id: item.id,
                tenant_name: item.tenant_name,
                parent_id: item.parent_id,
                is_leaf: item.is_leaf,
                schema_name: item.schema_name,
                enable: item.enable,
                create_at: item.create_at,
                updated_at: item.updated_at,
                children: [],
                expanded: true,
            });
        });

        // 构建父子关系
        items.forEach((item) => {
            const node = map.get(item.id)!;
            if (item.parent_id && map.has(item.parent_id)) {
                map.get(item.parent_id)!.children.push(node);
            } else {
                roots.push(node);
            }
        });

        setTreeData(roots);
    };

    // 获取父租户名称
    const getParentName = (parentId: number | null): string => {
        if (!parentId) return '（顶级租户）';
        const parent = tenants.find((t) => t.id === parentId);
        return parent ? parent.tenant_name : '（未知）';
    };

    // 打开新增弹窗
    const openAddModal = (parentId: number | null = null) => {
        setFormMode('add');
        setFormParentId(parentId);
        setFormName('');
        setFormSchemaName('');
        setFormIsLeaf(true);
        setFormEnable(true);
        setFormModalOpen(true);
    };

    // 打开编辑弹窗
    const openEditModal = () => {
        if (!selectedTenant) return;
        setFormMode('edit');
        setFormParentId(selectedTenant.parent_id);
        setFormName(selectedTenant.tenant_name);
        setFormSchemaName(selectedTenant.schema_name || '');
        setFormIsLeaf(selectedTenant.is_leaf);
        setFormEnable(selectedTenant.enable);
        setFormModalOpen(true);
    };

    // 保存租户
    const handleSave = async () => {
        if (!formName.trim()) {
            notifyError('验证失败', '请输入租户名称');
            return;
        }

        // 末级节点需要 schema 名称
        if (formIsLeaf && !formSchemaName.trim()) {
            notifyError('验证失败', '末级租户必须指定 Schema 名称');
            return;
        }

        // 验证 schema_name 格式
        if (formIsLeaf && formSchemaName.trim()) {
            if (!/^[a-zA-Z][a-zA-Z0-9_]*$/.test(formSchemaName.trim())) {
                notifyError('验证失败', 'Schema 名称必须以字母开头，只能包含字母、数字和下划线');
                return;
            }
        }

        try {
            if (formMode === 'add') {
                await doInsertTenant({
                    tenantName: formName.trim(),
                    parentId: formParentId?.toString() ?? null,
                    isLeaf: formIsLeaf,
                    schemaName: formIsLeaf ? formSchemaName.trim() : null,
                    enable: formEnable,
                    createdBy: null,
                });
                notifySuccess('租户添加成功');
            } else {
                if (!selectedTenant) return;
                await doUpdateTenant({
                    id: selectedTenant.id,
                    tenantName: formName.trim(),
                    enable: formEnable,
                });
                notifySuccess('租户更新成功');
            }
            setFormModalOpen(false);
            fetchTenants();
        } catch (err) {
            console.error('保存租户失败:', err);
            notifyError('保存租户失败', typeof err === 'string' ? err : undefined);
        }
    };

    // 打开删除确认
    const openDeleteModal = () => {
        setDeleteModalOpen(true);
    };

    // 确认删除
    const handleDelete = async () => {
        if (!selectedTenant) return;
        if (selectedTenant.id === 1) {
            notifyError('操作失败', '不能删除默认租户');
            return;
        }
        try {
            await doDeleteTenant(selectedTenant.id);
            setDeleteModalOpen(false);
            setSelectedTenant(null);
            notifySuccess('租户已禁用');
            fetchTenants();
        } catch (err) {
            console.error('禁用租户失败:', err);
            notifyError('禁用租户失败', typeof err === 'string' ? err : undefined);
        }
    };

    const getStatusBadge = (enable: boolean) => {
        if (enable) {
            return <Badge color="green">启用</Badge>;
        }
        return <Badge color="red">禁用</Badge>;
    };

    // 递归渲染树节点
    const renderTreeNode = (node: TreeNode, depth: number = 0) => {
        const isSelected = selectedTenant?.id === node.id;
        const hasChildren = node.children.length > 0;

        return (
            <React.Fragment key={node.id}>
                <Box
                    style={{
                        paddingLeft: `${depth * 20 + 8}px`,
                        paddingRight: '8px',
                        paddingTop: '6px',
                        paddingBottom: '6px',
                        cursor: 'pointer',
                        borderRadius: 6,
                        backgroundColor: isSelected
                            ? 'var(--mantine-color-blue-light)'
                            : 'transparent',
                        display: 'flex',
                        alignItems: 'center',
                        gap: '4px',
                        userSelect: 'none',
                    }}
                    onClick={() => {
                        const tenant = tenants.find((t) => t.id === node.id) || null;
                        setSelectedTenant(tenant);
                    }}
                    onMouseEnter={(e) => {
                        if (!isSelected) {
                            e.currentTarget.style.backgroundColor =
                                'var(--mantine-color-gray-light)';
                        }
                    }}
                    onMouseLeave={(e) => {
                        if (!isSelected) {
                            e.currentTarget.style.backgroundColor = 'transparent';
                        }
                    }}
                >
                    {hasChildren ? (
                        <ActionIcon
                            variant="subtle"
                            size="sm"
                            onClick={(e) => {
                                e.stopPropagation();
                                node.expanded = !node.expanded;
                                setTreeData([...treeData]);
                            }}
                        >
                            {node.expanded ? (
                                <IconChevronDown size={14} />
                            ) : (
                                <IconChevronRight size={14} />
                            )}
                        </ActionIcon>
                    ) : (
                        <Box w={22} />
                    )}
                    {depth === 0 ? (
                        <IconBuildingStore size={16} style={{ flexShrink: 0 }} />
                    ) : (
                        <IconBuildingCommunity size={16} style={{ flexShrink: 0 }} />
                    )}
                    <Text size="sm" fw={isSelected ? 600 : 400} lineClamp={1}>
                        {node.tenant_name}
                    </Text>
                    {!node.enable && (
                        <Badge size="xs" color="red" variant="light">
                            已禁用
                        </Badge>
                    )}
                    {node.is_leaf && (
                        <Badge size="xs" color="blue" variant="light">
                            末级
                        </Badge>
                    )}
                </Box>
                {hasChildren && node.expanded && (
                    <>
                        {node.children.map((child) => renderTreeNode(child, depth + 1))}
                    </>
                )}
            </React.Fragment>
        );
    };

    return (
        <Layout>
            <Stack gap="lg">
                <Group justify="space-between">
                    <div>
                        <Title order={2}>租户管理</Title>
                        <Text c="dimmed">管理多租户树状结构</Text>
                    </div>
                    <Group>
                        <Button
                            variant="light"
                            leftSection={<IconRefresh size={16} />}
                            onClick={fetchTenants}
                            loading={loading}
                        >
                            刷新
                        </Button>
                        <Button
                            leftSection={<IconPlus size={16} />}
                            onClick={() => openAddModal(null)}
                        >
                            新增根租户
                        </Button>
                    </Group>
                </Group>

                {error && (
                    <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red">
                        {error}
                    </Alert>
                )}

                <SimpleGrid cols={{ base: 1, md: 2 }} spacing="lg">
                    {/* 左侧：租户树 */}
                    <Card withBorder padding="lg" radius="md" h={600}>
                        <Group justify="space-between" mb="md">
                            <Text fw={600} size="sm">
                                租户结构
                            </Text>
                            <Text size="xs" c="dimmed">
                                {tenants.length} 个租户
                            </Text>
                        </Group>
                        <Divider mb="md" />
                        {loading ? (
                            <Group justify="center" py="xl">
                                <Loader />
                            </Group>
                        ) : treeData.length === 0 ? (
                            <Text ta="center" c="dimmed" py="xl">
                                暂无租户数据，请新增根租户
                            </Text>
                        ) : (
                            <ScrollArea h={500}>
                                {treeData.map((node) => renderTreeNode(node))}
                            </ScrollArea>
                        )}
                    </Card>

                    {/* 右侧：租户详情 */}
                    <Card withBorder padding="lg" radius="md" h={600}>
                        {selectedTenant ? (
                            <Stack gap="md">
                                <Group justify="space-between">
                                    <Text fw={600} size="sm">
                                        租户详情
                                    </Text>
                                    <Group gap="xs">
                                        <Tooltip label="新增子租户">
                                            <Button
                                                size="xs"
                                                variant="light"
                                                leftSection={<IconPlus size={14} />}
                                                onClick={() => openAddModal(selectedTenant.id)}
                                            >
                                                新增子租户
                                            </Button>
                                        </Tooltip>
                                        <Tooltip label="编辑">
                                            <ActionIcon
                                                variant="light"
                                                color="blue"
                                                onClick={openEditModal}
                                            >
                                                <IconEdit size={16} />
                                            </ActionIcon>
                                        </Tooltip>
                                        <Tooltip label="禁用">
                                            <ActionIcon
                                                variant="light"
                                                color="red"
                                                onClick={openDeleteModal}
                                                disabled={selectedTenant.id === 1}
                                            >
                                                <IconTrash size={16} />
                                            </ActionIcon>
                                        </Tooltip>
                                    </Group>
                                </Group>
                                <Divider />

                                <Paper p="md" withBorder radius="sm">
                                    <Stack gap="sm">
                                        <Group>
                                            <Text size="sm" c="dimmed" w={100}>
                                                租户名称
                                            </Text>
                                            <Text size="sm" fw={500}>
                                                {selectedTenant.tenant_name}
                                            </Text>
                                        </Group>
                                        <Group>
                                            <Text size="sm" c="dimmed" w={100}>
                                                所属上级
                                            </Text>
                                            <Text size="sm">
                                                {getParentName(selectedTenant.parent_id)}
                                            </Text>
                                        </Group>
                                        <Group>
                                            <Text size="sm" c="dimmed" w={100}>
                                                节点类型
                                            </Text>
                                            <Badge color={selectedTenant.is_leaf ? 'blue' : 'yellow'}>
                                                {selectedTenant.is_leaf ? '末级节点' : '分组节点'}
                                            </Badge>
                                        </Group>
                                        <Group>
                                            <Text size="sm" c="dimmed" w={100}>
                                                Schema
                                            </Text>
                                            <Text size="sm" ff="monospace">
                                                {selectedTenant.schema_name || '（无，非末级节点）'}
                                            </Text>
                                        </Group>
                                        <Group>
                                            <Text size="sm" c="dimmed" w={100}>
                                                状态
                                            </Text>
                                            {getStatusBadge(selectedTenant.enable)}
                                        </Group>
                                    </Stack>
                                </Paper>

                                <Text size="xs" c="dimmed">
                                    创建时间:{' '}
                                    {selectedTenant.create_at
                                        ? new Date(selectedTenant.create_at).toLocaleString('zh-CN')
                                        : '-'}
                                </Text>
                                <Text size="xs" c="dimmed">
                                    更新时间:{' '}
                                    {selectedTenant.updated_at
                                        ? new Date(selectedTenant.updated_at).toLocaleString('zh-CN')
                                        : '-'}
                                </Text>
                            </Stack>
                        ) : (
                            <Stack align="center" justify="center" h="100%" gap="md">
                                <IconBuildingStore size={48} color="var(--mantine-color-gray-4)" />
                                <Text c="dimmed">请从左侧选择一个租户查看详情</Text>
                            </Stack>
                        )}
                    </Card>
                </SimpleGrid>
            </Stack>

            {/* 新增/编辑租户弹窗 */}
            <Modal
                opened={formModalOpen}
                onClose={() => setFormModalOpen(false)}
                title={formMode === 'add' ? '新增租户' : '编辑租户'}
                size="md"
            >
                <Stack gap="md">
                    {formMode === 'add' && formParentId && (
                        <Text size="sm" c="dimmed">
                            父级租户：{getParentName(formParentId)}
                        </Text>
                    )}
                    <TextInput
                        label="租户名称"
                        placeholder="请输入租户名称"
                        required
                        value={formName}
                        onChange={(e) => setFormName(e.target.value)}
                    />
                    {formMode === 'add' && (
                        <>
                            <Switch
                                label="末级节点"
                                description="末级节点拥有独立的数据库 Schema，非末级节点仅用于分组"
                                checked={formIsLeaf}
                                onLabel="是"
                                offLabel="否"
                                onChange={(e) => {
                                    setFormIsLeaf(e.currentTarget.checked);
                                    if (!e.currentTarget.checked) {
                                        setFormSchemaName('');
                                    }
                                }}
                            />
                            {formIsLeaf && (
                                <TextInput
                                    label="Schema 名称"
                                    placeholder="例如：factory_a"
                                    required
                                    description="PostgreSQL Schema 名称，必须以字母开头，只能包含字母、数字和下划线"
                                    value={formSchemaName}
                                    onChange={(e) => setFormSchemaName(e.target.value)}
                                />
                            )}
                        </>
                    )}
                    {formMode === 'edit' && (
                        <>
                            <TextInput
                                label="Schema 名称"
                                value={formSchemaName}
                                disabled
                                description="Schema 名称创建后不可修改"
                            />
                            <Text size="sm" c="dimmed">
                                节点类型：{formIsLeaf ? '末级节点' : '分组节点'}
                            </Text>
                        </>
                    )}
                    <Switch
                        label="启用状态"
                        checked={formEnable}
                        onLabel="启用"
                        offLabel="禁用"
                        onChange={(e) => setFormEnable(e.currentTarget.checked)}
                    />
                    <Group justify="flex-end" mt="md">
                        <Button variant="default" onClick={() => setFormModalOpen(false)}>
                            取消
                        </Button>
                        <Button onClick={handleSave} loading={adding || editing}>
                            {formMode === 'add' ? '保存' : '保存修改'}
                        </Button>
                    </Group>
                </Stack>
            </Modal>

            {/* 删除确认弹窗 */}
            <Modal
                opened={deleteModalOpen}
                onClose={() => setDeleteModalOpen(false)}
                title="确认禁用租户"
                size="sm"
            >
                <Stack gap="md">
                    <Text>
                        确定要禁用租户 <strong>{selectedTenant?.tenant_name}</strong> 吗？
                    </Text>
                    <Text size="sm" c="dimmed">
                        禁用后，该租户下的所有用户将无法登录系统。您可以通过编辑重新启用。
                    </Text>
                    <Group justify="flex-end" mt="md">
                        <Button variant="default" onClick={() => setDeleteModalOpen(false)}>
                            取消
                        </Button>
                        <Button color="red" onClick={handleDelete} loading={deleting}>
                            确认禁用
                        </Button>
                    </Group>
                </Stack>
            </Modal>
        </Layout>
    );
};

export default TenantsPage;
