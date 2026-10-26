'use client';

import React, { useCallback, useEffect, useRef, useState } from 'react';
import dynamic from 'next/dynamic';
import {
    ActionIcon,
    Alert,
    Badge,
    Box,
    Button,
    Center,
    Group,
    Loader,
    Menu,
    Modal,
    Stack,
    Text,
    TextInput,
    Textarea,
    Tooltip,
} from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import {
    IconAlertCircle,
    IconArrowBackUp,
    IconArrowForwardUp,
    IconDeviceFloppy,
    IconDownload,
    IconMaximize,
    IconPlayerPlay,
    IconUpload,
    IconZoomIn,
    IconZoomOut,
} from '@tabler/icons-react';

import NodePalettePanel from './NodePalettePanel';
import NodeConfigPanel from './NodeConfigPanel';
import type {
    BpmnCanvasApi,
    ElementPatch,
    EngineFields,
    SelectedBpmnElement,
    ValidationIssue,
} from './bpmn/types';
import { BLANK_BPMN_XML } from './bpmn/template/blankDiagram';
import { collectNodeTypes } from './bpmn/mapping/helpers';
import { definitionToBpmn } from './bpmn/mapping/definitionToBpmn';
import {
    executeWorkflow,
    getExecution,
    getWorkflow,
    saveWorkflow,
    type WorkflowDefinition,
} from '@/services/workflowService';

const BpmnCanvas = dynamic(() => import('./bpmn/BpmnCanvas'), {
    ssr: false,
    loading: () => (
        <Center h="100%">
            <Loader size="sm" />
            <Text size="sm" c="dimmed" ml="sm">
                加载 BPMN 引擎...
            </Text>
        </Center>
    ),
});

// ======================== 工具函数 ========================

function safeFileName(name: string): string {
    return (name.trim() || 'workflow').replace(/[\\/:*?"<>|\s]+/g, '_');
}

function downloadFile(fileName: string, content: string, mime: string): void {
    const blob = new Blob([content], { type: mime });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = fileName;
    anchor.click();
    URL.revokeObjectURL(url);
}

function parseExecutionInput(raw: string): Record<string, unknown> | undefined {
    const text = raw.trim();
    if (!text) return undefined;
    try {
        const parsed: unknown = JSON.parse(text);
        if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
            return parsed as Record<string, unknown>;
        }
    } catch {
        // 非 JSON 输入按纯文本处理
    }
    return { text: raw };
}

function readProcessName(xml: string): string {
    try {
        const doc = new DOMParser().parseFromString(xml, 'application/xml');
        const processElement = doc.getElementsByTagNameNS('*', 'process')[0];
        return processElement?.getAttribute('name') || '';
    } catch {
        return '';
    }
}

interface WorkflowEditorProps {
    workflowId?: string;
    onSaved?: (id: string) => void;
}


export default function WorkflowEditor({ workflowId, onSaved }: WorkflowEditorProps) {
    const apiRef = useRef<BpmnCanvasApi | null>(null);
    const [api, setApi] = useState<BpmnCanvasApi | null>(null);
    const [xml, setXml] = useState<string | null>(null);
    const [loading, setLoading] = useState(true);
    const [saving, setSaving] = useState(false);
    const [executing, setExecuting] = useState(false);
    const [lastSaved, setLastSaved] = useState<Date | null>(null);
    const [dirty, setDirty] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [issues, setIssues] = useState<ValidationIssue[]>([]);
    const [selected, setSelected] = useState<SelectedBpmnElement | null>(null);
    const [name, setName] = useState('新建工作流');
    const [description, setDescription] = useState('');
    const [canUndo, setCanUndo] = useState(false);
    const [canRedo, setCanRedo] = useState(false);
    const [execMessage, setExecMessage] = useState<string | null>(null);

    const [execOpened, { open: openExec, close: closeExec }] = useDisclosure(false);
    const [execInput, setExecInput] = useState('');
    const fileInputRef = useRef<HTMLInputElement>(null);

    // 加载：bpmn_xml 优先，其次旧 IR → BPMN 转换，最后空白流程
    useEffect(() => {
        let cancelled = false;
        setLoading(true);
        setError(null);
        setIssues([]);
        setSelected(null);
        setExecMessage(null);
        setXml(null);

        (async () => {
            try {
                if (!workflowId) {
                    if (!cancelled) {
                        setName('新建工作流');
                        setDescription('');
                        setXml(BLANK_BPMN_XML);
                    }
                    return;
                }

                const definition = await getWorkflow(workflowId);
                if (cancelled) return;
                if (!definition) {
                    setName('新建工作流');
                    setDescription('');
                    setXml(BLANK_BPMN_XML);
                    setError('未找到该工作流，已打开空白流程。');
                    return;
                }

                setName(definition.name || '未命名工作流');
                setDescription(definition.description || '');
                const bpmnXml = definition.bpmn_xml || (await definitionToBpmn(definition));
                if (!cancelled) setXml(bpmnXml);
            } catch (err) {
                if (cancelled) return;
                setError(err instanceof Error ? err.message : '加载工作流失败');
                setXml(BLANK_BPMN_XML);
            } finally {
                if (!cancelled) setLoading(false);
            }
        })();

        return () => {
            cancelled = true;
        };
    }, [workflowId]);

    const refreshHistory = useCallback(() => {
        setCanUndo(apiRef.current?.canUndo() ?? false);
        setCanRedo(apiRef.current?.canRedo() ?? false);
    }, []);

    const handleReady = useCallback(
        (next: BpmnCanvasApi | null) => {
            apiRef.current = next;
            setApi(next);
            refreshHistory();
        },
        [refreshHistory]
    );

    const handleDirtyChange = useCallback(
        (nextDirty: boolean) => {
            setDirty(nextDirty);
            refreshHistory();
        },
        [refreshHistory]
    );

    const handleSelectionChange = useCallback((element: SelectedBpmnElement | null) => {
        setSelected(element);
    }, []);

    const handleError = useCallback((message: string) => {
        setError(message);
    }, []);

    const handleInsert = useCallback(
        (presetId: string, extra?: { engine?: Partial<EngineFields>; name?: string }) => {
            const current = apiRef.current;
            if (!current) return;
            current.insertPreset(presetId, extra);
            setDirty(true);
            refreshHistory();
        },
        [refreshHistory]
    );

    const handleUpdate = useCallback(
        (patch: ElementPatch) => {
            const current = apiRef.current;
            if (!current) return;
            current.updateSelected(patch);
            setSelected(current.getSelected());
            setDirty(true);
            refreshHistory();
        },
        [refreshHistory]
    );

    const handleUndo = useCallback(() => {
        apiRef.current?.undo();
        setSelected(apiRef.current?.getSelected() ?? null);
        refreshHistory();
    }, [refreshHistory]);

    const handleRedo = useCallback(() => {
        apiRef.current?.redo();
        setSelected(apiRef.current?.getSelected() ?? null);
        refreshHistory();
    }, [refreshHistory]);

    const projectDefinition = useCallback(async () => {
        const current = apiRef.current;
        if (!current) throw new Error('画布尚未就绪');
        const finalName = name.trim() || '未命名工作流';
        current.updateProcessMeta({ name: finalName });
        const xmlText = await current.saveXml();
        const projection = await current.buildDefinition({
            name: finalName,
            description: description || undefined,
            version: '1.0.0',
        });
        return { xml: xmlText, definition: projection.definition, issues: projection.issues };
    }, [name, description]);

    /** 保存（IR + BPMN XML 双写）；notify=false 用于执行前静默保存 */
    const persist = useCallback(
        async (
            projection: { xml: string; definition: WorkflowDefinition; issues: ValidationIssue[] },
            notify = true
        ) => {
            const definition: WorkflowDefinition = {
                ...projection.definition,
                bpmn_xml: projection.xml,
            };
            const result = await saveWorkflow({
                id: workflowId,
                name: definition.name,
                description: definition.description,
                definition,
                bpmnXml: projection.xml,
                node_types: collectNodeTypes(definition),
                status: 'draft',
            });
            setLastSaved(new Date());
            setDirty(false);
            if (!name.trim()) setName(definition.name);
            if (notify) onSaved?.(result.id);
            return { id: result.id, definition };
        },
        [workflowId, name, onSaved]
    );

    const handleSave = useCallback(async (): Promise<string | undefined> => {
        setSaving(true);
        setError(null);
        try {
            const projection = await projectDefinition();
            setIssues(projection.issues);
            const { id } = await persist(projection, true);
            return id;
        } catch (err) {
            setError(err instanceof Error ? err.message : '保存失败');
            return undefined;
        } finally {
            setSaving(false);
        }
    }, [projectDefinition, persist]);

    const handleOpenExecute = useCallback(async () => {
        if (!apiRef.current) return;
        setError(null);
        try {
            const projection = await projectDefinition();
            setIssues(projection.issues);
            if (projection.issues.some((issue) => issue.level === 'error')) {
                setError('存在阻断执行的校验问题，请先修复（见下方列表）。');
                return;
            }
            openExec();
        } catch (err) {
            setError(err instanceof Error ? err.message : '校验失败');
        }
    }, [projectDefinition, openExec]);

    const handleExecute = useCallback(async () => {
        setExecuting(true);
        setError(null);
        try {
            const projection = await projectDefinition();
            setIssues(projection.issues);
            if (projection.issues.some((issue) => issue.level === 'error')) {
                setError('校验未通过，无法执行。');
                return;
            }

            const { id, definition } = await persist(projection, false);
            const result = await executeWorkflow({
                workflowId: id,
                definition,
                inputData: parseExecutionInput(execInput),
                title: definition.name,
            });

            closeExec();
            setExecMessage(`已提交执行：${result.executionId}（${result.status}）`);

            // 新建流程执行后跳转到持久化后的编辑器地址
            if (!workflowId) onSaved?.(id);

            try {
                const detail = await getExecution(result.executionId);
                if (detail?.node_results?.length) {
                    apiRef.current?.applyExecutionState(
                        detail.node_results.map((item) => ({
                            node_id: item.node_id,
                            status: item.status,
                        }))
                    );
                }
            } catch {
                // 后端未实现执行详情时忽略
            }
        } catch (err) {
            setError(err instanceof Error ? err.message : '执行失败');
        } finally {
            setExecuting(false);
        }
    }, [projectDefinition, persist, execInput, workflowId, onSaved, closeExec]);


    const handleNameCommit = useCallback(() => {
        const nextName = name.trim() || '未命名工作流';
        if (nextName !== name) setName(nextName);
        apiRef.current?.updateProcessMeta({ name: nextName });
    }, [name]);

    const handleExportXml = useCallback(async () => {
        try {
            const current = apiRef.current;
            if (!current) return;
            const xmlText = await current.saveXml();
            downloadFile(`${safeFileName(name)}.bpmn`, xmlText, 'application/xml');
        } catch (err) {
            setError(err instanceof Error ? err.message : '导出 BPMN 失败');
        }
    }, [name]);

    const handleExportJson = useCallback(async () => {
        try {
            const projection = await projectDefinition();
            const definition: WorkflowDefinition = {
                ...projection.definition,
                bpmn_xml: projection.xml,
            };
            downloadFile(
                `${safeFileName(name)}.workflow.json`,
                JSON.stringify(definition, null, 2),
                'application/json'
            );
        } catch (err) {
            setError(err instanceof Error ? err.message : '导出 JSON 失败');
        }
    }, [name, projectDefinition]);

    const handleImportClick = useCallback(() => {
        fileInputRef.current?.click();
    }, []);

    const handleFileChange = useCallback(async (event: React.ChangeEvent<HTMLInputElement>) => {
        const file = event.currentTarget.files?.[0];
        event.currentTarget.value = '';
        if (!file) return;

        setError(null);
        setIssues([]);
        setSelected(null);
        try {
            const text = await file.text();
            const lowerName = file.name.toLowerCase();

            if (lowerName.endsWith('.json')) {
                const parsed: unknown = JSON.parse(text);
                const definition = parsed as WorkflowDefinition;
                if (!definition || !Array.isArray(definition.nodes) || !Array.isArray(definition.edges)) {
                    throw new Error('JSON 不是有效的 WorkflowDefinition');
                }
                setName(definition.name || file.name.replace(/\.json$/i, ''));
                setDescription(definition.description || '');
                setXml(definition.bpmn_xml || (await definitionToBpmn(definition)));
            } else {
                const doc = new DOMParser().parseFromString(text, 'application/xml');
                const rootName = doc.documentElement?.nodeName || '';
                if (doc.getElementsByTagName('parsererror').length > 0 || !rootName.endsWith('definitions')) {
                    throw new Error('文件不是有效的 BPMN 2.0 XML');
                }
                setName(readProcessName(text) || lowerName.replace(/\.(bpmn|xml)$/i, ''));
                setDescription('');
                setXml(text);
            }

            setLastSaved(null);
            setDirty(true);
        } catch (err) {
            setError(err instanceof Error ? err.message : '导入失败');
        }
    }, []);

    const hasErrors = issues.some((issue) => issue.level === 'error');

    if (loading) {
        return (
            <Box
                style={{
                    display: 'flex',
                    justifyContent: 'center',
                    alignItems: 'center',
                    height: '100%',
                }}
            >
                <Loader />
            </Box>
        );
    }

    if (!xml) {
        return (
            <Center h="100%">
                <Text size="sm" c="dimmed">
                    无法初始化 BPMN 画布
                </Text>
            </Center>
        );
    }


    return (
        <Box style={{ display: 'flex', height: '100%', flexDirection: 'column' }}>
            <input
                ref={fileInputRef}
                type="file"
                accept=".bpmn,.xml,.json"
                style={{ display: 'none' }}
                onChange={handleFileChange}
            />

            {/* 顶部工具栏 */}
            <Group
                p="sm"
                gap="xs"
                wrap="wrap"
                style={{
                    borderBottom: '1px solid var(--mantine-color-gray-3)',
                    flexShrink: 0,
                }}
            >
                <TextInput
                    size="xs"
                    w={220}
                    value={name}
                    placeholder="工作流名称"
                    onChange={(event) => setName(event.currentTarget.value)}
                    onBlur={handleNameCommit}
                    onKeyDown={(event) => {
                        if (event.key === 'Enter') event.currentTarget.blur();
                    }}
                />
                <Badge size="sm" variant="light" color="grape">
                    BPMN 2.0
                </Badge>

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
                    color="green"
                    leftSection={<IconPlayerPlay size={14} />}
                    onClick={handleOpenExecute}
                >
                    执行
                </Button>

                <Tooltip label="撤销 (Ctrl+Z)">
                    <ActionIcon
                        variant="default"
                        size="lg"
                        disabled={!canUndo}
                        onClick={handleUndo}
                        aria-label="撤销"
                    >
                        <IconArrowBackUp size={16} />
                    </ActionIcon>
                </Tooltip>
                <Tooltip label="重做 (Ctrl+Shift+Z)">
                    <ActionIcon
                        variant="default"
                        size="lg"
                        disabled={!canRedo}
                        onClick={handleRedo}
                        aria-label="重做"
                    >
                        <IconArrowForwardUp size={16} />
                    </ActionIcon>
                </Tooltip>
                <Tooltip label="放大">
                    <ActionIcon
                        variant="default"
                        size="lg"
                        onClick={() => apiRef.current?.zoomIn()}
                        aria-label="放大"
                    >
                        <IconZoomIn size={16} />
                    </ActionIcon>
                </Tooltip>
                <Tooltip label="缩小">
                    <ActionIcon
                        variant="default"
                        size="lg"
                        onClick={() => apiRef.current?.zoomOut()}
                        aria-label="缩小"
                    >
                        <IconZoomOut size={16} />
                    </ActionIcon>
                </Tooltip>
                <Tooltip label="适应画布">
                    <ActionIcon
                        variant="default"
                        size="lg"
                        onClick={() => apiRef.current?.fitView()}
                        aria-label="适应画布"
                    >
                        <IconMaximize size={16} />
                    </ActionIcon>
                </Tooltip>


                <Menu position="bottom-start" withArrow>
                    <Menu.Target>
                        <Button
                            size="compact-sm"
                            variant="light"
                            leftSection={<IconDownload size={14} />}
                        >
                            导出
                        </Button>
                    </Menu.Target>
                    <Menu.Dropdown>
                        <Menu.Item onClick={handleExportXml}>导出 BPMN XML (.bpmn)</Menu.Item>
                        <Menu.Item onClick={handleExportJson}>
                            导出工作流 JSON（含 bpmn_xml）
                        </Menu.Item>
                    </Menu.Dropdown>
                </Menu>
                <Button
                    size="compact-sm"
                    variant="light"
                    leftSection={<IconUpload size={14} />}
                    onClick={handleImportClick}
                >
                    导入
                </Button>

                <Text size="xs" c="dimmed" ml="auto">
                    {dirty
                        ? '有未保存修改'
                        : lastSaved
                          ? `已保存: ${lastSaved.toLocaleTimeString()}`
                          : '未保存'}
                </Text>
            </Group>

            {error && (
                <Alert
                    icon={<IconAlertCircle size={14} />}
                    color="red"
                    variant="light"
                    p="xs"
                    withCloseButton
                    onClose={() => setError(null)}
                >
                    {error}
                </Alert>
            )}

            {execMessage && (
                <Alert
                    color="blue"
                    variant="light"
                    p="xs"
                    withCloseButton
                    onClose={() => setExecMessage(null)}
                >
                    {execMessage}
                </Alert>
            )}

            {issues.length > 0 && (
                <Box px="sm" pb="xs">
                    <Alert
                        color={hasErrors ? 'red' : 'yellow'}
                        variant="light"
                        p="xs"
                        withCloseButton
                        onClose={() => setIssues([])}
                        title={hasErrors ? '校验发现阻断问题' : '校验提示'}
                    >
                        <Stack gap={4}>
                            {issues.slice(0, 6).map((issue, index) => (
                                <Group
                                    key={`${issue.elementId || 'global'}-${index}`}
                                    gap={6}
                                    wrap="nowrap"
                                    align="flex-start"
                                >
                                    <Badge
                                        size="xs"
                                        variant="light"
                                        color={issue.level === 'error' ? 'red' : 'yellow'}
                                    >
                                        {issue.level === 'error' ? '错误' : '提示'}
                                    </Badge>
                                    <Text
                                        size="xs"
                                        style={{
                                            flex: 1,
                                            cursor: issue.elementId ? 'pointer' : 'default',
                                        }}
                                        onClick={() => {
                                            if (issue.elementId) {
                                                apiRef.current?.selectElement(issue.elementId);
                                            }
                                        }}
                                    >
                                        {issue.message}
                                    </Text>
                                </Group>
                            ))}
                            {issues.length > 6 && (
                                <Text size="xs" c="dimmed">
                                    还有 {issues.length - 6} 条问题，保存/执行时会再次校验。
                                </Text>
                            )}
                        </Stack>
                    </Alert>
                </Box>
            )}


            {/* 三栏布局：业务模板 / BPMN 画布 / 属性面板 */}
            <Box style={{ display: 'flex', flex: 1, overflow: 'hidden', minHeight: 0 }}>
                <Box style={{ width: 260, flexShrink: 0, height: '100%' }}>
                    <NodePalettePanel onInsert={handleInsert} disabled={!api} />
                </Box>

                <Box
                    style={{
                        flex: 1,
                        minWidth: 0,
                        display: 'flex',
                        flexDirection: 'column',
                        padding: 8,
                        overflow: 'hidden',
                    }}
                >
                    <BpmnCanvas
                        xml={xml}
                        onReady={handleReady}
                        onSelectionChange={handleSelectionChange}
                        onDirtyChange={handleDirtyChange}
                        onError={handleError}
                    />
                </Box>

                <Box style={{ flexShrink: 0, height: '100%' }}>
                    <NodeConfigPanel element={selected} onUpdate={handleUpdate} />
                </Box>
            </Box>

            {/* 执行对话框 */}
            <Modal opened={execOpened} onClose={closeExec} title="执行工作流" size="md">
                <Stack gap="md">
                    <Text size="sm" c="dimmed">
                        执行前会自动保存当前 BPMN XML 与执行 IR。
                    </Text>
                    {error && (
                        <Alert
                            color="red"
                            variant="light"
                            p="xs"
                            icon={<IconAlertCircle size={14} />}
                        >
                            {error}
                        </Alert>
                    )}
                    <Textarea
                        label="输入数据 (JSON)"
                        placeholder={'{"text": "要处理的文本", "file_path": "/path/to/file.pdf"}'}
                        minRows={4}
                        value={execInput}
                        onChange={(event) => setExecInput(event.currentTarget.value)}
                    />
                    <Group justify="flex-end">
                        <Button variant="default" onClick={closeExec}>
                            取消
                        </Button>
                        <Button
                            onClick={handleExecute}
                            loading={executing}
                            leftSection={<IconPlayerPlay size={14} />}
                        >
                            开始执行
                        </Button>
                    </Group>
                </Stack>
            </Modal>
        </Box>
    );
}
