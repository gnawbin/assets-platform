'use client';
/**
 * 聊天附件输入组件
 *
 * 支持多附件：图片（base64 即时预览）、视频/音频/PDF（S3 分片上传，显示进度）。
 * 限制：单条消息 ≤ 5 个附件，其中图片 ≤ 5 张、单张图片 ≤ 10MB。
 */
import React, {
    useRef, useState, useCallback, useEffect, forwardRef, useImperativeHandle,
} from 'react';
import {
    ActionIcon, Tooltip, Text, Group, Box, Loader, Progress, Paper,
} from '@mantine/core';
import {
    IconPaperclip, IconX, IconVideo, IconMusic, IconFileText, IconAlertCircle, IconPhoto,
} from '@tabler/icons-react';
import type { ChatAttachment } from '@/services/conversationService';
import { useChunkedUpload } from '@/hooks/useChunkedUpload';

// ======================== 限制 ========================
const MAX_ATTACHMENTS = 5;
const MAX_IMAGES = 5;
const MAX_IMAGE_SIZE = 10 * 1024 * 1024; // 10MB
const ACCEPT = 'image/*,video/*,audio/*,.pdf,.doc,.docx,.txt';

export interface PendingAttachment {
    id: string;
    name: string;
    type: 'image' | 'video' | 'audio' | 'document';
    size: number;
    mime?: string;
    dataUrl?: string;
    url?: string;
    progress: number;
    uploading: boolean;
    error?: string;
    /** 待上传的原始文件（video/audio/document 使用） */
    file?: File;
}

export interface ChatAttachmentInputHandle {
    clear: () => void;
}

interface ChatAttachmentInputProps {
    disabled?: boolean;
    attachments: ChatAttachment[];
    onChange: (atts: ChatAttachment[]) => void;
}

// ======================== 工具函数 ========================

function extOf(name: string): string {
    return name.split('.').pop()?.toLowerCase() || '';
}

function typeOfFile(file: File): PendingAttachment['type'] {
    if (file.type.startsWith('image/')) return 'image';
    if (file.type.startsWith('video/')) return 'video';
    if (file.type.startsWith('audio/')) return 'audio';
    const ext = extOf(file.name);
    if (['pdf', 'doc', 'docx', 'txt', 'md'].includes(ext)) return 'document';
    return 'document';
}

function fileIcon(type: PendingAttachment['type'], size = 18) {
    switch (type) {
        case 'image': return <IconPhoto size={size} color="purple" />;
        case 'video': return <IconVideo size={size} color="orange" />;
        case 'audio': return <IconMusic size={size} color="teal" />;
        default: return <IconFileText size={size} color="blue" />;
    }
}

// ======================== 单个上传任务（视频/音频/文档） ========================

function UploadTaskItem({ att, onReady, onRemove }: {
    att: PendingAttachment;
    onReady: (id: string, url: string, err?: string) => void;
    onRemove: (id: string) => void;
}) {
    const upload = useChunkedUpload({
        concurrency: 1,
        autoResume: true,
        onComplete: (result) => onReady(att.id, result.fileUrl),
        onError: (err) => onReady(att.id, '', err),
    });
    const startedRef = useRef(false);

    useEffect(() => {
        if (startedRef.current || !att.file) return;
        startedRef.current = true;
        upload.start(att.file).catch(() => { /* 错误通过 onError 回调 */ });
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    const progress = upload.status === 'completed' ? 100 : upload.progress;

    return (
        <Paper withBorder p="xs" radius="md" style={{ minWidth: 220, maxWidth: 300 }}>
            <Group gap="xs" wrap="nowrap" justify="space-between">
                <Group gap="xs" wrap="nowrap" style={{ flex: 1, minWidth: 0 }}>
                    {fileIcon(att.type)}
                    <Text size="sm" truncate style={{ flex: 1 }}>{att.name}</Text>
                </Group>
                <ActionIcon size="sm" color="red" variant="subtle" onClick={() => onRemove(att.id)}>
                    <IconX size={14} />
                </ActionIcon>
            </Group>
            {upload.status !== 'completed' && upload.status !== 'error' && (
                <Group gap="xs" mt={6} align="center">
                    <Loader size={12} />
                    <Progress value={progress} size={6} style={{ flex: 1 }} />
                    <Text size="xs" c="dimmed">{progress}%</Text>
                </Group>
            )}
            {upload.error && (
                <Text size="xs" c="red" mt={4} style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                    <IconAlertCircle size={12} /> {upload.error}
                </Text>
            )}
        </Paper>
    );
}

// ======================== 主组件 ========================

export const ChatAttachmentInput = forwardRef<ChatAttachmentInputHandle, ChatAttachmentInputProps>(
    function ChatAttachmentInput({ disabled, attachments, onChange }, ref) {
        const fileInputRef = useRef<HTMLInputElement>(null);
        const [pending, setPending] = useState<PendingAttachment[]>([]);

        // 对外暴露清空方法
        useImperativeHandle(ref, () => ({
            clear: () => setPending([]),
        }), []);

        // pending 变化 → 计算可发送的附件列表（图片含 dataUrl，其他含 url）
        useEffect(() => {
            const ready = pending
                .filter((a) => !a.error)
                .map((a): ChatAttachment => ({
                    type: a.type,
                    name: a.name,
                    dataUrl: a.dataUrl,
                    url: a.url,
                    mime: a.mime,
                }))
                .filter((a) => Boolean(a.dataUrl || a.url));
            onChange(ready);
            // eslint-disable-next-line react-hooks/exhaustive-deps
        }, [pending]);

        const handleFiles = useCallback((files: FileList | null) => {
            if (!files || files.length === 0) return;
            const imageCount = pending.filter((a) => a.type === 'image').length;

            Array.from(files).forEach((file) => {
                const id = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
                const type = typeOfFile(file);

                // 上限校验
                if (pending.length + 1 > MAX_ATTACHMENTS) {
                    alert(`单条消息最多支持 ${MAX_ATTACHMENTS} 个附件`);
                    return;
                }
                if (type === 'image' && imageCount + 1 > MAX_IMAGES) {
                    alert(`单条消息最多支持 ${MAX_IMAGES} 张图片`);
                    return;
                }
                if (type === 'image' && file.size > MAX_IMAGE_SIZE) {
                    alert(`图片 ${file.name} 超过 10MB 限制`);
                    return;
                }

                const base: PendingAttachment = {
                    id, name: file.name, type, size: file.size, mime: file.type || undefined,
                    progress: 0, uploading: false, file,
                };

                if (type === 'image') {
                    // 图片：前端读 base64 即时预览
                    const reader = new FileReader();
                    reader.onload = () => {
                        const dataUrl = reader.result as string;
                        setPending((prev) => prev.map((p) => (p.id === id ? { ...p, dataUrl } : p)));
                    };
                    reader.onerror = () => {
                        setPending((prev) => prev.map((p) => (p.id === id ? { ...p, error: '图片读取失败' } : p)));
                    };
                    reader.readAsDataURL(file);
                }

                setPending((prev) => [...prev, { ...base, file }]);
            });
        }, [pending]);

        const handleReady = useCallback((id: string, url: string, err?: string) => {
            setPending((prev) => prev.map((p) => {
                if (p.id !== id) return p;
                if (err) return { ...p, uploading: false, error: err, url: undefined };
                return { ...p, uploading: false, url, progress: 100 };
            }));
        }, []);

        const handleRemove = useCallback((id: string) => {
            setPending((prev) => prev.filter((p) => p.id !== id));
        }, []);

        return (
            <Group gap="xs" align="center" wrap="nowrap">
                <Tooltip label="上传文件/图片/视频（最多 5 个）">
                    <ActionIcon
                        variant="subtle"
                        color="blue"
                        disabled={disabled}
                        onClick={() => fileInputRef.current?.click()}
                    >
                        <IconPaperclip size={20} />
                    </ActionIcon>
                </Tooltip>
                <input
                    ref={fileInputRef}
                    type="file"
                    accept={ACCEPT}
                    multiple
                    style={{ display: 'none' }}
                    onChange={(e) => {
                        handleFiles(e.target.files);
                        e.target.value = '';
                    }}
                />
                {pending.length > 0 && (
                    <Box style={{ display: 'flex', gap: 8, overflowX: 'auto', maxWidth: '100%', flex: 1 }}>
                        {pending.map((att) => (
                            <Box key={att.id} style={{ flexShrink: 0 }}>
                                {att.type === 'image' && att.dataUrl ? (
                                    <Box style={{ position: 'relative' }}>
                                        {/* eslint-disable-next-line @next/next/no-img-element */}
                                        <img
                                            src={att.dataUrl}
                                            alt={att.name}
                                            style={{ height: 40, width: 40, objectFit: 'cover', borderRadius: 6, border: '1px solid #dee2e6' }}
                                        />
                                        <ActionIcon
                                            size="xs"
                                            color="red"
                                            variant="filled"
                                            style={{ position: 'absolute', top: -6, right: -6 }}
                                            onClick={() => handleRemove(att.id)}
                                        >
                                            <IconX size={10} />
                                        </ActionIcon>
                                    </Box>
                                ) : (
                                    <UploadTaskItem att={att} onReady={handleReady} onRemove={handleRemove} />
                                )}
                            </Box>
                        ))}
                    </Box>
                )}
                {attachments.length > 0 && (
                    <Text size="xs" c="dimmed" style={{ whiteSpace: 'nowrap' }}>
                        已选 {attachments.length}/{MAX_ATTACHMENTS}
                    </Text>
                )}
            </Group>
        );
    },
);

export default ChatAttachmentInput;



