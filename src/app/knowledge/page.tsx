'use client';

import React, { useEffect, useState, useCallback } from 'react';
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
    Textarea,
    Select,
    Loader,
    Alert,
    SimpleGrid,
    Badge,
    ActionIcon,
    Tooltip,
    Paper,
    ScrollArea,
    Box,
    Divider,
} from '@mantine/core';
import {
    IconAlertCircle,
    IconPlus,
    IconEdit,
    IconTrash,
    IconRefresh,
    IconFolder,
    IconFileDescription,
    IconFolderOpen,
    IconChevronRight,
    IconChevronDown,
    IconBook,
    IconArrowLeft,
} from '@tabler/icons-react';
import {
    getKnowledgeTree,
    getKnowledgeList,
    getKnowledgeById,
    insertKnowledgeNode,
    updateKnowledgeNode,
    deleteKnowledgeNode,
    insertKnowledge,
    updateKnowledge,
    deleteKnowledge,
    type KnowledgeTreeNode,
    type AssetKnowledge,
} from '@/services/knowledgeService';

// ======================== 树节点组件 ========================

interface TreeNodeProps {
    node: KnowledgeTreeNode;
    selectedId: string | null;
    onSelect: (id: string) => void;
    onAddChild: (parentId: string) => void;
    onEdit: (node: KnowledgeTreeNode) => void;
    onDelete: (id: string) => void;
}

const TreeNode: React.FC<TreeNodeProps> = ({
    node,
    selectedId,
    onSelect,
    onAddChild,
    onEdit,
    onDelete,
}) => {
    const [expanded, setExpanded] = useState(node.is_expanded);
    const hasChildren = node.children && node.children.length > 0;
    const isSelected = selectedId === node.id;

    const toggleExpand = (e: React.MouseEvent) => {
        e.stopPropagation();
        setExpanded(!expanded);
    };

    return (
        <Box>
            <Box
                style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: '2px',
                    padding: '4px 8px',
                    cursor: 'pointer',
                    borderRadius: 6,
                    fontSize: '14px',
                    backgroundColor: isSelected
                        ? 'var(--mantine-color-blue-light)'
                        : 'transparent',
                    color: isSelected
                        ? 'var(--mantine-color-blue-filled)'
                        : 'var(--mantine-color-gray-7)',
                }}
                onClick={() => onSelect(node.id)}
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
                {/* 展开/折叠按钮 */}
                <Box
                    style={{
                        width: 16,
                        height: 16,
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        flexShrink: 0,
                        visibility: hasChildren ? 'visible' : 'hidden',
                    }}
                    onClick={hasChildren ? toggleExpand : undefined}
                >
                    {expanded ? (
                        <IconChevronDown size={14} />
                    ) : (
                        <IconChevronRight size={14} />
                    )}
                </Box>

                {/* 图标 */}
                <Box style={{ flexShrink: 0, display: 'flex', alignItems: 'center' }}>
                    {node.node_type === 'folder' ? (
                        expanded && hasChildren ? (
                            <IconFolderOpen size={16} color="var(--mantine-color-yellow-6)" />
                        ) : (
                            <IconFolder size={16} color="var(--mantine-color-yellow-6)" />
                        )
                    ) : (
                        <IconFileDescription size={16} color="var(--mantine-color-blue-6)" />
                    )}
                </Box>

                {/* 标题 */}
                <Text
                    size="sm"
                    style={{
                        flex: 1,
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        whiteSpace: 'nowrap',
                        marginLeft: 4,
                    }}
                >
                    {node.title}
                </Text>

                {/* 操作按钮 */}
                <Group
                    gap={2}
                    style={{ opacity: 0 }}
                    className="tree-actions"
                    onClick={(e) => e.stopPropagation()}
                >
                    <Tooltip label="新增子节点">
                        <ActionIcon
                            variant="subtle"
                            color="green"
                            size="sm"
                            onClick={() => onAddChild(node.id)}
                        >
                            <IconPlus size={12} />
                        </ActionIcon>
                    </Tooltip>
                    <Tooltip label="编辑">
                        <ActionIcon
                            variant="subtle"
                            color="blue"
                            size="sm"
                            onClick={() => onEdit(node)}
                        >
                            <IconEdit size={12} />
                        </ActionIcon>
                    </Tooltip>
                    <Tooltip label="删除">
                        <ActionIcon
                            variant="subtle"
                            color="red"
                            size="sm"
                            onClick={() => onDelete(node.id)}
                        >
                            <IconTrash size={12} />
                        </ActionIcon>
                    </Tooltip>
                </Group>
            </Box>

            {/* 子节点 */}
            {hasChildren && expanded && (
                <Box
                    style={{
                        marginLeft: 16,
                        borderLeft: '1px solid var(--mantine-color-gray-3)',
                        paddingLeft: 4,
                    }}
                >
                    {node.children.map((child) => (
                        <TreeNode
                            key={child.id}
                            node={child}
                            selectedId={selectedId}
                            onSelect={onSelect}
                            onAddChild={onAddChild}
                            onEdit={onEdit}
                            onDelete={onDelete}
                        />
                    ))}
                </Box>
            )}

            <style jsx>{`
                .tree-actions {
                    opacity: 0;
                    transition: opacity 0.1s;
                }
                div:hover > .tree-actions {
                    opacity: 1;
                }
            `}</style>
        </Box>
    );
};

// ======================== 主页面 ========================

export default function KnowledgePage() {
    const [tree, setTree] = useState<KnowledgeTreeNode[]>([]);
    const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
    const [knowledgeList, setKnowledgeList] = useState<AssetKnowledge[]>([]);
    const [selectedKnowledge, setSelectedKnowledge] = useState<AssetKnowledge | null>(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    // 对话框状态
    const [showNodeDialog, setShowNodeDialog] = useState(false);
    const [showKnowledgeDialog, setShowKnowledgeDialog] = useState(false);
    const [editingNode, setEditingNode] = useState<KnowledgeTreeNode | null>(null);
    const [editingKnowledge, setEditingKnowledge] = useState<AssetKnowledge | null>(null);
    const [parentIdForNew, setParentIdForNew] = useState<string | null>(null);

    // 表单状态
    const [nodeForm, setNodeForm] = useState({ title: '', node_type: 'folder', icon: '' });
    const [knowledgeForm, setKnowledgeForm] = useState({
        title: '',
        content: '',
        knowledge_type: 'basic',
        permission_level: 'internal',
    });

    // 加载知识树
    const loadTree = useCallback(async () => {
        try {
            setLoading(true);
            setError(null);
            const data = await getKnowledgeTree();
            setTree(data);
        } catch (err: unknown) {
            setError(err instanceof Error ? err.message : '加载知识树失败');
        } finally {
            setLoading(false);
        }
    }, []);

    // 加载知识条目列表
    const loadKnowledgeList = useCallback(async (nodeId: string | null) => {
        try {
            if (nodeId) {
                const data = await getKnowledgeList({ knowledge_id: nodeId });
                setKnowledgeList(data);
            } else {
                const data = await getKnowledgeList();
                setKnowledgeList(data);
            }
        } catch {
            setKnowledgeList([]);
        }
    }, []);

    useEffect(() => {
        loadTree();
    }, [loadTree]);

    useEffect(() => {
        loadKnowledgeList(selectedNodeId);
        setSelectedKnowledge(null);
    }, [selectedNodeId, loadKnowledgeList]);

    // 选择节点
    const handleSelectNode = (id: string) => {
        setSelectedNodeId(id);
    };

    // ======================== 节点操作 ========================

    const handleAddChild = (parentId: string) => {
        setParentIdForNew(parentId);
        setEditingNode(null);
        setNodeForm({ title: '', node_type: 'folder', icon: '' });
        setShowNodeDialog(true);
    };

    const handleEditNode = (node: KnowledgeTreeNode) => {
        setEditingNode(node);
        setParentIdForNew(null);
        setNodeForm({
            title: node.title,
            node_type: node.node_type,
            icon: node.icon || '',
        });
        setShowNodeDialog(true);
    };

    const handleDeleteNode = async (id: string) => {
        if (!confirm('确定删除此节点及其所有子节点？')) return;
        try {
            await deleteKnowledgeNode(id);
            await loadTree();
            if (selectedNodeId === id) {
                setSelectedNodeId(null);
            }
        } catch (err: unknown) {
            alert(err instanceof Error ? err.message : '删除失败');
        }
    };

    const handleSaveNode = async () => {
        try {
            if (editingNode) {
                await updateKnowledgeNode({
                    id: editingNode.id,
                    title: nodeForm.title,
                    icon: nodeForm.icon || undefined,
                });
            } else {
                await insertKnowledgeNode({
                    parent_id: parentIdForNew || undefined,
                    node_type: nodeForm.node_type,
                    title: nodeForm.title,
                    icon: nodeForm.icon || undefined,
                });
            }
            setShowNodeDialog(false);
            await loadTree();
        } catch (err: unknown) {
            alert(err instanceof Error ? err.message : '保存失败');
        }
    };

    // ======================== 知识条目操作 ========================

    const handleViewKnowledge = async (id: string) => {
        try {
            const data = await getKnowledgeById(id);
            setSelectedKnowledge(data);
        } catch {
            // ignore
        }
    };

    const handleAddKnowledge = () => {
        setEditingKnowledge(null);
        setKnowledgeForm({
            title: '',
            content: '',
            knowledge_type: 'basic',
            permission_level: 'internal',
        });
        setShowKnowledgeDialog(true);
    };

    const handleEditKnowledge = (item: AssetKnowledge) => {
        setEditingKnowledge(item);
        setKnowledgeForm({
            title: item.title,
            content: item.content,
            knowledge_type: item.knowledge_type,
            permission_level: item.permission_level,
        });
        setShowKnowledgeDialog(true);
    };

    const handleDeleteKnowledge = async (id: string) => {
        if (!confirm('确定删除此知识条目？')) return;
        try {
            await deleteKnowledge(id);
            await loadKnowledgeList(selectedNodeId);
            if (selectedKnowledge?.id === id) {
                setSelectedKnowledge(null);
            }
        } catch (err: unknown) {
            alert(err instanceof Error ? err.message : '删除失败');
        }
    };

    const handleSaveKnowledge = async () => {
        try {
            if (editingKnowledge) {
                await updateKnowledge({
                    id: editingKnowledge.id,
                    title: knowledgeForm.title,
                    content: knowledgeForm.content,
                    knowledge_type: knowledgeForm.knowledge_type,
                    permission_level: knowledgeForm.permission_level,
                });
            } else {
                await insertKnowledge({
                    knowledge_id: selectedNodeId || undefined,
                    title: knowledgeForm.title,
                    content: knowledgeForm.content,
                    knowledge_type: knowledgeForm.knowledge_type,
                    permission_level: knowledgeForm.permission_level,
                });
            }
            setShowKnowledgeDialog(false);
            await loadKnowledgeList(selectedNodeId);
        } catch (err: unknown) {
            alert(err instanceof Error ? err.message : '保存失败');
        }
    };

    // ======================== 渲染 ========================

    return (
        <Layout>
            <Stack gap="lg">
                {/* 页面标题 */}
                <Group justify="space-between">
                    <Group>
                        <IconBook size={28} />
                        <div>
                            <Title order={2}>知识库</Title>
                            <Text c="dimmed">管理知识树和知识条目</Text>
                        </div>
                    </Group>
                    <Group>
                        <Button
                            variant="light"
                            leftSection={<IconRefresh size={16} />}
                            onClick={loadTree}
                            loading={loading}
                        >
                            刷新
                        </Button>
                    </Group>
                </Group>

                {error && (
                    <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red">
                        {error}
                    </Alert>
                )}

                {/* 左右分栏 */}
                <Group gap="lg" align="flex-start" grow wrap="nowrap">
                    {/* 左侧：知识树 */}
                    <Card withBorder padding="lg" radius="md" style={{ maxWidth: 320, minWidth: 280 }}>
                        <Group justify="space-between" mb="md">
                            <Text fw={600} size="sm">
                                知识树
                            </Text>
                            <Tooltip label="新增根节点">
                                <ActionIcon
                                    variant="light"
                                    color="blue"
                                    size="sm"
                                    onClick={() => {
                                        setParentIdForNew(null);
                                        setEditingNode(null);
                                        setNodeForm({ title: '', node_type: 'folder', icon: '' });
                                        setShowNodeDialog(true);
                                    }}
                                >
                                    <IconPlus size={14} />
                                </ActionIcon>
                            </Tooltip>
                        </Group>
                        <Divider mb="md" />

                        {loading ? (
                            <Group justify="center" py="xl">
                                <Loader />
                            </Group>
                        ) : tree.length === 0 ? (
                            <Text ta="center" c="dimmed" py="xl" size="sm">
                                暂无知识节点
                            </Text>
                        ) : (
                            <ScrollArea h={500}>
                                {tree.map((node) => (
                                    <TreeNode
                                        key={node.id}
                                        node={node}
                                        selectedId={selectedNodeId}
                                        onSelect={handleSelectNode}
                                        onAddChild={handleAddChild}
                                        onEdit={handleEditNode}
                                        onDelete={handleDeleteNode}
                                    />
                                ))}
                            </ScrollArea>
                        )}
                    </Card>

                    {/* 右侧：知识条目列表 / 详情 */}
                    <Card withBorder padding="lg" radius="md" style={{ flex: 1 }}>
                        {selectedKnowledge ? (
                            // 知识条目详情
                            <Stack gap="md">
                                <Group>
                                    <Button
                                        variant="subtle"
                                        leftSection={<IconArrowLeft size={16} />}
                                        onClick={() => setSelectedKnowledge(null)}
                                        size="sm"
                                    >
                                        返回列表
                                    </Button>
                                    <Group gap="xs" ml="auto">
                                        <Button
                                            size="compact-sm"
                                            variant="light"
                                            leftSection={<IconEdit size={14} />}
                                            onClick={() => handleEditKnowledge(selectedKnowledge)}
                                        >
                                            编辑
                                        </Button>
                                        <Button
                                            size="compact-sm"
                                            variant="light"
                                            color="red"
                                            leftSection={<IconTrash size={14} />}
                                            onClick={() => handleDeleteKnowledge(selectedKnowledge.id)}
                                        >
                                            删除
                                        </Button>
                                    </Group>
                                </Group>
                                <Divider />

                                <Title order={3}>{selectedKnowledge.title}</Title>

                                <Group gap="lg">
                                    <Badge variant="light" color="blue" size="sm">
                                        类型：{selectedKnowledge.knowledge_type}
                                    </Badge>
                                    <Badge variant="light" color="gray" size="sm">
                                        来源：{selectedKnowledge.doc_source}
                                    </Badge>
                                    <Badge variant="light" color="teal" size="sm">
                                        权限：{selectedKnowledge.permission_level}
                                    </Badge>
                                </Group>

                                <Paper p="md" withBorder radius="sm">
                                    <Text size="sm" style={{ whiteSpace: 'pre-wrap', lineHeight: 1.7 }}>
                                        {selectedKnowledge.content}
                                    </Text>
                                </Paper>
                            </Stack>
                        ) : (
                            // 知识条目列表
                            <Stack gap="md">
                                <Group justify="space-between">
                                    <Text fw={600} size="sm">
                                        {selectedNodeId ? '知识条目' : '全部知识条目'}
                                    </Text>
                                    <Button
                                        size="compact-sm"
                                        leftSection={<IconPlus size={14} />}
                                        onClick={handleAddKnowledge}
                                    >
                                        新增
                                    </Button>
                                </Group>
                                <Divider />

                                {knowledgeList.length === 0 ? (
                                    <Text ta="center" c="dimmed" py="xl" size="sm">
                                        暂无知识条目
                                    </Text>
                                ) : (
                                    <SimpleGrid cols={{ base: 1, md: 2 }} spacing="md">
                                        {knowledgeList.map((item) => (
                                            <Card
                                                key={item.id}
                                                withBorder
                                                padding="md"
                                                radius="sm"
                                                style={{ cursor: 'pointer' }}
                                                onClick={() => handleViewKnowledge(item.id)}
                                            >
                                                <Group justify="space-between" align="flex-start" wrap="nowrap">
                                                    <Text fw={500} size="sm" lineClamp={1} style={{ flex: 1 }}>
                                                        {item.title}
                                                    </Text>
                                                    <Group gap={2} wrap="nowrap" onClick={(e) => e.stopPropagation()}>
                                                        <Tooltip label="编辑">
                                                            <ActionIcon
                                                                variant="subtle"
                                                                color="blue"
                                                                size="sm"
                                                                onClick={() => handleEditKnowledge(item)}
                                                            >
                                                                <IconEdit size={14} />
                                                            </ActionIcon>
                                                        </Tooltip>
                                                        <Tooltip label="删除">
                                                            <ActionIcon
                                                                variant="subtle"
                                                                color="red"
                                                                size="sm"
                                                                onClick={() => handleDeleteKnowledge(item.id)}
                                                            >
                                                                <IconTrash size={14} />
                                                            </ActionIcon>
                                                        </Tooltip>
                                                    </Group>
                                                </Group>
                                                <Text size="xs" c="dimmed" lineClamp={2} mt="xs">
                                                    {item.content}
                                                </Text>
                                                <Group gap="md" mt="sm">
                                                    <Badge variant="light" color="blue" size="xs">
                                                        {item.knowledge_type}
                                                    </Badge>
                                                    <Badge variant="light" color="teal" size="xs">
                                                        {item.permission_level}
                                                    </Badge>
                                                </Group>
                                            </Card>
                                        ))}
                                    </SimpleGrid>
                                )}
                            </Stack>
                        )}
                    </Card>
                </Group>
            </Stack>

            {/* ======================== 节点对话框 ======================== */}
            <Modal
                opened={showNodeDialog}
                onClose={() => setShowNodeDialog(false)}
                title={editingNode ? '编辑节点' : '新增节点'}
                size="md"
            >
                <Stack gap="md">
                    <Select
                        label="节点类型"
                        data={[
                            { value: 'folder', label: '文件夹' },
                            { value: 'document', label: '文档' },
                        ]}
                        value={nodeForm.node_type}
                        onChange={(val) =>
                            setNodeForm({ ...nodeForm, node_type: val || 'folder' })
                        }
                        disabled={!!editingNode}
                    />
                    <TextInput
                        label="标题"
                        placeholder="请输入节点标题"
                        required
                        value={nodeForm.title}
                        onChange={(e) =>
                            setNodeForm({ ...nodeForm, title: e.target.value })
                        }
                    />
                    <TextInput
                        label="图标（可选）"
                        placeholder="图标名称"
                        value={nodeForm.icon}
                        onChange={(e) =>
                            setNodeForm({ ...nodeForm, icon: e.target.value })
                        }
                    />
                    <Group justify="flex-end" mt="md">
                        <Button variant="default" onClick={() => setShowNodeDialog(false)}>
                            取消
                        </Button>
                        <Button onClick={handleSaveNode}>保存</Button>
                    </Group>
                </Stack>
            </Modal>

            {/* ======================== 知识条目对话框 ======================== */}
            <Modal
                opened={showKnowledgeDialog}
                onClose={() => setShowKnowledgeDialog(false)}
                title={editingKnowledge ? '编辑知识条目' : '新增知识条目'}
                size="lg"
            >
                <Stack gap="md">
                    <TextInput
                        label="标题"
                        placeholder="请输入知识条目标题"
                        required
                        value={knowledgeForm.title}
                        onChange={(e) =>
                            setKnowledgeForm({
                                ...knowledgeForm,
                                title: e.target.value,
                            })
                        }
                    />
                    <Group grow>
                        <Select
                            label="知识类型"
                            data={[
                                { value: 'basic', label: '基础' },
                                { value: 'contract', label: '合同' },
                                { value: 'hardware', label: '硬件' },
                                { value: 'intangible', label: '无形资产' },
                            ]}
                            value={knowledgeForm.knowledge_type}
                            onChange={(val) =>
                                setKnowledgeForm({
                                    ...knowledgeForm,
                                    knowledge_type: val || 'basic',
                                })
                            }
                        />
                        <Select
                            label="权限等级"
                            data={[
                                { value: 'public', label: '公开' },
                                { value: 'internal', label: '内部' },
                                { value: 'secret', label: '保密' },
                            ]}
                            value={knowledgeForm.permission_level}
                            onChange={(val) =>
                                setKnowledgeForm({
                                    ...knowledgeForm,
                                    permission_level: val || 'internal',
                                })
                            }
                        />
                    </Group>
                    <Textarea
                        label="内容"
                        placeholder="请输入知识条目内容"
                        minRows={8}
                        value={knowledgeForm.content}
                        onChange={(e) =>
                            setKnowledgeForm({
                                ...knowledgeForm,
                                content: e.target.value,
                            })
                        }
                    />
                    <Group justify="flex-end" mt="md">
                        <Button variant="default" onClick={() => setShowKnowledgeDialog(false)}>
                            取消
                        </Button>
                        <Button onClick={handleSaveKnowledge}>保存</Button>
                    </Group>
                </Stack>
            </Modal>
        </Layout>
    );
}
