'use client';
import React from 'react';
import { Group, Text, Badge, Card, Divider } from '@mantine/core';
import type { LlmModel } from '@/services/llmProviderService';

interface ModelListProps {
    models: LlmModel[];
    providerName: string;
}

const MODEL_TYPE_COLORS: Record<string, string> = {
    chat: 'blue',
    embedding: 'teal',
    asr: 'violet',
    tts: 'pink',
};

const MODEL_TYPE_LABELS: Record<string, string> = {
    chat: '对话',
    embedding: '向量',
    asr: '语音识别',
    tts: '语音合成',
};

const ModelList: React.FC<ModelListProps> = ({ models, providerName }) => {
    const grouped = models.reduce<Record<string, LlmModel[]>>((acc, m) => {
        if (!acc[m.model_type]) acc[m.model_type] = [];
        acc[m.model_type].push(m);
        return acc;
    }, {});

    return (
        <Card withBorder padding="sm">
            <Text size="sm" fw={600} mb="sm">{providerName} - 模型列表</Text>
            <Divider mb="sm" />
            {Object.entries(grouped).map(([type, typeModels]) => (
                <React.Fragment key={type}>
                    <Text size="xs" fw={500} c="dimmed" mb={4}>
                        {MODEL_TYPE_LABELS[type] || type} ({typeModels.length})
                    </Text>
                    {typeModels.map(m => (
                        <Group key={m.id} gap="xs" mb={3}>
                            <Badge variant="light" color={MODEL_TYPE_COLORS[m.model_type] || 'gray'} size="xs">
                                {m.model_code}
                            </Badge>
                            <Text size="xs" style={{ flex: 1 }}>{m.model_name}</Text>
                            {m.context_window && (
                                <Text size="xs" c="dimmed">{Math.round(m.context_window / 1000)}K ctx</Text>
                            )}
                        </Group>
                    ))}
                </React.Fragment>
            ))}
        </Card>
    );
};

export default ModelList;