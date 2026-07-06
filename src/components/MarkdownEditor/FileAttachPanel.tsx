'use client';
import React, { useRef } from 'react';
import {
    Group,
    Text,
    Button,
    ActionIcon,
    Paper,
    FileButton,
    Progress,
    Box,
    Stack,
    Tooltip,
} from '@mantine/core';
import {
    IconUpload,
    IconPaperclip,
    IconX,
    IconExternalLink,
    IconPlayerPause,
    IconPlayerPlay,
    IconRefresh,
    IconFile,
    IconFileTypePdf,
    IconFileTypeDoc,
    IconFileTypeXls,
    IconFileTypeZip,
    IconPhoto,
} from '@tabler/icons-react';
import { useChunkedUpload } from '@/hooks/useChunkedUpload';

/**
 * 上传状态
 */
export type AttachUploadStatus =
    | 'idle'
    | 'uploading'
    | 'paused'
    | 'completed'
    | 'error';

export interface FileAttachPanelProps {
    fileUrl?: string;
    fileName?: string;
    fileSize?: number;

    /** 上传完成回调（父组件可在此拿到 fileUrl 绑定到业务实体） */
    onUploadComplete?: (result: { fileUrl: string; fileName: string; fileSize: number }) => void;
    /** 上传错误回调 */
    onUploadError?: (err: string) => void;
}

const formatFileSize = (bytes: number): string => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
};

function getFileExt(filename: string): string {
    return filename.split('.').pop()?.toLowerCase() || '';
}

function getFileIcon(filename: string): React.ReactNode {
    const ext = getFileExt(filename);
    const iconMap: Record<string, React.ReactNode> = {
        pdf: <IconFileTypePdf size={20} color="red" />,
        doc: <IconFileTypeDoc size={20} color="blue" />,
        docx: <IconFileTypeDoc size={20} color="blue" />,
        xls: <IconFileTypeXls size={20} color="green" />,
        xlsx: <IconFileTypeXls size={20} color="green" />,
        jpg: <IconPhoto size={20} color="purple" />,
        jpeg: <IconPhoto size={20} color="purple" />,
        png: <IconPhoto size={20} color="purple" />,
        gif: <IconPhoto size={20} color="purple" />,
        webp: <IconPhoto size={20} color="purple" />,
        zip: <IconFileTypeZip size={20} color="yellow" />,
        rar: <IconFileTypeZip size={20} color="yellow" />,
        '7z': <IconFileTypeZip size={20} color="yellow" />,
    };
    return iconMap[ext] || <IconFile size={20} color="gray" />;
}

const FileAttachPanel: React.FC<FileAttachPanelProps> = ({
    fileUrl: externalFileUrl,
    fileName: externalFileName,
    fileSize: externalFileSize,
    onUploadComplete,
    onUploadError,
}) => {
    const fileInputRef = useRef<HTMLInputElement>(null);
    const hasExternalFile = !!externalFileName;

    // 使用 useChunkedUpload 管理上传逻辑
    const upload = useChunkedUpload({
        concurrency: 3,
        autoResume: true,
        storageKey: 'file_attach_upload',
        onProgress: (pct: number) => {
            // progress is tracked internally
        },
        onComplete: (result) => {
            if (onUploadComplete) {
                onUploadComplete({
                    fileUrl: result.fileUrl,
                    fileName: upload.fileName || '',
                    fileSize: upload.totalBytes,
                });
            }
        },
        onError: (err) => {
            onUploadError?.(err);
        },
    });

    // 获取当前显示的文件信息
    const displayFileName = externalFileName || upload.fileName;
    const displayFileSize = externalFileSize || upload.totalBytes;
    const displayFileUrl = externalFileUrl || upload.fileUrl;

    const handleFileSelect = (file: File | null) => {
        if (!file) return;
        upload.start(file);
    };

    // 进度条颜色
    const progressColor =
        upload.status === 'error'
            ? 'red'
            : upload.status === 'paused'
                ? 'yellow'
                : 'blue';

    // 格式化上传速度
    const formatSpeed = (bytesPerSec: number): string => {
        if (bytesPerSec === 0) return '';
        const k = 1024;
        const sizes = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
        const i = Math.floor(Math.log(bytesPerSec) / Math.log(k));
        return parseFloat((bytesPerSec / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
    };

    return (
        <Paper p="sm" withBorder>
            <Stack gap="xs">
                <Group justify="space-between">
                    <Group gap="xs" style={{ flex: 1, minWidth: 0 }}>
                        {displayFileName ? (
                            <>
                                {getFileIcon(displayFileName)}
                                <Text size="sm" truncate style={{ maxWidth: 200 }}>
                                    {displayFileName}
                                </Text>
                                {displayFileSize > 0 && (
                                    <Text size="xs" c="dimmed">
                                        ({formatFileSize(displayFileSize)})
                                    </Text>
                                )}
                                {displayFileUrl && (
                                    <ActionIcon
                                        variant="subtle"
                                        component="a"
                                        href={displayFileUrl}
                                        target="_blank"
                                        size="sm"
                                    >
                                        <IconExternalLink size={14} />
                                    </ActionIcon>
                                )}
                            </>
                        ) : upload.status === 'completed' ? (
                            <Group gap="xs">
                                <IconFile size={20} color="gray" />
                                <Text size="sm" c="green" fw={500}>
                                    上传完成
                                </Text>
                            </Group>
                        ) : upload.status === 'idle' ? (
                            <>
                                <IconPaperclip size={16} />
                                <Text size="sm" c="dimmed">
                                    暂无附件
                                </Text>
                            </>
                        ) : (
                            // uploading / paused / error
                            <>
                                {getFileIcon(upload.fileName || '文件')}
                                <Text size="sm" truncate style={{ maxWidth: 200 }}>
                                    {upload.fileName || '上传中...'}
                                </Text>
                            </>
                        )}
                    </Group>

                    {/* 操作按钮 */}
                    <Group gap="xs" wrap="nowrap">
                        {upload.status === 'uploading' && (
                            <>
                                <Tooltip label="暂停">
                                    <ActionIcon
                                        variant="subtle"
                                        color="yellow"
                                        onClick={upload.pause}
                                        size="sm"
                                    >
                                        <IconPlayerPause size={14} />
                                    </ActionIcon>
                                </Tooltip>
                                <Tooltip label="取消">
                                    <ActionIcon
                                        variant="subtle"
                                        color="red"
                                        onClick={upload.cancel}
                                        size="sm"
                                    >
                                        <IconX size={14} />
                                    </ActionIcon>
                                </Tooltip>
                            </>
                        )}
                        {upload.status === 'paused' && (
                            <>
                                <Tooltip label="继续">
                                    <ActionIcon
                                        variant="subtle"
                                        color="blue"
                                        onClick={upload.resume}
                                        size="sm"
                                    >
                                        <IconPlayerPlay size={14} />
                                    </ActionIcon>
                                </Tooltip>
                                <Tooltip label="取消">
                                    <ActionIcon
                                        variant="subtle"
                                        color="red"
                                        onClick={upload.cancel}
                                        size="sm"
                                    >
                                        <IconX size={14} />
                                    </ActionIcon>
                                </Tooltip>
                            </>
                        )}
                        {upload.status === 'error' && (
                            <>
                                <Tooltip label="重试">
                                    <ActionIcon
                                        variant="subtle"
                                        color="orange"
                                        onClick={upload.retry}
                                        size="sm"
                                    >
                                        <IconRefresh size={14} />
                                    </ActionIcon>
                                </Tooltip>
                                <Tooltip label="清除">
                                    <ActionIcon
                                        variant="subtle"
                                        color="red"
                                        onClick={upload.cancel}
                                        size="sm"
                                    >
                                        <IconX size={14} />
                                    </ActionIcon>
                                </Tooltip>
                            </>
                        )}
                        {upload.status === 'idle' && !displayFileName && (
                            <FileButton onChange={handleFileSelect} accept="*">
                                {(props) => (
                                    <Button
                                        {...props}
                                        size="xs"
                                        variant="light"
                                        leftSection={<IconUpload size={14} />}
                                    >
                                        上传文件
                                    </Button>
                                )}
                            </FileButton>
                        )}
                        {(upload.status === 'completed' || displayFileName) && (
                            <Button
                                size="xs"
                                variant="light"
                                color="green"
                                leftSection={<IconUpload size={14} />}
                                onClick={() => {
                                    upload.cancel();
                                    // 同时清除外部文件引用（通过重新选择文件实现）
                                }}
                            >
                                重新上传
                            </Button>
                        )}
                    </Group>
                </Group>

                {/* 进度条 */}
                {(upload.status === 'uploading' || upload.status === 'paused') && (
                    <Box>
                        <Group gap="xs" mb={2}>
                            <Progress
                                value={upload.progress}
                                color={progressColor}
                                size="sm"
                                style={{ flex: 1 }}
                                animated={upload.status === 'uploading'}
                            />
                            <Text
                                size="xs"
                                c="dimmed"
                                style={{ minWidth: 35, textAlign: 'right' }}
                            >
                                {upload.progress}%
                            </Text>
                        </Group>
                        {upload.status === 'uploading' && upload.speed > 0 && (
                            <Text size="xs" c="dimmed">
                                {formatSpeed(upload.speed)}
                            </Text>
                        )}
                    </Box>
                )}

                {/* 错误信息 */}
                {upload.status === 'error' && upload.error && (
                    <Text size="xs" c="red">
                        {upload.error}
                    </Text>
                )}
            </Stack>
        </Paper>
    );
};

export default FileAttachPanel;