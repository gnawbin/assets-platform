import React, { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';
import { IconBrain } from '@tabler/icons-react';
import { Paper, Text, Group, Badge } from '@mantine/core';

const SkillNode: React.FC<NodeProps> = ({ data, selected }) => {
    const d = data as Record<string, unknown>;
    const label = (d.label as string) || 'Skill';
    const skillId = d.skill_id as string | undefined;

    return (
        <Paper
            withBorder
            p="sm"
            radius="md"
            style={{
                borderColor: selected ? '#1971c2' : '#74c0fc',
                borderWidth: 2,
                minWidth: 180,
                background: '#e7f5ff',
            }}
        >
            <Handle type="target" position={Position.Top} style={{ background: '#1971c2' }} />
            <Handle type="source" position={Position.Bottom} style={{ background: '#1971c2' }} />
            <Group gap="xs" wrap="nowrap">
                <IconBrain size={20} color="#1971c2" />
                <div>
                    <Text size="sm" fw={600}>
                        {label}
                    </Text>
                    {skillId && (
                        <Badge size="xs" color="blue" variant="light" mt={2}>
                            {skillId}
                        </Badge>
                    )}
                </div>
            </Group>
        </Paper>
    );
};

export default memo(SkillNode);