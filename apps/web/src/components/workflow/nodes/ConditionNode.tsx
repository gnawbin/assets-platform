import React, { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';
import { IconGitBranch } from '@tabler/icons-react';
import { Paper, Text, Group, Badge } from '@mantine/core';

const ConditionNode: React.FC<NodeProps> = ({ data, selected }) => {
    const d = data as Record<string, unknown>;
    const label = (d.label as string) || '条件分支';
    const field = d.field as string | undefined;
    const operator = d.operator as string | undefined;
    const value = d.value;

    return (
        <Paper
            withBorder
            p="sm"
            radius="md"
            style={{
                borderColor: selected ? '#e8590c' : '#ffa94d',
                borderWidth: 2,
                minWidth: 180,
                background: '#fff4e6',
                clipPath: 'polygon(50% 0%, 100% 50%, 50% 100%, 0% 50%)',
                padding: '16px 24px',
            }}
        >
            <Handle type="target" position={Position.Top} style={{ background: '#e8590c' }} />
            <Handle
                type="source"
                position={Position.Bottom}
                id="yes"
                style={{ background: '#2b8a3e', left: '30%' }}
            />
            <Handle
                type="source"
                position={Position.Bottom}
                id="no"
                style={{ background: '#e03131', left: '70%' }}
            />
            <Group gap="xs" justify="center" wrap="nowrap">
                <IconGitBranch size={18} color="#e8590c" />
                <div style={{ textAlign: 'center' }}>
                    <Text size="sm" fw={600}>
                        {label}
                    </Text>
                    {field && operator && (
                        <Badge size="xs" color="orange" variant="light" mt={2}>
                            {field} {operator} {String(value ?? '')}
                        </Badge>
                    )}
                </div>
            </Group>
            <Group justify="space-between" mt={4}>
                <Text size="xs" c="green" fw={500}>是</Text>
                <Text size="xs" c="red" fw={500}>否</Text>
            </Group>
        </Paper>
    );
};

export default memo(ConditionNode);