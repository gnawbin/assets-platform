'use client';
import React from 'react';
import { Group, Paper, Avatar, Text, Badge, Anchor, Divider, Card } from '@mantine/core';
import type { AssetInfo, ChatAttachment } from '@/services/conversationService';

interface MessageBubbleProps {
    role: 'user' | 'assistant';
    content: string;
    citedAssets?: AssetInfo[];
    referenceText?: string;
    metadata?: { model?: string; durationMs?: number; };
    /** 多模态附件（用户消息渲染） */
    attachments?: ChatAttachment[];
}

const OKF_TYPE_LABELS: Record<string, string> = {
    raw_source: '原始素材', concept: '概念', fact: '事实',
    rule: '规则', param: '参数', process: '流程', case: '案例',
};

const CitationPanel: React.FC<{ citedAssets: AssetInfo[]; referenceText?: string }> = ({
    citedAssets, referenceText,
}) => (
    <Card withBorder padding="sm" mt="sm" style={{ backgroundColor: '#f8f9fa' }}>
        <Text size="sm" fw={600} mb="xs">
            📎 引用来源 ({citedAssets.length})
        </Text>
        {citedAssets.map((asset) => (
            <Group key={asset.id} gap="xs" mb={4}>
                <Badge variant="light" color="blue" size="xs">
                    {OKF_TYPE_LABELS[asset.okfType] || asset.okfType}
                </Badge>
                <Anchor href={`/knowledge-asset?id=${asset.id}`} target="_blank" size="sm">
                    {asset.title}
                </Anchor>
            </Group>
        ))}
        {referenceText && (
            <>
                <Divider my="xs" />
                <Text size="xs" c="dimmed" lineClamp={3}>{referenceText}</Text>
            </>
        )}
    </Card>
);

const MessageBubble: React.FC<MessageBubbleProps> = ({
    role, content, citedAssets, referenceText, metadata, attachments,
}) => {
    const isUser = role === 'user';

    return (
        <Group justify={isUser ? 'flex-end' : 'flex-start'} align="flex-start" mb="md">
            {!isUser && <Avatar color="violet" radius="xl">AI</Avatar>}
            <Paper withBorder p="md" style={{
                maxWidth: '70%',
                backgroundColor: isUser ? 'var(--mantine-color-blue-light)' : 'white',
            }}>
                {isUser && attachments && attachments.length > 0 && (
                    <Group gap="xs" mb="xs">
                        {attachments.map((att, i) => (
                            <AttachmentView key={`${att.name}-${i}`} att={att} />
                        ))}
                    </Group>
                )}
                <Text size="sm" style={{ whiteSpace: 'pre-wrap' }}>{content}</Text>
                {citedAssets && citedAssets.length > 0 && (
                    <CitationPanel citedAssets={citedAssets} referenceText={referenceText} />
                )}
                {metadata && (
                    <Text size="xs" c="dimmed" ta="right" mt="xs">
                        {metadata.model && `${metadata.model} · `}{metadata.durationMs}ms
                    </Text>
                )}
            </Paper>
            {isUser && <Avatar color="blue" radius="xl">👤</Avatar>}
        </Group>
    );
};

/** 附件视图：图片缩略图 / 视频播放 / 音频播放 / 文件卡片 */
const AttachmentView: React.FC<{ att: ChatAttachment }> = ({ att }) => {
    const src = att.dataUrl || att.url;
    if (!src) return null;

    if (att.type === 'image') {
        return (
            // eslint-disable-next-line @next/next/no-img-element
            <img
                src={src}
                alt={att.name}
                style={{ maxHeight: 160, maxWidth: 220, borderRadius: 8, objectFit: 'cover', border: '1px solid #dee2e6' }}
            />
        );
    }
    if (att.type === 'video') {
        return <video src={src} controls style={{ maxWidth: 280, maxHeight: 180, borderRadius: 8 }} />;
    }
    if (att.type === 'audio') {
        return <audio src={src} controls style={{ maxWidth: 260 }} />;
    }
    // document
    return (
        <Card withBorder padding="xs" radius="md" style={{ minWidth: 160 }}>
            <Text size="xs" truncate style={{ maxWidth: 180 }}>📄 {att.name}</Text>
            <Anchor href={att.url} target="_blank" size="xs">查看文件</Anchor>
        </Card>
    );
};

export default MessageBubble;