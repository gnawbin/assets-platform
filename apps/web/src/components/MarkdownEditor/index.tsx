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
} from '@mantine/core';
import {
    IconArrowLeft,
    IconDeviceFloppy,
    IconEye,
    IconCode,
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
}) => {
    const router = useRouter();
    const [activeTab, setActiveTab] = useState<string | null>('edit');

    const handleSave = useCallback(() => {
        onSave?.();
    }, [onSave]);

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