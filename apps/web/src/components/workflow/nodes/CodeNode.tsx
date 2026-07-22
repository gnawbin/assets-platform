import React, { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';
import { IconCode } from '@tabler/icons-react';
import { Paper, Text, Group, Badge } from '@mantine/core';

const CodeNode: React.FC<NodeProps> = ({ data, selected }) => {
    const d = data as Record<string, unknown>;
    const label = (d.label as string) || '代码';
    const language = d.language as string | undefined;

    return (
        <Paper
            withBorder
            p="sm"
            radius="md"
            style={{
                borderColor: selected ? '#495057' : '#adb5bd',
                borderWidth: 2,
                minWidth: 180,
                background: '#f8f9fa',
            }}
        >
            <Handle type="target" position={Position.Top} style={{ background: '#495057' }} />
            <Handle type="source" position={Position.Bottom} style={{ background: '#495057' }} />
            <Group gap="xs" wrap="nowrap">
                <IconCode size={20} color="#495057" />
                <div>
                    <Text size="sm" fw={600}>
                        {label}
                    </Text>
                    {language && (
                        <Badge size="xs" color="gray" variant="light" mt={2}>
                            {language}
                        </Badge>
                    )}
                </div>
            </Group>
        </Paper>
    );
};

export default memo(CodeNode);