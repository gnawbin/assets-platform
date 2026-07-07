'use client';

import React, { useEffect, useState, useCallback, useRef } from 'react';
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
    Select,
    Loader,
    Alert,
    ActionIcon,
    Tooltip,
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
    IconChevronRight,
    IconChevronDown,
    IconBook,
    IconArrowLeft,
    IconFileUpload,
    IconBrain,
    IconCode,
} from '@tabler/icons-react';
import {
    getKnowledgeTree,
    insertKnowledgeNode,
    updateKnowledgeNode,
    deleteKnowledgeNode,
    type KnowledgeTreeNode,
} from '@/services/knowledgeService';
import {
    getKnowledgeAssetByTreeNode,
    createKnowledgeAsset,
    updateKnowledgeAsset,
    deleteKnowledgeAsset,
    attachFileToKnowledge,
    type KnowledgeAsset,
    type OkfType,
} from '@/services/knowledgeAssetService';
import MarkdownEditor from '@/components/MarkdownEditor';
import { OKF_TYPE_OPTIONS } from '@/components/MarkdownEditor/types';

// ======================== 节点图标映射 ========================

const NODE_ICON_MAP: Record<string, { icon: React.ReactNode; color: string }> = {
    folder: { icon: <IconFolder size={16} />, color: 'var(--mantine-color-yellow-6)' },
    wiki_node: { icon: <IconBrain size={16} />, color: 'var(--mantine-color-violet-6)' },
    raw_file: { icon: <IconFileUpload size={16} />, color: 'var(--mantine-color-blue-6)' },
    skill: { icon: <IconCode size={16} />, color: 'var(--mantine-color-green-6)' },
    document: { icon: <IconFileDescription size={16} />, color: 'var(--mantine-color-blue-6)' },
    link: { icon: <IconFileDescription size={16} />, color: 'var(--mantine-color-gray-6)' },
};

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
    const nodeIcon = NODE_ICON_MAP[node.node_type] || NODE_ICON_MAP.document;

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
                        e.currentTarget.style.backgroundColor = 'var(--mantine-color-gray-light)';
                    }
                }}
                onMouseLeave={(e) => {
                    if (!isSelected) {
                        e.currentTarget.style.backgroundColor = 'transparent';
                    }
                }}
            >
                <Box style={{
                    width: 16, height: 16, display: 'flex', alignItems: 'center',
                    justifyContent: 'center', flexShrink: 0,
                    visibility: hasChildren ? 'visible' : 'hidden'
                }}
                    onClick={hasChildren ? (e) => { e.stopPropagation(); setExpanded(!expanded); } : undefined}>
                    {expanded ? <IconChevronDown size={14} /> : <IconChevronRight size={14} />}
                </Box>

                <Box style={{ flexShrink: 0, display: 'flex', alignItems: 'center', marginLeft: 4 }}>
                    <span style={{ color: nodeIcon.color }}>{nodeIcon.icon}</span>
                </Box>

                <Text size="sm" style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', marginLeft: 6 }}>
                    {node.title}
                </Text>

                <Group gap={2} style={{ opacity: 0 }} className="tree-actions" onClick={(e) => e.stopPropagation()}>
                    <Tooltip label="新增子节点">
                        <ActionIcon variant="subtle" color="green" size="sm" onClick={() => onAddChild(node.id)}>
                            <IconPlus size={12} />
                        </ActionIcon>
                    </Tooltip>
                    <Tooltip label="编辑">
                        <ActionIcon variant="subtle" color="blue" size="sm" onClick={() => onEdit(node)}>
                            <IconEdit size={12} />
                        </ActionIcon>
                    </Tooltip>
                    <Tooltip label="删除">
                        <ActionIcon variant="subtle" color="red" size="sm" onClick={() => onDelete(node.id)}>
                            <IconTrash size={12} />
                        </ActionIcon>
                    </Tooltip>
                </Group>
            </Box>

            {hasChildren && expanded && (
                <Box style={{ marginLeft: 16, borderLeft: '1px solid var(--mantine-color-gray-3)', paddingLeft: 4 }}>
                    {node.children.map((child) => (
                        <TreeNode key={child.id} node={child} selectedId={selectedId}
                            onSelect={onSelect} onAddChild={onAddChild} onEdit={onEdit} onDelete={onDelete} />
                    ))}
                </Box>
            )}

            <style jsx>{`
                .tree-actions { opacity: 0; transition: opacity 0.1s; }
                div:hover > .tree-actions { opacity: 1; }
            `}</style>
        </Box>
    );
};

// ======================== 主页面 ========================

export default function KnowledgePage() {
    const [tree, setTree] = useState<KnowledgeTreeNode[]>([]);
    const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    // ---- 编辑器状态 ----
    const [okfAsset, setOkfAsset] = useState<KnowledgeAsset | null>(null);
    const [editorTitle, setEditorTitle] = useState('');
    const [editorContent, setEditorContent] = useState('');
    const [editorOkfType, setEditorOkfType] = useState<OkfType>('raw_source');
    const [editorSummary, setEditorSummary] = useState('');
    const [editorSource, setEditorSource] = useState('');
    const [editorStatus, setEditorStatus] = useState<'draft' | 'valid' | 'outdated'>('draft');
    const [editorTags, setEditorTags] = useState<string[]>([]);
    const [editorFileUrl, setEditorFileUrl] = useState<string | undefined>();
    const [editorFileName, setEditorFileName] = useState<string | undefined>();
    const [editorFileSize, setEditorFileSize] = useState<number | undefined>();
    const [showEditor, setShowEditor] = useState(false);
    const [saving, setSaving] = useState(false);

    // ---- 文件上传状态（FileAttachPanel 自管理，只需接收完成回调） ----

    // ---- 节点对话框 ----
    const [showNodeDialog, setShowNodeDialog] = useState(false);
    const [editingNode, setEditingNode] = useState<KnowledgeTreeNode | null>(null);
    const [parentIdForNew, setParentIdForNew] = useState<string | null>(null);
    const [nodeForm, setNodeForm] = useState({ title: '', node_type: 'folder', icon: '' });

    // ---- 加载知识树 ----
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

    useEffect(() => { loadTree(); }, [loadTree]);

    // ---- 选择节点 → 加载关联资产 ----
    const handleSelectNode = async (id: string) => {
        setSelectedNodeId(id);
        try {
            const asset = await getKnowledgeAssetByTreeNode(id);
            if (asset) {
                // 有关联资产
                setEditorTitle(asset.title);
                setEditorContent(asset.content || '');
                setEditorOkfType(asset.okf_type);
                setEditorSummary(asset.summary || '');
                setEditorSource(asset.source || '');
                setEditorStatus(asset.status as 'draft' | 'valid' | 'outdated');
                setEditorTags(asset.tags || []);
                setEditorFileUrl(asset.file_url || undefined);
                setEditorFileName(asset.file_name || undefined);
                setEditorFileSize(asset.file_size || undefined);
                setOkfAsset(asset);
                setShowEditor(true);
            } else {
                // 无关联资产
                setOkfAsset(null);
                setEditorTitle('');
                setEditorContent('');
                setEditorOkfType('raw_source');
                setEditorSummary('');
                setEditorSource('');
                setEditorStatus('draft');
                setEditorTags([]);
                setEditorFileUrl(undefined);
                setEditorFileName(undefined);
                setEditorFileSize(undefined);
                setShowEditor(false);
            }
        } catch (err) {
            // 真正的异常（网络错误等）
            console.error('获取知识资产失败:', err);
        }
    };

    // ---- 文件上传完成回调 ----
    const handleUploadComplete = useCallback((result: { fileUrl: string; fileName: string; fileSize: number }) => {
        setEditorFileName(result.fileName);
        setEditorFileSize(result.fileSize);
        setEditorFileUrl(result.fileUrl);
    }, []);

    // ---- 保存 ----
    const handleSave = async () => {
        if (!selectedNodeId) return;
        setSaving(true);
        try {
            let updatedAsset: KnowledgeAsset;

            if (okfAsset) {
                // 更新已有资产
                updatedAsset = await updateKnowledgeAsset({
                    id: okfAsset.id,
                    title: editorTitle,
                    content: editorContent,
                    okfType: editorOkfType,
                    summary: editorSummary,
                    source: editorSource,
                    status: editorStatus,
                    tags: editorTags,
                });

                // 如果有新上传的文件，绑定到资产
                if (editorFileUrl && okfAsset.file_url !== editorFileUrl) {
                    updatedAsset = await attachFileToKnowledge({
                        assetId: okfAsset.id,
                        fileUrl: editorFileUrl,
                        fileName: editorFileName || '',
                        fileSize: editorFileSize || 0,
                        fileMime: '',
                        fileMd5: '',
                    });
                }
            } else {
                // 创建新资产
                const created = await createKnowledgeAsset({
                    treeNodeId: selectedNodeId,
                    title: editorTitle,
                    okfType: editorOkfType,
                    content: editorContent,
                    summary: editorSummary,
                    source: editorSource,
                    tags: editorTags,
                });
                updatedAsset = created;
                setShowEditor(true);

                // 如果有文件，绑定到新创建的资产
                if (editorFileUrl) {
                    updatedAsset = await attachFileToKnowledge({
                        assetId: created.id,
                        fileUrl: editorFileUrl,
                        fileName: editorFileName || '',
                        fileSize: editorFileSize || 0,
                        fileMime: '',
                        fileMd5: '',
                    });
                }
            }

            setOkfAsset(updatedAsset);
        } catch (err) {
            console.error('保存失败', err);
        } finally {
            setSaving(false);
        }
    };

    // ---- 节点操作 ----
    const handleAddChild = (parentId: string) => {
        setParentIdForNew(parentId);
        setEditingNode(null);
        setNodeForm({ title: '', node_type: 'folder', icon: '' });
        setShowNodeDialog(true);
    };

    const handleEditNode = (node: KnowledgeTreeNode) => {
        setEditingNode(node);
        setParentIdForNew(null);
        setNodeForm({ title: node.title, node_type: node.node_type, icon: node.icon || '' });
        setShowNodeDialog(true);
    };

    const handleDeleteNode = async (id: string) => {
        if (!confirm('确定删除此节点及其所有子节点？')) return;
        try {
            await deleteKnowledgeNode(id);
            await loadTree();
            if (selectedNodeId === id) {
                setSelectedNodeId(null);
                setShowEditor(false);
                setOkfAsset(null);
            }
        } catch (err: unknown) {
            alert(err instanceof Error ? err.message : '删除失败');
        }
    };

    const handleSaveNode = async () => {
        try {
            if (editingNode) {
                await updateKnowledgeNode({ id: editingNode.id, title: nodeForm.title, icon: nodeForm.icon || undefined });
            } else {
                await insertKnowledgeNode({
                    parentId: parentIdForNew || undefined,
                    nodeType: nodeForm.node_type,
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

    const handleAddNewRoot = () => {
        setParentIdForNew(null);
        setEditingNode(null);
        setNodeForm({ title: '', node_type: 'folder', icon: '' });
        setShowNodeDialog(true);
    };

    return (
        <Layout>
            <Stack gap="lg">
                <Group justify="space-between">
                    <Group>
                        <IconBook size={28} />
                        <div>
                            <Title order={2}>OKF 知识库</Title>
                            <Text c="dimmed">知识树 + Markdown 编辑器 + S3 分片上传</Text>
                        </div>
                    </Group>
                    <Button variant="light" leftSection={<IconRefresh size={16} />} onClick={loadTree} loading={loading}>
                        刷新
                    </Button>
                </Group>

                {error && <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red">{error}</Alert>}

                <Group gap="lg" align="flex-start" grow wrap="nowrap">
                    {/* 左侧：知识树 */}
                    <Card withBorder padding="lg" radius="md" style={{ maxWidth: 320, minWidth: 280 }}>
                        <Group justify="space-between" mb="md">
                            <Text fw={600} size="sm">知识树</Text>
                            <Tooltip label="新增根节点">
                                <ActionIcon variant="light" color="blue" size="sm" onClick={handleAddNewRoot}>
                                    <IconPlus size={14} />
                                </ActionIcon>
                            </Tooltip>
                        </Group>
                        <Divider mb="md" />
                        {loading ? (
                            <Group justify="center" py="xl"><Loader /></Group>
                        ) : tree.length === 0 ? (
                            <Text ta="center" c="dimmed" py="xl" size="sm">暂无知识节点</Text>
                        ) : (
                            <ScrollArea h={500}>
                                {tree.map((node) => (
                                    <TreeNode key={node.id} node={node} selectedId={selectedNodeId}
                                        onSelect={handleSelectNode} onAddChild={handleAddChild}
                                        onEdit={handleEditNode} onDelete={handleDeleteNode} />
                                ))}
                            </ScrollArea>
                        )}
                    </Card>

                    {/* 右侧：编辑器 */}
                    <Card withBorder padding="lg" radius="md" style={{ flex: 1 }}>
                        {showEditor && selectedNodeId ? (
                            <Stack gap="md">
                                <Group>
                                    <Button variant="subtle" leftSection={<IconArrowLeft size={16} />}
                                        onClick={() => setShowEditor(false)} size="sm">返回</Button>
                                    {!okfAsset && (
                                        <Button size="sm" onClick={handleSave} loading={saving}>创建资产</Button>
                                    )}
                                </Group>
                                <MarkdownEditor
                                    title={editorTitle}
                                    onTitleChange={setEditorTitle}
                                    content={editorContent}
                                    onChange={setEditorContent}
                                    okfType={editorOkfType}
                                    onOkfTypeChange={setEditorOkfType}
                                    summary={editorSummary}
                                    onSummaryChange={setEditorSummary}
                                    source={editorSource}
                                    onSourceChange={setEditorSource}
                                    status={editorStatus}
                                    onStatusChange={setEditorStatus}
                                    tags={editorTags}
                                    onTagsChange={setEditorTags}
                                    fileUrl={editorFileUrl}
                                    fileName={editorFileName}
                                    fileSize={editorFileSize}
                                    onSave={handleSave}
                                    saving={saving}
                                    // 文件上传（FileAttachPanel 自管理）
                                    onUploadComplete={handleUploadComplete}
                                />
                            </Stack>
                        ) : (
                            <Stack gap="md">
                                <Group justify="space-between">
                                    <Text fw={600} size="sm">OKF 知识资产</Text>
                                    <Text size="xs" c="dimmed">选择一个树节点来编辑其关联资产</Text>
                                </Group>
                                <Divider />
                                {selectedNodeId && (
                                    <Button variant="light" leftSection={<IconEdit size={14} />}
                                        onClick={() => {
                                            setEditorTitle('');
                                            setEditorContent('');
                                            setEditorOkfType('raw_source');
                                            setEditorSummary('');
                                            setEditorSource('');
                                            setEditorStatus('draft');
                                            setEditorTags([]);
                                            setEditorFileUrl(undefined);
                                            setEditorFileName(undefined);
                                            setEditorFileSize(undefined);
                                            setOkfAsset(null);
                                            setShowEditor(true);
                                        }}>
                                        在此节点新建 OKF 资产
                                    </Button>
                                )}
                                <Text ta="center" c="dimmed" py="xl" size="sm">
                                    {selectedNodeId ? '点击上方按钮创建 OKF 知识资产' : '请在左侧知识树中选择一个节点'}
                                </Text>
                            </Stack>
                        )}
                    </Card>
                </Group>
            </Stack>

            {/* 节点对话框 */}
            <Modal opened={showNodeDialog} onClose={() => setShowNodeDialog(false)}
                title={editingNode ? '编辑节点' : '新增节点'} size="md">
                <Stack gap="md">
                    <Select label="节点类型"
                        data={[
                            { value: 'folder', label: '文件夹' },
                            { value: 'wiki_node', label: 'OKF词条' },
                            { value: 'raw_file', label: '原始文件' },
                            { value: 'skill', label: 'Skill规则' },
                            { value: 'document', label: '文档' },
                            { value: 'link', label: '链接' },
                        ]}
                        value={nodeForm.node_type}
                        onChange={(val) => setNodeForm({ ...nodeForm, node_type: val || 'folder' })}
                        disabled={!!editingNode}
                    />
                    <TextInput label="标题" placeholder="请输入节点标题" required
                        value={nodeForm.title}
                        onChange={(e) => setNodeForm({ ...nodeForm, title: e.target.value })} />
                    <TextInput label="图标（可选）" placeholder="图标名称"
                        value={nodeForm.icon}
                        onChange={(e) => setNodeForm({ ...nodeForm, icon: e.target.value })} />
                    <Group justify="flex-end" mt="md">
                        <Button variant="default" onClick={() => setShowNodeDialog(false)}>取消</Button>
                        <Button onClick={handleSaveNode}>保存</Button>
                    </Group>
                </Stack>
            </Modal>
        </Layout>
    );
}