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
    Modal,
    TextInput,
    Select,
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
    listKnowledgeAssets,
    attachFileToKnowledge,
    type KnowledgeAsset,
    type OkfType,
} from '@/services/knowledgeAssetService';
import { UploadService } from '@/services/uploadService';
import { notifications } from '@mantine/notifications';
import MarkdownEditor from '@/components/MarkdownEditor';
import { OKF_TYPE_OPTIONS } from '@/components/MarkdownEditor/types';
import type { AttachUploadStatus } from '@/components/MarkdownEditor/FileAttachPanel';

// ======================== 节点图标 ========================

const NODE_ICON_MAP: Record<string, { icon: React.ReactNode; color: string }> = {
    folder: { icon: <IconFolder size={16} />, color: 'var(--mantine-color-yellow-6)' },
    wiki_node: { icon: <IconBrain size={16} />, color: 'var(--mantine-color-violet-6)' },
    raw_file: { icon: <IconFileUpload size={16} />, color: 'var(--mantine-color-blue-6)' },
    skill: { icon: <IconCode size={16} />, color: 'var(--mantine-color-green-6)' },
    document: { icon: <IconFileDescription size={16} />, color: 'var(--mantine-color-blue-6)' },
    link: { icon: <IconFileDescription size={16} />, color: 'var(--mantine-color-gray-6)' },
};

const NODE_TYPE_LABELS: Record<string, string> = {
    folder: '文件夹',
    wiki_node: '词条',
    raw_file: '原始文件',
    skill: 'Skill规则',
    document: '文档',
    link: '链接',
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
                <Box
                    style={{
                        width: 16, height: 16, display: 'flex', alignItems: 'center',
                        justifyContent: 'center', flexShrink: 0,
                        visibility: hasChildren ? 'visible' : 'hidden',
                    }}
                    onClick={hasChildren ? (e) => { e.stopPropagation(); setExpanded(!expanded); } : undefined}
                >
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

export default function KnowledgeAssetPage() {
    const [tree, setTree] = useState<KnowledgeTreeNode[]>([]);
    const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
    const [assetList, setAssetList] = useState<KnowledgeAsset[]>([]);
    const [editingAsset, setEditingAsset] = useState<KnowledgeAsset | null>(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [saving, setSaving] = useState(false);

    // 对话框
    const [showNodeDialog, setShowNodeDialog] = useState(false);
    const [editingNode, setEditingNode] = useState<KnowledgeTreeNode | null>(null);
    const [parentIdForNew, setParentIdForNew] = useState<string | null>(null);
    const [nodeForm, setNodeForm] = useState({ title: '', node_type: 'folder', icon: '' });

    // 编辑器状态
    const [editorTitle, setEditorTitle] = useState('');
    const [editorContent, setEditorContent] = useState('');
    const [editorOkfType, setEditorOkfType] = useState<OkfType>('raw_source');
    const [editorSummary, setEditorSummary] = useState('');
    const [editorSource, setEditorSource] = useState('');
    const [editorStatus, setEditorStatus] = useState<'draft' | 'valid' | 'outdated'>('draft');
    const [editorTags, setEditorTags] = useState<string[]>([]);
    const [showEditor, setShowEditor] = useState(false);

    // 编辑器文件信息
    const [editorFileUrl, setEditorFileUrl] = useState<string | undefined>();
    const [editorFileName, setEditorFileName] = useState<string | undefined>();
    const [editorFileSize, setEditorFileSize] = useState<number | undefined>();

    // ---- 文件上传状态 ----
    const [uploadStatus, setUploadStatus] = useState<AttachUploadStatus>('idle');
    const [uploadProgress, setUploadProgress] = useState(0);
    const [uploadSpeed, setUploadSpeed] = useState(0);
    const [uploadError, setUploadError] = useState<string | null>(null);
    const uploadServiceRef = useRef(new UploadService());
    const pausedRef = useRef(false);
    const selectedFileRef = useRef<File | null>(null);
    const uploadIdRef = useRef<string | null>(null);

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

    // 加载资产列表
    const loadAssetList = useCallback(async () => {
        try {
            const data = await listKnowledgeAssets();
            setAssetList(data);
        } catch {
            setAssetList([]);
        }
    }, []);

    useEffect(() => { loadTree(); loadAssetList(); }, [loadTree, loadAssetList]);

    // ---- 重置上传状态 ----
    const resetUploadState = useCallback(() => {
        setUploadStatus('idle');
        setUploadProgress(0);
        setUploadSpeed(0);
        setUploadError(null);
        pausedRef.current = false;
        selectedFileRef.current = null;
        uploadIdRef.current = null;
    }, []);

    // ---- S3 分片上传核心逻辑 ----
    const startChunkedUpload = useCallback(async (file: File) => {
        const uploadService = uploadServiceRef.current;
        pausedRef.current = false;
        selectedFileRef.current = file;

        setUploadStatus('uploading');
        setUploadProgress(0);
        setUploadError(null);

        try {
            // 1. 初始化分片上传（占位）
            const initResp = await uploadService.init(file.name, file.size, file.type);
            const uploadId = initResp.uploadId;
            uploadIdRef.current = uploadId;

            // 2. 开始上传（创建 S3 MultipartUpload，返回 presigned URLs）
            const startResp = await uploadService.startUpload(uploadId);
            const { chunkSize, totalChunks, presignedUrls } = startResp;

            // 3. 分片
            const chunks: Blob[] = [];
            for (let start = 0; start < file.size; start += chunkSize) {
                chunks.push(file.slice(start, Math.min(start + chunkSize, file.size)));
            }

            // 4. 并发上传分片（并发数 3）
            const concurrency = 3;
            let uploadedCount = 0;
            let lastLoaded = 0;
            let lastTime = Date.now();

            const uploadOneChunk = async (partNumber: number): Promise<void> => {
                if (pausedRef.current) return;
                const presignedUrl = presignedUrls[partNumber - 1];
                const chunk = chunks[partNumber - 1];

                const etag = await uploadService.uploadChunk(presignedUrl, chunk, partNumber);
                await uploadService.reportChunk(uploadId, partNumber, etag);

                uploadedCount++;
                const pct = Math.round((uploadedCount / totalChunks) * 100);
                setUploadProgress(pct);

                const now = Date.now();
                const elapsed = (now - lastTime) / 1000;
                if (elapsed > 0.5) {
                    const currentLoaded = uploadedCount * chunkSize;
                    const bytesPerSec = (currentLoaded - lastLoaded) / elapsed;
                    setUploadSpeed(bytesPerSec);
                    lastLoaded = currentLoaded;
                    lastTime = now;
                }
            };

            const workers = [];
            for (let i = 0; i < concurrency; i++) {
                workers.push(
                    (async () => {
                        for (let j = i; j < totalChunks; j += concurrency) {
                            if (pausedRef.current) break;
                            await uploadOneChunk(j + 1);
                        }
                    })()
                );
            }
            await Promise.all(workers);

            if (pausedRef.current) {
                setUploadStatus('paused');
                return;
            }

            // 4. 完成合并
            const result = await uploadService.complete(uploadId);

            // 5. 更新编辑器文件信息
            setEditorFileName(file.name);
            setEditorFileSize(file.size);
            setEditorFileUrl(result.fileUrl);
            setUploadStatus('completed');

            notifications.show({
                title: '上传成功',
                message: `${file.name} 已上传至对象存储`,
                color: 'green',
            });
        } catch (err: any) {
            if (pausedRef.current) return;
            const errMsg = err.message || '上传失败';
            setUploadError(errMsg);
            setUploadStatus('error');
            notifications.show({
                title: '上传失败',
                message: errMsg,
                color: 'red',
            });
        }
    }, []);

    // ---- 文件选择回调 ----
    const handleFileSelect = useCallback((file: File) => {
        startChunkedUpload(file);
    }, [startChunkedUpload]);

    // ---- 上传控制 ----
    const handlePause = useCallback(() => {
        pausedRef.current = true;
        setUploadStatus('paused');
    }, []);

    const handleResume = useCallback(() => {
        if (selectedFileRef.current) {
            startChunkedUpload(selectedFileRef.current);
        }
    }, [startChunkedUpload]);

    const handleCancel = useCallback(async () => {
        pausedRef.current = true;
        if (uploadIdRef.current) {
            try {
                await uploadServiceRef.current.abort(uploadIdRef.current);
            } catch {
                // ignore
            }
        }
        resetUploadState();
        setEditorFileUrl(undefined);
        setEditorFileName(undefined);
        setEditorFileSize(undefined);
    }, [resetUploadState]);

    const handleRetry = useCallback(() => {
        if (selectedFileRef.current) {
            resetUploadState();
            startChunkedUpload(selectedFileRef.current);
        }
    }, [resetUploadState, startChunkedUpload]);

    // 选择节点 → 加载关联资产
    const handleSelectNode = async (id: string) => {
        setSelectedNodeId(id);
        resetUploadState();
        try {
            const asset = await getKnowledgeAssetByTreeNode(id);
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
            setEditingAsset(asset);
            setShowEditor(true);
        } catch {
            // 无关联资产，新建模式
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
            setEditingAsset(null);
            setShowEditor(true);
        }
    };

    // 保存编辑器内容
    const handleSave = async () => {
        if (!selectedNodeId) return;
        setSaving(true);
        try {
            let updatedAsset: KnowledgeAsset;

            if (editingAsset) {
                updatedAsset = await updateKnowledgeAsset({
                    id: editingAsset.id,
                    title: editorTitle,
                    content: editorContent,
                    okfType: editorOkfType,
                    summary: editorSummary,
                    source: editorSource,
                    status: editorStatus,
                    tags: editorTags,
                });

                // 如果有新上传的文件，绑定到资产
                if (editorFileUrl && editingAsset.file_url !== editorFileUrl) {
                    updatedAsset = await attachFileToKnowledge({
                        assetId: editingAsset.id,
                        fileUrl: editorFileUrl,
                        fileName: editorFileName || '',
                        fileSize: editorFileSize || 0,
                        fileMime: '',
                        fileMd5: '',
                    });
                }
                setEditingAsset(updatedAsset);
            } else {
                const created = await createKnowledgeAsset({
                    treeNodeId: selectedNodeId,
                    title: editorTitle,
                    okfType: editorOkfType,
                    content: editorContent,
                    summary: editorSummary,
                    source: editorSource,
                    tags: editorTags,
                });
                setEditingAsset(created);

                // 如果有文件，绑定到新创建的资产
                if (editorFileUrl) {
                    await attachFileToKnowledge({
                        assetId: created.id,
                        fileUrl: editorFileUrl,
                        fileName: editorFileName || '',
                        fileSize: editorFileSize || 0,
                        fileMime: '',
                        fileMd5: '',
                    });
                }
            }
            await loadAssetList();
        } catch (err) {
            console.error('保存失败', err);
        } finally {
            setSaving(false);
        }
    };

    // 节点操作
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
            if (selectedNodeId === id) { setSelectedNodeId(null); setShowEditor(false); }
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

    const handleDeleteAsset = async (id: string) => {
        if (!confirm('确定删除此知识资产？')) return;
        try {
            await deleteKnowledgeAsset(id);
            await loadAssetList();
            if (editingAsset?.id === id) { setEditingAsset(null); setShowEditor(false); }
        } catch {
            // ignore
        }
    };

    const okfTypeLabel = (t: string) => OKF_TYPE_OPTIONS.find(o => o.value === t)?.label || t;

    return (
        <Layout>
            <Stack gap="lg">
                <Group justify="space-between">
                    <Group>
                        <IconBook size={28} />
                        <div>
                            <Title order={2}>OKF 知识库</Title>
                            <Text c="dimmed">标准化知识资产 + Markdown 编辑器 + S3 分片上传</Text>
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

                    {/* 右侧：编辑器 / 资产列表 */}
                    <Card withBorder padding="lg" radius="md" style={{ flex: 1 }}>
                        {showEditor && selectedNodeId ? (
                            <Stack gap="md">
                                <Group>
                                    <Button variant="subtle" leftSection={<IconArrowLeft size={16} />}
                                        onClick={() => setShowEditor(false)} size="sm">返回列表</Button>
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
                                    // 文件上传状态
                                    uploadStatus={uploadStatus}
                                    uploadProgress={uploadProgress}
                                    uploadSpeed={uploadSpeed}
                                    uploadError={uploadError}
                                    onFileSelect={handleFileSelect}
                                    onPause={handlePause}
                                    onResume={handleResume}
                                    onCancel={handleCancel}
                                    onRetry={handleRetry}
                                />
                            </Stack>
                        ) : (
                            <Stack gap="md">
                                <Group justify="space-between">
                                    <Text fw={600} size="sm">全部知识资产</Text>
                                    <Text size="xs" c="dimmed">选择一个树节点来编辑其关联资产</Text>
                                </Group>
                                <Divider />
                                {assetList.length === 0 ? (
                                    <Text ta="center" c="dimmed" py="xl" size="sm">暂无知识资产</Text>
                                ) : (
                                    <SimpleGrid cols={{ base: 1, md: 2 }} spacing="md">
                                        {assetList.map((item) => (
                                            <Card key={item.id} withBorder padding="md" radius="sm">
                                                <Group justify="space-between" align="flex-start" wrap="nowrap">
                                                    <Text fw={500} size="sm" lineClamp={1} style={{ flex: 1 }}>
                                                        {item.title}
                                                    </Text>
                                                    <Group gap={2} wrap="nowrap">
                                                        <Tooltip label="编辑">
                                                            <ActionIcon variant="subtle" color="blue" size="sm"
                                                                onClick={() => handleSelectNode(item.tree_node_id)}>
                                                                <IconEdit size={14} />
                                                            </ActionIcon>
                                                        </Tooltip>
                                                        <Tooltip label="删除">
                                                            <ActionIcon variant="subtle" color="red" size="sm"
                                                                onClick={() => handleDeleteAsset(item.id)}>
                                                                <IconTrash size={14} />
                                                            </ActionIcon>
                                                        </Tooltip>
                                                    </Group>
                                                </Group>
                                                {item.summary && (
                                                    <Text size="xs" c="dimmed" lineClamp={2} mt="xs">{item.summary}</Text>
                                                )}
                                                <Group gap="md" mt="sm">
                                                    <Badge variant="light" color="violet" size="xs">{okfTypeLabel(item.okf_type)}</Badge>
                                                    <Badge variant="light" color={item.status === 'valid' ? 'green' : 'gray'} size="xs">{item.status}</Badge>
                                                    {item.file_name && <Badge variant="light" color="blue" size="xs">{item.file_name}</Badge>}
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