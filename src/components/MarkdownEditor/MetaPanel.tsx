'use client';
import React from 'react';
import { Stack, TextInput, Select, Group, Text, ActionIcon, Badge } from '@mantine/core';
import { IconX } from '@tabler/icons-react';
import { OKF_TYPE_OPTIONS, STATUS_OPTIONS, EDITOR_MODE_OPTIONS, type EditorMode, type OkfType } from './types';

interface MetaPanelProps {
    title: string;
    onTitleChange: (title: string) => void;
    okfType: OkfType;
    onOkfTypeChange: (type: OkfType) => void;
    summary?: string;
    onSummaryChange?: (summary: string) => void;
    source?: string;
    onSourceChange?: (source: string) => void;
    status: 'draft' | 'valid' | 'outdated';
    onStatusChange?: (status: 'draft' | 'valid' | 'outdated') => void;
    editorMode?: EditorMode;
    onEditorModeChange?: (mode: EditorMode) => void;
    tags?: string[];
    onTagsChange?: (tags: string[]) => void;
}

const MetaPanel: React.FC<MetaPanelProps> = ({
    title,
    onTitleChange,
    okfType,
    onOkfTypeChange,
    summary,
    onSummaryChange,
    source,
    onSourceChange,
    status,
    onStatusChange,
    editorMode,
    onEditorModeChange,
    tags,
    onTagsChange,
}) => {
    const [tagInput, setTagInput] = React.useState('');

    const handleAddTag = (e: React.KeyboardEvent<HTMLInputElement>) => {
        if (e.key === 'Enter' && tagInput.trim()) {
            e.preventDefault();
            const newTags = [...(tags || []), tagInput.trim()];
            onTagsChange?.(newTags);
            setTagInput('');
        }
    };

    const handleRemoveTag = (tag: string) => {
        onTagsChange?.(tags?.filter((t) => t !== tag) || []);
    };

    return (
        <Stack gap="sm">
            <TextInput
                label="标题"
                placeholder="输入知识标题"
                required
                value={title}
                onChange={(e) => onTitleChange(e.currentTarget.value)}
            />

            <Group grow>
                <Select
                    label="知识类型"
                    data={OKF_TYPE_OPTIONS}
                    value={okfType}
                    onChange={(v) => v && onOkfTypeChange(v as OkfType)}
                />
                <Select
                    label="状态"
                    data={STATUS_OPTIONS}
                    value={status}
                    onChange={(v) => v && onStatusChange?.(v as 'draft' | 'valid' | 'outdated')}
                />
                <Select
                    label="编辑器模式"
                    data={EDITOR_MODE_OPTIONS}
                    value={editorMode || 'wysiwyg'}
                    onChange={(v) => v && onEditorModeChange?.(v as EditorMode)}
                />
            </Group>

            <TextInput
                label="摘要"
                placeholder="AI 生成的知识摘要"
                value={summary || ''}
                onChange={(e) => onSummaryChange?.(e.currentTarget.value)}
            />

            <TextInput
                label="来源"
                placeholder="知识来源（URL / 文档名 / 引用）"
                value={source || ''}
                onChange={(e) => onSourceChange?.(e.currentTarget.value)}
            />

            <div>
                <Text size="sm" fw={500} mb={4}>标签</Text>
                <Group gap="xs" mb={4}>
                    {tags?.map((tag) => (
                        <Badge
                            key={tag}
                            variant="light"
                            rightSection={
                                <ActionIcon size="xs" variant="transparent" onClick={() => handleRemoveTag(tag)}>
                                    <IconX size={10} />
                                </ActionIcon>
                            }
                        >
                            {tag}
                        </Badge>
                    ))}
                </Group>
                <TextInput
                    placeholder="输入标签后按 Enter 添加"
                    value={tagInput}
                    onChange={(e) => setTagInput(e.currentTarget.value)}
                    onKeyDown={handleAddTag}
                />
            </div>
        </Stack>
    );
};

export default MetaPanel;