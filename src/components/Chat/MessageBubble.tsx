'use client';
import React from 'react';
import { Group, Paper, Avatar, Text, Badge, Anchor, Divider, Card } from '@mantine/core';
import type { AssetInfo } from '@/services/conversationService';

interface MessageBubbleProps {
    role: 'user' | 'assistant';
    content: string;
    citedAssets?: AssetInfo[];
    referenceText?: string;
    metadata?: { model?: string; durationMs?: number; };
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
    role, content, citedAssets, referenceText, metadata,
}) => {
    const isUser = role === 'user';

    return (
        <Group justify={isUser ? 'flex-end' : 'flex-start'} align="flex-start" mb="md">
            {!isUser && <Avatar color="violet" radius="xl">AI</Avatar>}
            <Paper withBorder p="md" style={{
                maxWidth: '70%',
                backgroundColor: isUser ? 'var(--mantine-color-blue-light)' : 'white',
            }}>
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

export default MessageBubble;