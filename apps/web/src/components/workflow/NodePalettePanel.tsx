'use client';

import React, { useEffect, useState } from 'react';
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
import { defaultEngineForPreset, NODE_PRESETS, type NodePreset } from './bpmn/mapping/nodeKinds';
import type { AssetsDragPayload, EngineFields } from './bpmn/types';

interface NodePalettePanelProps {
    onInsert: (presetId: string, extra?: { engine?: Partial<EngineFields>; name?: string }) => void;
    disabled?: boolean;
}

interface PaletteItemProps {
    icon: string;
    label: string;
    description: string;
    color: string;
    payload: AssetsDragPayload;
    disabled?: boolean;
    onClick: () => void;
}

function PaletteItem({ icon, label, description, color, payload, disabled, onClick }: PaletteItemProps) {
    const handleDragStart = (event: React.DragEvent<HTMLDivElement>) => {
        if (disabled) {
            event.preventDefault();
            return;
        }
        event.dataTransfer.setData('application/assets-node', JSON.stringify(payload));
        event.dataTransfer.effectAllowed = 'copy';
    };

    return (
        <Paper
            withBorder
            p="xs"
            radius="sm"
            draggable={!disabled}
            onDragStart={handleDragStart}
            onClick={onClick}
            style={{
                cursor: disabled ? 'not-allowed' : 'grab',
                borderLeft: `3px solid ${color}`,
                opacity: disabled ? 0.6 : 1,
            }}
        >
            <Group gap="sm" wrap="nowrap">
                <Text size="xl" style={{ lineHeight: 1 }}>
                    {icon}
                </Text>
                <Box style={{ flex: 1, minWidth: 0 }}>
                    <Text size="sm" fw={500} lineClamp={1}>
                        {label}
                    </Text>
                    <Text size="xs" c="dimmed" lineClamp={2}>
                        {description}
                    </Text>
                </Box>
            </Group>
        </Paper>
    );
}

function presetItemProps(
    preset: NodePreset,
    disabled: boolean | undefined,
    onInsert: NodePalettePanelProps['onInsert']
): PaletteItemProps {
    const engine = defaultEngineForPreset(preset);
    return {
        icon: preset.icon,
        label: preset.label,
        description: preset.description,
        color: preset.color,
        disabled,
        payload: { presetId: preset.id, name: preset.label, engine },
        onClick: () => onInsert(preset.id, { engine, name: preset.label }),
    };
}

/**
 * 左侧节点面板：业务模板 + Skill 库。
 *
 * 所有条目都会插入标准 BPMN 元素（ServiceTask / Gateway / ScriptTask ...），
 * 而不是自定义节点。
 */
export default function NodePalettePanel({ onInsert, disabled }: NodePalettePanelProps) {
    const [skills, setSkills] = useState<SkillMeta[]>([]);
    const [loading, setLoading] = useState(true);
    const [keyword, setKeyword] = useState('');

    useEffect(() => {
        let mounted = true;
        listSkills()
            .then((items) => {
                if (mounted) setSkills(Array.isArray(items) ? items : []);
            })
            .catch(() => {
                if (mounted) setSkills([]);
            })
            .finally(() => {
                if (mounted) setLoading(false);
            });
        return () => {
            mounted = false;
        };
    }, []);

    const filteredSkills = skills.filter((skill) => {
        const text = keyword.trim().toLowerCase();
        if (!text) return true;
        return (
            skill.name.toLowerCase().includes(text) ||
            skill.id.toLowerCase().includes(text) ||
            (skill.description || '').toLowerCase().includes(text)
        );
    });

    const systemPresets = NODE_PRESETS.filter((preset) => preset.id !== 'skill');

    return (
        <Paper withBorder h="100%" style={{ display: 'flex', flexDirection: 'column', minHeight: 0 }}>
            <Box p="sm">
                <Text size="sm" fw={600}>
                    节点面板
                </Text>
                <Text size="xs" c="dimmed">
                    拖拽到画布，生成标准 BPMN 元素
                </Text>
            </Box>
            <Divider />
            <ScrollArea style={{ flex: 1, minHeight: 0 }} p="sm">
                <Stack gap="xs">
                    <Text size="xs" fw={600} c="dimmed">
                        流程控制
                    </Text>
                    {systemPresets.map((preset) => (
                        <PaletteItem
                            key={preset.id}
                            {...presetItemProps(preset, disabled, onInsert)}
                        />
                    ))}

                    <Divider my={4} />

                    <Group justify="space-between">
                        <Text size="xs" fw={600} c="dimmed">
                            Skill 库
                        </Text>
                        <Badge size="xs" variant="light">
                            {skills.length}
                        </Badge>
                    </Group>
                    <TextInput
                        size="xs"
                        placeholder="搜索 Skill"
                        leftSection={<IconSearch size={14} />}
                        value={keyword}
                        onChange={(event) => setKeyword(event.currentTarget.value)}
                    />

                    {loading && (
                        <Group gap="xs" py="sm">
                            <Loader size="xs" />
                            <Text size="xs" c="dimmed">
                                加载中...
                            </Text>
                        </Group>
                    )}
                    {!loading && filteredSkills.length === 0 && (
                        <Text size="xs" c="dimmed" py="xs">
                            没有可用的 Skill
                        </Text>
                    )}
                    {filteredSkills.map((skill) => {
                        const engine: Partial<EngineFields> = {
                            kind: 'skill',
                            skillId: skill.id,
                            skillName: skill.name,
                        };
                        return (
                            <PaletteItem
                                key={skill.id}
                                icon={skill.icon || '🧩'}
                                label={skill.name}
                                description={skill.description || skill.id}
                                color="#1971c2"
                                disabled={disabled}
                                payload={{ presetId: 'skill', name: skill.name, engine }}
                                onClick={() => onInsert('skill', { engine, name: skill.name })}
                            />
                        );
                    })}
                </Stack>
            </ScrollArea>
        </Paper>
    );
}

