import React, { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';
import { IconArrowRight } from '@tabler/icons-react';
import { Paper, Text, Group } from '@mantine/core';

const OutputNode: React.FC<NodeProps> = ({ data, selected }) => {
    const d = data as Record<string, unknown>;
    const label = (d.label as string) || '输出';

    return (
        <Paper
            withBorder
            p="sm"
            radius="md"
            style={{
                borderColor: selected ? '#c92a2a' : '#ff8787',
                borderWidth: 2,
                minWidth: 180,
                background: '#fff5f5',
            }}
        >
            <Handle type="target" position={Position.Top} style={{ background: '#c92a2a' }} />
            <Group gap="xs" wrap="nowrap">
                <IconArrowRight size={20} color="#c92a2a" />
                <Text size="sm" fw={600}>
                    {label}
                </Text>
            </Group>
        </Paper>
    );
};

export default memo(OutputNode);