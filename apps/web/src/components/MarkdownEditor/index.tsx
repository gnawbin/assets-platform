'use client';
import React, { useState, useCallback, useRef } from 'react';
import dynamic from 'next/dynamic';
import {
    Stack,
    Group,
    Button,
    ActionIcon,
    Paper,
    Tabs,
    Text,
    Center,
    Loader,
    Progress,
} from '@mantine/core';
import {
    IconArrowLeft,
    IconDeviceFloppy,
    IconEye,
    IconCode,
    IconPlayerPause,
    IconPlayerPlay,
    IconX,
    IconRefresh,
} from '@tabler/icons-react';
import { useRouter } from 'next/navigation';
import ReactMarkdown from 'react-markdown';
import MetaPanel from './MetaPanel';
import FileAttachPanel from './FileAttachPanel';
import { type MarkdownEditorProps, type OkfType } from './types';

// 动态导入 MDXEditor，禁用 SSR（Next.js 兼容）
const MDXEditorWrapper = dynamic(() => import('@/components/MarkdownEditor/MDXEditorWrapper'), {
    ssr: false,
    loading: () => (
        <Center py="xl">
            <Loader size="sm" />
            <Text size="sm" c="dimmed" ml="sm">加载编辑器...</Text>
        </Center>
    ),
}) as React.ComponentType<{ content: string; onChange?: (content: string) => void }>;

/** 格式化上传速度 */
function formatSpeed(bytesPerSec: number): string {
    if (!bytesPerSec || bytesPerSec <= 0) return '';
    const k = 1024;
    const sizes = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
    const i = Math.min(Math.floor(Math.log(bytesPerSec) / Math.log(k)), sizes.length - 1);
    return parseFloat((bytesPerSec / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

const MarkdownEditor: React.FC<MarkdownEditorProps> = ({
    content = '',
    onChange,
    title,
    onTitleChange,
    okfType,
    onOkfTypeChange,
    summary,
    onSummaryChange,
    source,
    onSourceChange,
    status,
    onStatusChange,
    fileUrl,
    fileName,
    fileSize,
    onFileUpload,
    tags,
    onTagsChange,
    onSave,
    saving,
    // 文件上传完成/错误回调
    onUploadComplete,
    onUploadError,
    // 文件上传状态（页面自管理上传）
    uploadStatus,
    uploadProgress,
    uploadSpeed,
    uploadError,
    onFileSelect,
    onPause,
    onResume,
    onCancel,
    onRetry,
}) => {
    const router = useRouter();
    const [activeTab, setActiveTab] = useState<string | null>('edit');
    const fileInputRef = useRef<HTMLInputElement>(null);

    const handleSave = useCallback(() => {
        onSave?.();
    }, [onSave]);

    const handleFileSelectClick = () => {
        fileInputRef.current?.click();
    };

    const handleFileInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
        const file = e.currentTarget.files?.[0];
        if (file) onFileSelect?.(file);
        e.currentTarget.value = '';
    };

    return (
        <Stack gap="md" h="100%">
            {/* 顶部工具栏 */}
            <Paper p="sm" withBorder>
                <Group justify="space-between">
                    <Group gap="xs">
                        <ActionIcon variant="subtle" onClick={() => router.back()}>
                            <IconArrowLeft size={18} />
                        </ActionIcon>
                    </Group>
                    <Group gap="xs">
                        <Tabs value={activeTab} onChange={setActiveTab}>
                            <Tabs.List>
                                <Tabs.Tab value="edit" leftSection={<IconCode size={14} />}>
                                    编辑
                                </Tabs.Tab>
                                <Tabs.Tab value="preview" leftSection={<IconEye size={14} />}>
                                    预览
                                </Tabs.Tab>
                            </Tabs.List>
                        </Tabs>
                        <Button
                            size="sm"
                            leftSection={<IconDeviceFloppy size={16} />}
                            onClick={handleSave}
                            loading={saving}
                        >
                            保存
                        </Button>
                    </Group>
                </Group>
            </Paper>

            {/* 元数据面板 */}
            <MetaPanel
                title={title}
                onTitleChange={onTitleChange}
                okfType={okfType}
                onOkfTypeChange={onOkfTypeChange}
                summary={summary}
                onSummaryChange={onSummaryChange}
                source={source}
                onSourceChange={onSourceChange}
                status={status}
                onStatusChange={onStatusChange}
                tags={tags}
                onTagsChange={onTagsChange}
            />

            {/* 编辑 / 预览区域 */}
            <Paper p="sm" withBorder style={{ flex: 1, minHeight: 400 }}>
                {activeTab === 'edit' ? (
                    <MDXEditorWrapper
                        content={content}
                        onChange={onChange}
                    />
                ) : (
                    <Paper p="md" style={{ minHeight: 300 }}>
                        <div className="prose prose-sm max-w-none">
                            <ReactMarkdown>
                                {content || ''}
                            </ReactMarkdown>
                        </div>
                    </Paper>
                )}
            </Paper>

            {/* 上传进度面板（页面自管理上传时展示） */}
            {(uploadStatus && uploadStatus !== 'idle') || onFileSelect ? (
                <Paper p="sm" withBorder>
                    <Group justify="space-between" mb={4}>
                        <Text size="sm" fw={500}>
                            {uploadStatus === 'uploading' ? '文件上传中...'
                                : uploadStatus === 'paused' ? '上传已暂停'
                                : uploadStatus === 'completed' ? '上传完成'
                                : uploadStatus === 'error' ? '上传失败'
                                : '上传文件'}
                        </Text>
                        {uploadStatus === 'uploading' && uploadSpeed ? (
                            <Text size="xs" c="dimmed">{formatSpeed(uploadSpeed)}</Text>
                        ) : null}
                    </Group>
                    {uploadStatus === 'uploading' || uploadStatus === 'paused' ? (
                        <Progress value={uploadProgress ?? 0} size="sm" />
                    ) : null}
                    {uploadError ? (
                        <Text size="xs" c="red" mt={4}>{uploadError}</Text>
                    ) : null}
                    <Group gap="xs" mt="xs">
                        {onFileSelect ? (
                            <>
                                <input
                                    ref={fileInputRef}
                                    type="file"
                                    style={{ display: 'none' }}
                                    onChange={handleFileInputChange}
                                />
                                <Button size="xs" variant="default" onClick={handleFileSelectClick}>
                                    选择文件
                                </Button>
                            </>
                        ) : null}
                        {uploadStatus === 'uploading' && onPause ? (
                            <Button size="xs" variant="light" onClick={onPause}>暂停</Button>
                        ) : null}
                        {uploadStatus === 'paused' && onResume ? (
                            <Button size="xs" variant="light" onClick={onResume}>继续</Button>
                        ) : null}
                        {(uploadStatus === 'uploading' || uploadStatus === 'paused') && onCancel ? (
                            <Button size="xs" variant="default" color="red" onClick={onCancel}>取消</Button>
                        ) : null}
                        {uploadStatus === 'error' && onRetry ? (
                            <Button size="xs" variant="light" onClick={onRetry}>重试</Button>
                        ) : null}
                    </Group>
                </Paper>
            ) : null}

            {/* 文件附件面板（自管理上传逻辑） */}
            <FileAttachPanel
                fileUrl={fileUrl}
                fileName={fileName}
                fileSize={fileSize}
                onUploadComplete={onUploadComplete}
                onUploadError={onUploadError}
            />
        </Stack>
    );
};

export default MarkdownEditor;