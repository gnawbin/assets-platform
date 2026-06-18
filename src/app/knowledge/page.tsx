'use client';

import React, { useEffect, useState, useCallback } from 'react';
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
    type KnowledgeTree,
    type AssetKnowledge,
} from '@/services/knowledgeService';

// ======================== 图标组件 ========================

const FolderIcon = () => (
    <svg className="w-4 h-4 text-yellow-500" fill="currentColor" viewBox="0 0 20 20">
        <path d="M2 6a2 2 0 012-2h5l2 2h5a2 2 0 012 2v6a2 2 0 01-2 2H4a2 2 0 01-2-2V6z" />
    </svg>
);

const DocumentIcon = () => (
    <svg className="w-4 h-4 text-blue-500" fill="currentColor" viewBox="0 0 20 20">
        <path d="M9 2a2 2 0 00-2 2v8a2 2 0 002 2h6a2 2 0 002-2V6.414A2 2 0 0016.414 5L14 2.586A2 2 0 0012.586 2H9z" />
    </svg>
);

const PlusIcon = () => (
    <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 4v16m8-8H4" />
    </svg>
);

const EditIcon = () => (
    <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
    </svg>
);

const DeleteIcon = () => (
    <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
    </svg>
);

const ChevronRight = () => (
    <svg className="w-3.5 h-3.5" fill="currentColor" viewBox="0 0 20 20">
        <path fillRule="evenodd" d="M7.293 14.707a1 1 0 010-1.414L10.586 10 7.293 6.707a1 1 0 011.414-1.414l4 4a1 1 0 010 1.414l-4 4a1 1 0 01-1.414 0z" clipRule="evenodd" />
    </svg>
);

const ChevronDown = () => (
    <svg className="w-3.5 h-3.5" fill="currentColor" viewBox="0 0 20 20">
        <path fillRule="evenodd" d="M5.293 7.293a1 1 0 011.414 0L10 10.586l3.293-3.293a1 1 0 111.414 1.414l-4 4a1 1 0 01-1.414 0l-4-4a1 1 0 010-1.414z" clipRule="evenodd" />
    </svg>
);

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
        <div className="select-none">
            <div
                className={`flex items-center gap-1 px-2 py-1.5 cursor-pointer rounded-md text-sm transition-colors ${isSelected
                        ? 'bg-blue-100 text-blue-700'
                        : 'hover:bg-gray-100 text-gray-700'
                    }`}
                onClick={() => onSelect(node.id)}
            >
                {/* 展开/折叠按钮 */}
                <span
                    className={`flex-shrink-0 w-4 h-4 flex items-center justify-center transition-transform ${hasChildren ? 'opacity-100' : 'opacity-0'
                        }`}
                    onClick={hasChildren ? toggleExpand : undefined}
                >
                    {expanded ? <ChevronDown /> : <ChevronRight />}
                </span>

                {/* 图标 */}
                <span className="flex-shrink-0">
                    {node.node_type === 'folder' ? <FolderIcon /> : <DocumentIcon />}
                </span>

                {/* 标题 */}
                <span className="flex-1 truncate">{node.title}</span>

                {/* 操作按钮 */}
                <div className="flex gap-0.5 opacity-0 group-hover:opacity-100">
                    <button
                        className="p-0.5 hover:text-green-600"
                        title="新增子节点"
                        onClick={(e) => {
                            e.stopPropagation();
                            onAddChild(node.id);
                        }}
                    >
                        <PlusIcon />
                    </button>
                    <button
                        className="p-0.5 hover:text-blue-600"
                        title="编辑"
                        onClick={(e) => {
                            e.stopPropagation();
                            onEdit(node);
                        }}
                    >
                        <EditIcon />
                    </button>
                    <button
                        className="p-0.5 hover:text-red-600"
                        title="删除"
                        onClick={(e) => {
                            e.stopPropagation();
                            onDelete(node.id);
                        }}
                    >
                        <DeleteIcon />
                    </button>
                </div>
            </div>

            {/* 子节点 */}
            {hasChildren && expanded && (
                <div className="ml-4 border-l border-gray-200 pl-1">
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
                </div>
            )}
        </div>
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
        <div className="h-full flex">
            {/* 左侧：知识树 */}
            <div className="w-72 flex-shrink-0 border-r border-gray-200 bg-white flex flex-col">
                <div className="flex items-center justify-between px-4 py-3 border-b border-gray-100">
                    <h2 className="text-sm font-semibold text-gray-700">知识库</h2>
                    <button
                        className="p-1 text-gray-400 hover:text-blue-600 hover:bg-blue-50 rounded"
                        title="新增根节点"
                        onClick={() => {
                            setParentIdForNew(null);
                            setEditingNode(null);
                            setNodeForm({ title: '', node_type: 'folder', icon: '' });
                            setShowNodeDialog(true);
                        }}
                    >
                        <PlusIcon />
                    </button>
                </div>

                <div className="flex-1 overflow-y-auto p-2">
                    {loading ? (
                        <div className="flex items-center justify-center h-20 text-sm text-gray-400">
                            加载中...
                        </div>
                    ) : error ? (
                        <div className="text-sm text-red-500 p-2">{error}</div>
                    ) : tree.length === 0 ? (
                        <div className="text-sm text-gray-400 p-2 text-center">
                            暂无知识节点
                        </div>
                    ) : (
                        tree.map((node) => (
                            <TreeNode
                                key={node.id}
                                node={node}
                                selectedId={selectedNodeId}
                                onSelect={handleSelectNode}
                                onAddChild={handleAddChild}
                                onEdit={handleEditNode}
                                onDelete={handleDeleteNode}
                            />
                        ))
                    )}
                </div>
            </div>

            {/* 右侧：知识条目列表 / 详情 */}
            <div className="flex-1 flex flex-col bg-gray-50">
                {selectedKnowledge ? (
                    // 知识条目详情
                    <div className="flex-1 flex flex-col">
                        <div className="flex items-center justify-between px-6 py-3 bg-white border-b border-gray-200">
                            <button
                                className="text-sm text-blue-600 hover:text-blue-800"
                                onClick={() => setSelectedKnowledge(null)}
                            >
                                ← 返回列表
                            </button>
                            <div className="flex gap-2">
                                <button
                                    className="px-3 py-1 text-sm text-blue-600 hover:bg-blue-50 rounded"
                                    onClick={() => handleEditKnowledge(selectedKnowledge)}
                                >
                                    编辑
                                </button>
                                <button
                                    className="px-3 py-1 text-sm text-red-600 hover:bg-red-50 rounded"
                                    onClick={() => handleDeleteKnowledge(selectedKnowledge.id)}
                                >
                                    删除
                                </button>
                            </div>
                        </div>
                        <div className="flex-1 overflow-y-auto p-6">
                            <h1 className="text-xl font-semibold text-gray-800 mb-4">
                                {selectedKnowledge.title}
                            </h1>
                            <div className="flex gap-4 mb-6 text-xs text-gray-500">
                                <span>类型：{selectedKnowledge.knowledge_type}</span>
                                <span>来源：{selectedKnowledge.doc_source}</span>
                                <span>权限：{selectedKnowledge.permission_level}</span>
                            </div>
                            <div className="prose prose-sm max-w-none bg-white rounded-lg p-4 border border-gray-200">
                                {selectedKnowledge.content.split('\n').map((line, i) => (
                                    <p key={i} className="mb-2">
                                        {line}
                                    </p>
                                ))}
                            </div>
                        </div>
                    </div>
                ) : (
                    // 知识条目列表
                    <div className="flex-1 flex flex-col">
                        <div className="flex items-center justify-between px-6 py-3 bg-white border-b border-gray-200">
                            <h2 className="text-sm font-semibold text-gray-700">
                                {selectedNodeId ? '知识条目' : '全部知识条目'}
                            </h2>
                            <button
                                className="flex items-center gap-1 px-3 py-1.5 text-sm text-white bg-blue-600 hover:bg-blue-700 rounded-md"
                                onClick={handleAddKnowledge}
                            >
                                <PlusIcon />
                                新增
                            </button>
                        </div>

                        <div className="flex-1 overflow-y-auto p-4">
                            {knowledgeList.length === 0 ? (
                                <div className="flex items-center justify-center h-40 text-sm text-gray-400">
                                    暂无知识条目
                                </div>
                            ) : (
                                <div className="grid gap-3">
                                    {knowledgeList.map((item) => (
                                        <div
                                            key={item.id}
                                            className="bg-white rounded-lg border border-gray-200 p-4 cursor-pointer hover:shadow-sm transition-shadow"
                                            onClick={() => handleViewKnowledge(item.id)}
                                        >
                                            <div className="flex items-start justify-between">
                                                <h3 className="text-sm font-medium text-gray-800">
                                                    {item.title}
                                                </h3>
                                                <div className="flex gap-1">
                                                    <button
                                                        className="p-1 text-gray-400 hover:text-blue-600"
                                                        title="编辑"
                                                        onClick={(e) => {
                                                            e.stopPropagation();
                                                            handleEditKnowledge(item);
                                                        }}
                                                    >
                                                        <EditIcon />
                                                    </button>
                                                    <button
                                                        className="p-1 text-gray-400 hover:text-red-600"
                                                        title="删除"
                                                        onClick={(e) => {
                                                            e.stopPropagation();
                                                            handleDeleteKnowledge(item.id);
                                                        }}
                                                    >
                                                        <DeleteIcon />
                                                    </button>
                                                </div>
                                            </div>
                                            <p className="mt-1 text-xs text-gray-500 line-clamp-2">
                                                {item.content}
                                            </p>
                                            <div className="mt-2 flex gap-3 text-xs text-gray-400">
                                                <span>类型：{item.knowledge_type}</span>
                                                <span>权限：{item.permission_level}</span>
                                            </div>
                                        </div>
                                    ))}
                                </div>
                            )}
                        </div>
                    </div>
                )}
            </div>

            {/* ======================== 节点对话框 ======================== */}
            {showNodeDialog && (
                <div className="fixed inset-0 bg-black/30 flex items-center justify-center z-50">
                    <div className="bg-white rounded-lg shadow-xl w-96 p-6">
                        <h3 className="text-base font-semibold text-gray-800 mb-4">
                            {editingNode ? '编辑节点' : '新增节点'}
                        </h3>
                        <div className="space-y-3">
                            <div>
                                <label className="block text-xs text-gray-500 mb-1">节点类型</label>
                                <select
                                    className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm"
                                    value={nodeForm.node_type}
                                    onChange={(e) =>
                                        setNodeForm({ ...nodeForm, node_type: e.target.value })
                                    }
                                    disabled={!!editingNode}
                                >
                                    <option value="folder">文件夹</option>
                                    <option value="document">文档</option>
                                </select>
                            </div>
                            <div>
                                <label className="block text-xs text-gray-500 mb-1">标题</label>
                                <input
                                    className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm"
                                    value={nodeForm.title}
                                    onChange={(e) =>
                                        setNodeForm({ ...nodeForm, title: e.target.value })
                                    }
                                    placeholder="请输入节点标题"
                                />
                            </div>
                            <div>
                                <label className="block text-xs text-gray-500 mb-1">图标（可选）</label>
                                <input
                                    className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm"
                                    value={nodeForm.icon}
                                    onChange={(e) =>
                                        setNodeForm({ ...nodeForm, icon: e.target.value })
                                    }
                                    placeholder="图标名称"
                                />
                            </div>
                        </div>
                        <div className="flex justify-end gap-2 mt-6">
                            <button
                                className="px-4 py-2 text-sm text-gray-600 hover:bg-gray-100 rounded-md"
                                onClick={() => setShowNodeDialog(false)}
                            >
                                取消
                            </button>
                            <button
                                className="px-4 py-2 text-sm text-white bg-blue-600 hover:bg-blue-700 rounded-md"
                                onClick={handleSaveNode}
                            >
                                保存
                            </button>
                        </div>
                    </div>
                </div>
            )}

            {/* ======================== 知识条目对话框 ======================== */}
            {showKnowledgeDialog && (
                <div className="fixed inset-0 bg-black/30 flex items-center justify-center z-50">
                    <div className="bg-white rounded-lg shadow-xl w-[600px] p-6">
                        <h3 className="text-base font-semibold text-gray-800 mb-4">
                            {editingKnowledge ? '编辑知识条目' : '新增知识条目'}
                        </h3>
                        <div className="space-y-3">
                            <div>
                                <label className="block text-xs text-gray-500 mb-1">标题</label>
                                <input
                                    className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm"
                                    value={knowledgeForm.title}
                                    onChange={(e) =>
                                        setKnowledgeForm({
                                            ...knowledgeForm,
                                            title: e.target.value,
                                        })
                                    }
                                    placeholder="请输入知识条目标题"
                                />
                            </div>
                            <div className="flex gap-3">
                                <div className="flex-1">
                                    <label className="block text-xs text-gray-500 mb-1">知识类型</label>
                                    <select
                                        className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm"
                                        value={knowledgeForm.knowledge_type}
                                        onChange={(e) =>
                                            setKnowledgeForm({
                                                ...knowledgeForm,
                                                knowledge_type: e.target.value,
                                            })
                                        }
                                    >
                                        <option value="basic">基础</option>
                                        <option value="contract">合同</option>
                                        <option value="hardware">硬件</option>
                                        <option value="intangible">无形资产</option>
                                    </select>
                                </div>
                                <div className="flex-1">
                                    <label className="block text-xs text-gray-500 mb-1">权限等级</label>
                                    <select
                                        className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm"
                                        value={knowledgeForm.permission_level}
                                        onChange={(e) =>
                                            setKnowledgeForm({
                                                ...knowledgeForm,
                                                permission_level: e.target.value,
                                            })
                                        }
                                    >
                                        <option value="public">公开</option>
                                        <option value="internal">内部</option>
                                        <option value="secret">保密</option>
                                    </select>
                                </div>
                            </div>
                            <div>
                                <label className="block text-xs text-gray-500 mb-1">内容</label>
                                <textarea
                                    className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm h-40"
                                    value={knowledgeForm.content}
                                    onChange={(e) =>
                                        setKnowledgeForm({
                                            ...knowledgeForm,
                                            content: e.target.value,
                                        })
                                    }
                                    placeholder="请输入知识条目内容"
                                />
                            </div>
                        </div>
                        <div className="flex justify-end gap-2 mt-6">
                            <button
                                className="px-4 py-2 text-sm text-gray-600 hover:bg-gray-100 rounded-md"
                                onClick={() => setShowKnowledgeDialog(false)}
                            >
                                取消
                            </button>
                            <button
                                className="px-4 py-2 text-sm text-white bg-blue-600 hover:bg-blue-700 rounded-md"
                                onClick={handleSaveKnowledge}
                            >
                                保存
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
}
