import React, { useState, useEffect } from 'react';
import {
    Paper,
    Text,
    TextInput,
    Stack,
    Group,
    Badge,
    ScrollArea,
    Divider,
    Box,
    Loader,
} from '@mantine/core';
import { IconSearch } from '@tabler/icons-react';
import { listSkills, type SkillMeta } from '@/services/skillService';

// ======================== 节点类型定义（非 Skill 系统节点） ========================

interface SystemNodeType {
    type: string;
    label: string;
    icon: string;
    color: string;
    description: string;
    defaultConfig?: Record<string, unknown>;
}

const SYSTEM_NODE_TYPES: SystemNodeType[] = [
    {
        type: 'trigger',
        label: '触发节点',
        icon: '🚀',
        color: '#2b8a3e',
        description: '工作流起点（手动/文件/定时）',
        defaultConfig: { trigger_type: 'manual' },
    },
    {
        type: 'llm',
        label: 'LLM 节点',
        icon: '🧠',
        color: '#9c36b5',
        description: '调用大语言模型',
        defaultConfig: { prompt: '', temperature: 0.1 },
    },
    {
        type: 'condition',
        label: '条件分支',
        icon: '🔀',
        color: '#e8590c',
        description: 'if/else 条件判断',
        defaultConfig: { field: '', operator: '>', value: 0 },
    },
    {
        type: 'code',
        label: '代码节点',
        icon: '💻',
        color: '#495057',
        description: '沙箱执行代码片段',
        defaultConfig: { language: 'javascript', code: '' },
    },
    {
        type: 'output',
        label: '输出节点',
        icon: '📤',
        color: '#c92a2a',
        description: '工作流终点',
    },
];

// ======================== 可拖拽节点项 ========================

interface DraggableItemProps {
    type: string;
    label: string;
    icon: string;
    color: string;
    description: string;
    config?: Record<string, unknown>;
}

const DraggableItem: React.FC<DraggableItemProps> = ({ type, label, icon, color, description }) => {
    const onDragStart = (event: React.DragEvent<HTMLDivElement>) => {
        const nodeData = JSON.stringify({
            type,
            label,
            config: type === 'skill' ? { skill_id: label } : undefined,
        });
        event.dataTransfer.setData('application/reactflow', nodeData);
        event.dataTransfer.effectAllowed = 'move';
    };

    return (
        <Paper
            withBorder
            p="xs"
            radius="sm"
            draggable
            onDragStart={onDragStart}
            style={{
                cursor: 'grab',
                borderLeft: `3px solid ${color}`,
                transition: 'background-color 150ms ease',
            }}
            onMouseEnter={(e) => {
                (e.currentTarget as HTMLElement).style.backgroundColor = 'var(--mantine-color-gray-0)';
            }}
            onMouseLeave={(e) => {
                (e.currentTarget as HTMLElement).style.backgroundColor = 'transparent';
            }}
        >
            <Group gap="sm" wrap="nowrap">
                <Text size="xl" style={{ lineHeight: 1 }}>
                    {icon}
                </Text>
                <div style={{ flex: 1, minWidth: 0 }}>
                    <Text size="sm" fw={500}>
                        {label}
                    </Text>
                    <Text size="xs" c="dimmed" lineClamp={1}>
                        {description}
                    </Text>
                </div>
            </Group>
        </Paper>
    );
};

// ======================== 主面板组件 ========================

interface SkillNodePanelProps {
    /** 当前宽度（由父组件控制，默认 260） */
    width?: number;
}

const SkillNodePanel: React.FC<SkillNodePanelProps> = ({ width = 260 }) => {
    const [skills, setSkills] = useState<SkillMeta[]>([]);
    const [loading, setLoading] = useState(true);
    const [searchQuery, setSearchQuery] = useState('');

    useEffect(() => {
        let mounted = true;
        (async () => {
            try {
                const data = await listSkills();
                if (mounted) setSkills(data);
            } catch {
                // ignore
            } finally {
                if (mounted) setLoading(false);
            }
        })();
        return () => {
            mounted = false;
        };
    }, []);

    const filteredSkills = skills.filter(
        (s) =>
            s.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
            s.description.toLowerCase().includes(searchQuery.toLowerCase())
    );

    return (
        <Paper withBorder h="100%" style={{ width, overflow: 'hidden' }}>
            <ScrollArea h="100%">
                <Stack gap="xs" p="sm">
                    <Text size="sm" fw={600}>
                        节点面板
                    </Text>
                    <TextInput
                        placeholder="搜索节点..."
                        size="xs"
                        leftSection={<IconSearch size={14} />}
                        value={searchQuery}
                        onChange={(e) => setSearchQuery(e.target.value)}
                    />

                    <Divider label="系统节点" labelPosition="left" />
                    {SYSTEM_NODE_TYPES.map((node) => (
                        <DraggableItem key={node.type} {...node} />
                    ))}

                    <Divider label="Skill 节点" labelPosition="left" />
                    {loading ? (
                        <Group justify="center" py="md">
                            <Loader size="sm" />
                        </Group>
                    ) : filteredSkills.length === 0 ? (
                        <Text size="xs" c="dimmed" ta="center" py="md">
                            {searchQuery ? '未找到匹配的 Skill' : '暂无可用 Skill'}
                        </Text>
                    ) : (
                        filteredSkills.map((skill) => (
                            <DraggableItem
                                key={skill.id}
                                type="skill"
                                label={skill.name}
                                icon={skill.icon}
                                color="#1971c2"
                                description={skill.description}
                                config={{ skill_id: skill.id, skill_name: skill.name, skill_icon: skill.icon }}
                            />
                        ))
                    )}

                    <Box pb="md">
                        <Text size="xs" c="dimmed" ta="center">
                            拖拽节点到画布开始编排
                        </Text>
                    </Box>
                </Stack>
            </ScrollArea>
        </Paper>
    );
};

export default SkillNodePanel;