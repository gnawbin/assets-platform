'use client';

import React, { useState, useCallback, useRef, useEffect } from 'react';
import {
    ReactFlow,
    Background,
    Controls,
    MiniMap,
    type Node,
    type Edge,
    type Connection,
    type OnNodesChange,
    type OnEdgesChange,
    type OnConnect,
    applyNodeChanges,
    applyEdgeChanges,
    addEdge,
    useReactFlow,
    ReactFlowProvider,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';

import { Box, Button, Group, Text, Loader, Modal, Textarea, Stack, ActionIcon, Tooltip, Alert } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import {
    IconPlayerPlay,
    IconDeviceFloppy,
    IconDownload,
    IconUpload,
    IconPlus,
    IconAlertCircle,
} from '@tabler/icons-react';

import { nodeTypes } from './nodes';
import SkillNodePanel from './SkillNodePanel';
import NodeConfigPanel from './NodeConfigPanel';
import {
    getWorkflow,
    saveWorkflow,
    executeWorkflow,
    type WorkflowDefinition,
    type WorkflowNode,
    type WorkflowEdge,
} from '@/services/workflowService';

// ======================== 生成唯一 ID ========================

let nodeIdCounter = 0;
const generateNodeId = () => `node_${++nodeIdCounter}`;
const generateEdgeId = () => `edge_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`;

// ======================== 转换函数 ========================

/** WorkflowNode → ReactFlow Node */
const toFlowNode = (n: WorkflowNode): Node => ({
    id: n.id,
    type: n.type,
    position: n.position,
    data: { label: n.label, ...(n.config || {}) } as Record<string, unknown>,
});

/** WorkflowEdge → ReactFlow Edge */
const toFlowEdge = (e: WorkflowEdge): Edge => ({
    id: e.id,
    source: e.source,
    target: e.target,
    label: e.label as string | undefined,
    style: e.label === 'yes' ? { stroke: '#2b8a3e' } : e.label === 'no' ? { stroke: '#e03131' } : undefined,
});

/** ReactFlow Node → WorkflowNode */
const toWorkflowNode = (n: Node): WorkflowNode => ({
    id: n.id,
    type: (n.type || 'skill') as WorkflowNode['type'],
    label: (n.data as Record<string, unknown>).label as string || '',
    position: n.position,
    config: Object.fromEntries(
        Object.entries(n.data).filter(([k]) => k !== 'label')
    ),
});

/** ReactFlow Edge → WorkflowEdge */
const toWorkflowEdge = (e: Edge): WorkflowEdge => ({
    id: e.id,
    source: e.source,
    target: e.target,
    label: e.label as unknown as string | undefined,
});

// ======================== 编辑器内部组件 ========================

interface EditorContentProps {
    workflowId?: string;
    onSaved?: (id: string) => void;
}

const EditorContent: React.FC<EditorContentProps> = ({ workflowId, onSaved }) => {
    const reactFlowInstance = useReactFlow();
    const [nodes, setNodes] = useState<Node[]>([]);
    const [edges, setEdges] = useState<Edge[]>([]);
    const [selectedNode, setSelectedNode] = useState<Node | null>(null);
    const [loading, setLoading] = useState(true);
    const [saving, setSaving] = useState(false);
    const [lastSaved, setLastSaved] = useState<Date | null>(null);
    const [error, setError] = useState<string | null>(null);

    // 执行对话框
    const [opened, { open, close }] = useDisclosure(false);
    const [execInput, setExecInput] = useState('');

    const workflowNameRef = useRef<string>('未命名工作流');
    const workflowDescriptionRef = useRef<string>('');

    // 加载已有工作流
    useEffect(() => {
        if (!workflowId) {
            setLoading(false);
            return;
        }
        (async () => {
            try {
                const def = await getWorkflow(workflowId);
                if (def) {
                    workflowNameRef.current = def.name;
                    workflowDescriptionRef.current = def.description || '';
                    setNodes(def.nodes.map(toFlowNode));
                    setEdges(def.edges.map(toFlowEdge));
                }
            } catch (err) {
                setError('加载工作流失败');
            } finally {
                setLoading(false);
            }
        })();
    }, [workflowId]);

    // 节点变化
    const onNodesChange: OnNodesChange = useCallback(
        (changes) => setNodes((nds) => applyNodeChanges(changes, nds)),
        []
    );

    // 边变化
    const onEdgesChange: OnEdgesChange = useCallback(
        (changes) => setEdges((eds) => applyEdgeChanges(changes, eds)),
        []
    );

    // 连接
    const onConnect: OnConnect = useCallback(
        (connection: Connection) => {
            setEdges((eds) =>
                addEdge({ ...connection, id: generateEdgeId() }, eds)
            );
        },
        []
    );

    // 拖拽节点到画布
    const onDragOver = useCallback((event: React.DragEvent) => {
        event.preventDefault();
        event.dataTransfer.dropEffect = 'move';
    }, []);

    const onDrop = useCallback(
        (event: React.DragEvent) => {
            event.preventDefault();
            const dataStr = event.dataTransfer.getData('application/reactflow');
            if (!dataStr) return;

            try {
                const { type, label, config } = JSON.parse(dataStr);
                const position = reactFlowInstance.screenToFlowPosition({
                    x: event.clientX,
                    y: event.clientY,
                });

                const newNode: Node = {
                    id: generateNodeId(),
                    type,
                    position,
                    data: { label, ...(config || {}) },
                };

                setNodes((nds) => nds.concat(newNode));
            } catch {
                // ignore
            }
        },
        [reactFlowInstance]
    );

    // 选中节点
    const onNodeClick = useCallback((_: React.MouseEvent, node: Node) => {
        setSelectedNode(node);
    }, []);

    const onPaneClick = useCallback(() => {
        setSelectedNode(null);
    }, []);

    // 更新节点数据
    const handleUpdateNode = useCallback((nodeId: string, data: Record<string, unknown>) => {
        setNodes((nds) =>
            nds.map((n) => (n.id === nodeId ? { ...n, data } : n))
        );
        // 同步更新当前选中的节点
        setSelectedNode((prev) =>
            prev && prev.id === nodeId ? { ...prev, data } : prev
        );
    }, []);

    // 导出工作流 JSON
    const exportWorkflow = useCallback(() => {
        const def: WorkflowDefinition = {
            name: workflowNameRef.current,
            description: workflowDescriptionRef.current,
            version: '1.0.0',
            nodes: nodes.map(toWorkflowNode),
            edges: edges.map(toWorkflowEdge),
        };
        const blob = new Blob([JSON.stringify(def, null, 2)], { type: 'application/json' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = `${def.name.replace(/\s+/g, '_')}.json`;
        a.click();
        URL.revokeObjectURL(url);
    }, [nodes, edges]);

    // 导入工作流 JSON
    const importWorkflow = useCallback(() => {
        const input = document.createElement('input');
        input.type = 'file';
        input.accept = '.json';
        input.onchange = async (e) => {
            const file = (e.target as HTMLInputElement).files?.[0];
            if (!file) return;
            try {
                const text = await file.text();
                const def: WorkflowDefinition = JSON.parse(text);
                workflowNameRef.current = def.name;
                workflowDescriptionRef.current = def.description || '';
                setNodes(def.nodes.map(toFlowNode));
                setEdges(def.edges.map(toFlowEdge));
            } catch {
                setError('导入失败：JSON 格式错误');
            }
        };
        input.click();
    }, []);

    // 保存工作流
    const handleSave = useCallback(async () => {
        setSaving(true);
        setError(null);
        try {
            const def: WorkflowDefinition = {
                name: workflowNameRef.current,
                description: workflowDescriptionRef.current,
                version: '1.0.0',
                nodes: nodes.map(toWorkflowNode),
                edges: edges.map(toWorkflowEdge),
            };
            const result = await saveWorkflow({
                id: workflowId,
                name: def.name,
                description: def.description,
                definition: def,
            });
            setLastSaved(new Date());
            onSaved?.(result.id);
        } catch (err) {
            setError('保存失败');
        } finally {
            setSaving(false);
        }
    }, [nodes, edges, workflowId, onSaved]);

    // 执行工作流
    const handleExecute = useCallback(async () => {
        if (!workflowId) {
            // 先保存
            await handleSave();
        }
        try {
            const result = await executeWorkflow({
                workflowId: workflowId || 'new',
                inputData: execInput ? { text: execInput } : undefined,
            });
            close();
            // TODO: 跳转到执行详情页
        } catch (err) {
            setError('执行失败');
        }
    }, [workflowId, execInput, handleSave, close]);

    if (loading) {
        return (
            <Box style={{ display: 'flex', justifyContent: 'center', alignItems: 'center', height: '100%' }}>
                <Loader />
            </Box>
        );
    }

    return (
        <Box style={{ display: 'flex', height: '100%', flexDirection: 'column' }}>
            {/* 顶部工具栏 */}
            <Group p="sm" gap="xs" style={{ borderBottom: '1px solid var(--mantine-color-gray-3)', flexShrink: 0 }}>
                <Button
                    size="compact-sm"
                    leftSection={<IconDeviceFloppy size={14} />}
                    onClick={handleSave}
                    loading={saving}
                >
                    保存
                </Button>
                <Button
                    size="compact-sm"
                    leftSection={<IconPlayerPlay size={14} />}
                    onClick={open}
                    color="green"
                >
                    执行
                </Button>
                <Button size="compact-sm" variant="light" leftSection={<IconDownload size={14} />} onClick={exportWorkflow}>
                    导出
                </Button>
                <Button size="compact-sm" variant="light" leftSection={<IconUpload size={14} />} onClick={importWorkflow}>
                    导入
                </Button>

                <Text size="xs" c="dimmed" ml="auto">
                    {lastSaved ? `已保存: ${lastSaved.toLocaleTimeString()}` : '未保存'}
                </Text>
            </Group>

            {error && (
                <Alert icon={<IconAlertCircle size={14} />} color="red" variant="light" p="xs" withCloseButton onClose={() => setError(null)}>
                    {error}
                </Alert>
            )}

            {/* 三栏布局 */}
            <Box style={{ display: 'flex', flex: 1, overflow: 'hidden' }}>
                {/* 左侧节点面板 */}
                <SkillNodePanel />

                {/* 中央画布 */}
                <Box style={{ flex: 1, position: 'relative' }}>
                    <ReactFlow
                        nodes={nodes}
                        edges={edges}
                        onNodesChange={onNodesChange}
                        onEdgesChange={onEdgesChange}
                        onConnect={onConnect}
                        onDrop={onDrop}
                        onDragOver={onDragOver}
                        onNodeClick={onNodeClick}
                        onPaneClick={onPaneClick}
                        nodeTypes={nodeTypes}
                        fitView
                        deleteKeyCode="Delete"
                        snapToGrid
                        snapGrid={[20, 20]}
                    >
                        <Background />
                        <Controls />
                        <MiniMap
                            nodeStrokeWidth={3}
                            style={{ borderRadius: 8 }}
                            pannable
                            zoomable
                        />
                    </ReactFlow>
                </Box>

                {/* 右侧配置面板 */}
                <NodeConfigPanel
                    selectedNode={selectedNode}
                    onUpdateNode={handleUpdateNode}
                />
            </Box>

            {/* 执行对话框 */}
            <Modal opened={opened} onClose={close} title="执行工作流" size="md">
                <Stack gap="md">
                    <Text size="sm" c="dimmed">
                        输入工作流的输入数据（可选）
                    </Text>
                    <Textarea
                        label="输入数据 (JSON)"
                        placeholder='{"text": "要处理的文本", "file_path": "/path/to/file.pdf"}'
                        minRows={4}
                        value={execInput}
                        onChange={(e) => setExecInput(e.target.value)}
                    />
                    <Group justify="flex-end">
                        <Button variant="default" onClick={close}>
                            取消
                        </Button>
                        <Button onClick={handleExecute} leftSection={<IconPlayerPlay size={14} />}>
                            开始执行
                        </Button>
                    </Group>
                </Stack>
            </Modal>
        </Box>
    );
};

// ======================== 外层组件（包裹 ReactFlowProvider） ========================

interface WorkflowEditorProps {
    workflowId?: string;
    onSaved?: (id: string) => void;
}

const WorkflowEditor: React.FC<WorkflowEditorProps> = (props) => {
    return (
        <ReactFlowProvider>
            <EditorContent {...props} />
        </ReactFlowProvider>
    );
};

export default WorkflowEditor;