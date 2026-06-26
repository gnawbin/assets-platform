'use client';
import React, { useRef } from 'react';
import { Group, Text, Button, ActionIcon, Paper, FileButton } from '@mantine/core';
import { IconUpload, IconPaperclip, IconX, IconExternalLink } from '@tabler/icons-react';

interface FileAttachPanelProps {
    fileUrl?: string;
    fileName?: string;
    fileSize?: number;
    onFileUpload?: (file: File) => Promise<string>;
    uploading?: boolean;
}

const formatFileSize = (bytes: number): string => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
};

const FileAttachPanel: React.FC<FileAttachPanelProps> = ({
    fileUrl,
    fileName,
    fileSize,
    onFileUpload,
    uploading,
}) => {
    const [localUploading, setLocalUploading] = React.useState(false);

    const handleFileChange = async (file: File | null) => {
        if (!file || !onFileUpload) return;
        setLocalUploading(true);
        try {
            await onFileUpload(file);
        } finally {
            setLocalUploading(false);
        }
    };

    const isLoading = uploading || localUploading;

    return (
        <Paper p="sm" withBorder>
            <Group justify="space-between">
                <Group gap="xs">
                    <IconPaperclip size={16} />
                    {fileName ? (
                        <>
                            <Text size="sm">{fileName}</Text>
                            {fileSize && fileSize > 0 && (
                                <Text size="xs" c="dimmed">
                                    ({formatFileSize(fileSize)})
                                </Text>
                            )}
                            {fileUrl && (
                                <ActionIcon
                                    variant="subtle"
                                    component="a"
                                    href={fileUrl}
                                    target="_blank"
                                    size="sm"
                                >
                                    <IconExternalLink size={14} />
                                </ActionIcon>
                            )}
                            {/* 清除文件绑定，由外部控制 */}
                        </>
                    ) : (
                        <Text size="sm" c="dimmed">
                            暂无附件
                        </Text>
                    )}
                </Group>

                <Group gap="xs">
                    {onFileUpload && (
                        <FileButton onChange={handleFileChange} accept="*">
                            {(props) => (
                                <Button
                                    {...props}
                                    size="xs"
                                    variant="light"
                                    leftSection={<IconUpload size={14} />}
                                    loading={isLoading}
                                >
                                    {isLoading ? '上传中...' : '上传文件'}
                                </Button>
                            )}
                        </FileButton>
                    )}
                </Group>
            </Group>
        </Paper>
    );
};

export default FileAttachPanel;