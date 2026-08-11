import React, { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';
import { IconBrain } from '@tabler/icons-react';
import { Paper, Text, Group } from '@mantine/core';

const LLMNode: React.FC<NodeProps> = ({ data, selected }) => {
    const d = data as Record<string, unknown>;
    const label = (d.label as string) || 'LLM';
    const prompt = d.prompt as string | undefined;
    const model = d.model as string | undefined;

    return (
        <Paper
            withBorder
            p="sm"
            radius="md"
            style={{
                borderColor: selected ? '#9c36b5' : '#da77f2',
                borderWidth: 2,
                minWidth: 180,
                background: '#f8f0fc',
            }}
        >
            <Handle type="target" position={Position.Top} style={{ background: '#9c36b5' }} />
            <Handle type="source" position={Position.Bottom} style={{ background: '#9c36b5' }} />
            <Group gap="xs" wrap="nowrap">
                <IconBrain size={20} color="#9c36b5" />
                <div style={{ flex: 1, minWidth: 0 }}>
                    <Text size="sm" fw={600}>
                        {label}
                    </Text>
                    {prompt && (
                        <Text size="xs" c="dimmed" lineClamp={2} mt={2}>
                            {prompt}
                        </Text>
                    )}
                    {model && (
                        <Text size="xs" c="dimmed" mt={1}>
                            model: {model}
                        </Text>
                    )}
                </div>
            </Group>
        </Paper>
    );
};

export default memo(LLMNode);