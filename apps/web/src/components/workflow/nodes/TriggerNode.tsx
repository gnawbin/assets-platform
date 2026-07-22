import React, { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';
import { IconRocket } from '@tabler/icons-react';
import { Paper, Text, Group, Badge } from '@mantine/core';

const TriggerNode: React.FC<NodeProps> = ({ data, selected }) => {
    const d = data as Record<string, unknown>;
    const label = (d.label as string) || '触发';
    const triggerType = d.trigger_type as string | undefined;

    return (
        <Paper
            withBorder
            p="sm"
            radius="md"
            style={{
                borderColor: selected ? '#2b8a3e' : '#69db7c',
                borderWidth: 2,
                minWidth: 180,
                background: '#ebfbee',
            }}
        >
            <Handle type="source" position={Position.Bottom} style={{ background: '#2b8a3e' }} />
            <Group gap="xs" wrap="nowrap">
                <IconRocket size={20} color="#2b8a3e" />
                <div>
                    <Text size="sm" fw={600}>
                        {label}
                    </Text>
                    {triggerType && (
                        <Badge size="xs" color="green" variant="light" mt={2}>
                            {triggerType === 'file_upload' ? '文件上传' : triggerType}
                        </Badge>
                    )}
                </div>
            </Group>
        </Paper>
    );
};

export default memo(TriggerNode);