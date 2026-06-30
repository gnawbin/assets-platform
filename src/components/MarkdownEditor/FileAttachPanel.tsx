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

    /** 上传状态（由父组件传入以驱动进度显示） */
    uploadStatus?: AttachUploadStatus;
    /** 上传进度 0-100 */
    uploadProgress?: number;
    /** 上传速度（字节/秒） */
    uploadSpeed?: number;
    /** 错误信息 */
    uploadError?: string | null;

    /** 用户选择文件后的回调 */
    onFileSelect?: (file: File) => void;
    /** 暂停 */
    onPause?: () => void;
    /** 继续 */
    onResume?: () => void;
    /** 取消/清除 */
    onCancel?: () => void;
    /** 重试 */
    onRetry?: () => void;
    /** 正在上传（外部 loading） */
    uploading?: boolean;
}

const formatFileSize = (bytes: number): string => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
};

const formatSpeed = (bytesPerSec: number): string => {
    if (bytesPerSec === 0) return '';
    const k = 1024;
    const sizes = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
    const i = Math.floor(Math.log(bytesPerSec) / Math.log(k));
    return parseFloat((bytesPerSec / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
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
    uploadStatus = 'idle',
    uploadProgress = 0,
    uploadSpeed = 0,
    uploadError = null,
    onFileSelect,
    onPause,
    onResume,
    onCancel,
    onRetry,
    uploading: externalUploading,
}) => {
    const fileInputRef = useRef<HTMLInputElement>(null);

    const hasExternalFile = !!externalFileName;
    const isUploading = uploadStatus === 'uploading' || externalUploading;
    const progressColor =
        uploadStatus === 'error'
            ? 'red'
            : uploadStatus === 'paused'
                ? 'yellow'
                : 'blue';

    const handleFileChange = (file: File | null) => {
        if (!file || !onFileSelect) return;
        onFileSelect(file);
    };

    return (
        <Paper p="sm" withBorder>
            <Stack gap="xs">
                <Group justify="space-between">
                    <Group gap="xs" style={{ flex: 1, minWidth: 0 }}>
                        {hasExternalFile ? (
                            <>
                                {getFileIcon(externalFileName || '')}
                                <Text size="sm" truncate style={{ maxWidth: 200 }}>
                                    {externalFileName}
                                </Text>
                                {externalFileSize && externalFileSize > 0 && (
                                    <Text size="xs" c="dimmed">
                                        ({formatFileSize(externalFileSize)})
                                    </Text>
                                )}
                                {externalFileUrl && (
                                    <ActionIcon
                                        variant="subtle"
                                        component="a"
                                        href={externalFileUrl}
                                        target="_blank"
                                        size="sm"
                                    >
                                        <IconExternalLink size={14} />
                                    </ActionIcon>
                                )}
                            </>
                        ) : uploadStatus === 'completed' ? (
                            <Group gap="xs">
                                <IconFile size={20} color="gray" />
                                <Text size="sm" c="green" fw={500}>
                                    上传完成
                                </Text>
                            </Group>
                        ) : uploadStatus === 'idle' ? (
                            <>
                                <IconPaperclip size={16} />
                                <Text size="sm" c="dimmed">
                                    暂无附件
                                </Text>
                            </>
                        ) : (
                            <>
                                <IconFile size={20} color="gray" />
                                <Text size="sm" truncate style={{ maxWidth: 200 }}>
                                    上传中...
                                </Text>
                            </>
                        )}
                    </Group>

                    {/* 操作按钮 */}
                    <Group gap="xs" wrap="nowrap">
                        {uploadStatus === 'uploading' && (
                            <>
                                <Tooltip label="暂停">
                                    <ActionIcon
                                        variant="subtle"
                                        color="yellow"
                                        onClick={onPause}
                                        size="sm"
                                    >
                                        <IconPlayerPause size={14} />
                                    </ActionIcon>
                                </Tooltip>
                                <Tooltip label="取消">
                                    <ActionIcon
                                        variant="subtle"
                                        color="red"
                                        onClick={onCancel}
                                        size="sm"
                                    >
                                        <IconX size={14} />
                                    </ActionIcon>
                                </Tooltip>
                            </>
                        )}
                        {uploadStatus === 'paused' && (
                            <>
                                <Tooltip label="继续">
                                    <ActionIcon
                                        variant="subtle"
                                        color="blue"
                                        onClick={onResume}
                                        size="sm"
                                    >
                                        <IconPlayerPlay size={14} />
                                    </ActionIcon>
                                </Tooltip>
                                <Tooltip label="取消">
                                    <ActionIcon
                                        variant="subtle"
                                        color="red"
                                        onClick={onCancel}
                                        size="sm"
                                    >
                                        <IconX size={14} />
                                    </ActionIcon>
                                </Tooltip>
                            </>
                        )}
                        {uploadStatus === 'error' && (
                            <>
                                <Tooltip label="重试">
                                    <ActionIcon
                                        variant="subtle"
                                        color="orange"
                                        onClick={onRetry}
                                        size="sm"
                                    >
                                        <IconRefresh size={14} />
                                    </ActionIcon>
                                </Tooltip>
                                <Tooltip label="清除">
                                    <ActionIcon
                                        variant="subtle"
                                        color="red"
                                        onClick={onCancel}
                                        size="sm"
                                    >
                                        <IconX size={14} />
                                    </ActionIcon>
                                </Tooltip>
                            </>
                        )}
                        {uploadStatus === 'idle' && !hasExternalFile && onFileSelect && (
                            <FileButton onChange={handleFileChange} accept="*">
                                {(props) => (
                                    <Button
                                        {...props}
                                        size="xs"
                                        variant="light"
                                        leftSection={<IconUpload size={14} />}
                                        loading={isUploading}
                                    >
                                        {isUploading ? '上传中...' : '上传文件'}
                                    </Button>
                                )}
                            </FileButton>
                        )}
                        {(uploadStatus === 'completed' || hasExternalFile) && (
                            <Button
                                size="xs"
                                variant="light"
                                color="green"
                                leftSection={<IconUpload size={14} />}
                                onClick={onCancel}
                            >
                                重新上传
                            </Button>
                        )}
                    </Group>
                </Group>

                {/* 进度条 */}
                {(uploadStatus === 'uploading' || uploadStatus === 'paused') && (
                    <Box>
                        <Group gap="xs" mb={2}>
                            <Progress
                                value={uploadProgress}
                                color={progressColor}
                                size="sm"
                                style={{ flex: 1 }}
                                animated={uploadStatus === 'uploading'}
                            />
                            <Text
                                size="xs"
                                c="dimmed"
                                style={{ minWidth: 35, textAlign: 'right' }}
                            >
                                {uploadProgress}%
                            </Text>
                        </Group>
                        {uploadStatus === 'uploading' && uploadSpeed > 0 && (
                            <Text size="xs" c="dimmed">
                                {formatSpeed(uploadSpeed)}
                            </Text>
                        )}
                    </Box>
                )}

                {/* 错误信息 */}
                {uploadStatus === 'error' && uploadError && (
                    <Text size="xs" c="red">
                        {uploadError}
                    </Text>
                )}
            </Stack>
        </Paper>
    );
};

export default FileAttachPanel;