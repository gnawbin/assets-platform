import React from 'react';
import {
    Paper,
    Text,
    Stack,
    TextInput,
    Textarea,
    Select,
    NumberInput,
    Divider,
    Group,
    Badge,
    ScrollArea,
} from '@mantine/core';
import type { Node } from '@xyflow/react';

// ======================== 配置表单组件 ========================

interface ConfigFieldProps {
    label: string;
    children: React.ReactNode;
}

const ConfigField: React.FC<ConfigFieldProps> = ({ label, children }) => (
    <Stack gap={4}>
        <Text size="xs" fw={500} c="dimmed">
            {label}
        </Text>
        {children}
    </Stack>
);

// ======================== 节点配置面板 ========================

interface NodeConfigPanelProps {
    selectedNode: Node | null;
    onUpdateNode: (nodeId: string, data: Record<string, unknown>) => void;
    width?: number;
}

const NodeConfigPanel: React.FC<NodeConfigPanelProps> = ({ selectedNode, onUpdateNode, width = 300 }) => {
    if (!selectedNode) {
        return (
            <Paper withBorder h="100%" style={{ width, overflow: 'hidden' }} p="md">
                <Text size="sm" c="dimmed" ta="center" mt="xl">
                    点击画布上的节点<br />查看配置
                </Text>
            </Paper>
        );
    }

    const nodeData = selectedNode.data as Record<string, unknown>;
    const nodeType = selectedNode.type || '';

    const str = (val: unknown, fallback = ''): string => (val as string) ?? fallback;
    const num = (val: unknown, fallback: number): number => (val as number) ?? fallback;

    const updateField = (field: string, value: unknown) => {
        onUpdateNode(selectedNode.id, { ...nodeData, [field]: value });
    };

    const renderConfigForm = () => {
        switch (nodeType) {
            case 'trigger':
                return (
                    <>
                        <TextInput
                            label="节点名称"
                            value={str(nodeData.label)}
                            onChange={(e) => updateField('label', e.target.value)}
                            size="xs"
                        />
                        <Select
                            label="触发方式"
                            value={str(nodeData.trigger_type, 'manual')}
                            onChange={(val) => updateField('trigger_type', val)}
                            data={[
                                { value: 'manual', label: '手动触发' },
                                { value: 'file_upload', label: '文件上传' },
                                { value: 'scheduled', label: '定时触发' },
                                { value: 'webhook', label: 'Webhook' },
                            ]}
                            size="xs"
                        />
                        {str(nodeData.trigger_type) === 'file_upload' && (
                            <TextInput
                                label="文件类型"
                                value={str(nodeData.accept)}
                                onChange={(e) => updateField('accept', e.target.value)}
                                placeholder=".pdf,.docx"
                                size="xs"
                            />
                        )}
                    </>
                );

            case 'skill':
                return (
                    <>
                        <TextInput
                            label="节点名称"
                            value={str(nodeData.label)}
                            onChange={(e) => updateField('label', e.target.value)}
                            size="xs"
                        />
                        <ConfigField label="Skill ID">
                            <Badge variant="light" color="blue" size="lg">
                                {str(nodeData.skill_id, '未选择')}
                            </Badge>
                        </ConfigField>
                    </>
                );

            case 'llm':
                return (
                    <>
                        <TextInput
                            label="节点名称"
                            value={str(nodeData.label)}
                            onChange={(e) => updateField('label', e.target.value)}
                            size="xs"
                        />
                        <Textarea
                            label="提示词 (Prompt)"
                            value={str(nodeData.prompt)}
                            onChange={(e) => updateField('prompt', e.target.value)}
                            minRows={4}
                            size="xs"
                            placeholder="请输入提示词..."
                        />
                        <Select
                            label="模型"
                            value={str(nodeData.model)}
                            onChange={(val) => updateField('model', val)}
                            data={[
                                { value: '', label: '使用默认模型' },
                                { value: 'gpt-4o', label: 'GPT-4o' },
                                { value: 'claude-3.5-sonnet', label: 'Claude 3.5 Sonnet' },
                                { value: 'qwen-max', label: '通义千问 Max' },
                            ]}
                            size="xs"
                            clearable
                        />
                        <NumberInput
                            label="温度 (Temperature)"
                            value={num(nodeData.temperature, 0.1)}
                            onChange={(val) => updateField('temperature', val)}
                            min={0}
                            max={2}
                            step={0.1}
                            size="xs"
                            decimalScale={2}
                        />
                        <NumberInput
                            label="最大 Token"
                            value={num(nodeData.max_tokens, 2000)}
                            onChange={(val) => updateField('max_tokens', val)}
                            min={1}
                            max={32000}
                            size="xs"
                        />
                    </>
                );

            case 'condition':
                return (
                    <>
                        <TextInput
                            label="节点名称"
                            value={str(nodeData.label)}
                            onChange={(e) => updateField('label', e.target.value)}
                            size="xs"
                        />
                        <TextInput
                            label="判断字段"
                            value={str(nodeData.field)}
                            onChange={(e) => updateField('field', e.target.value)}
                            placeholder="amount, status, score..."
                            size="xs"
                        />
                        <Select
                            label="运算符"
                            value={str(nodeData.operator, '>')}
                            onChange={(val) => updateField('operator', val)}
                            data={[
                                { value: '>', label: '大于 (>' },
                                { value: '<', label: '小于 (<' },
                                { value: '>=', label: '大于等于 (>=' },
                                { value: '<=', label: '小于等于 (<=' },
                                { value: '==', label: '等于 (==' },
                                { value: '!=', label: '不等于 (!=' },
                                { value: 'contains', label: '包含' },
                                { value: 'is_empty', label: '为空' },
                            ]}
                            size="xs"
                        />
                        <TextInput
                            label="比较值"
                            value={str(nodeData.value)}
                            onChange={(e) => updateField('value', e.target.value)}
                            size="xs"
                        />
                        <TextInput
                            label="是标签"
                            value={str(nodeData.yes_label)}
                            onChange={(e) => updateField('yes_label', e.target.value)}
                            placeholder="yes"
                            size="xs"
                        />
                        <TextInput
                            label="否标签"
                            value={str(nodeData.no_label)}
                            onChange={(e) => updateField('no_label', e.target.value)}
                            placeholder="no"
                            size="xs"
                        />
                    </>
                );

            case 'code':
                return (
                    <>
                        <TextInput
                            label="节点名称"
                            value={str(nodeData.label)}
                            onChange={(e) => updateField('label', e.target.value)}
                            size="xs"
                        />
                        <Select
                            label="语言"
                            value={str(nodeData.language, 'javascript')}
                            onChange={(val) => updateField('language', val)}
                            data={[
                                { value: 'javascript', label: 'JavaScript' },
                                { value: 'python', label: 'Python' },
                            ]}
                            size="xs"
                        />
                        <Textarea
                            label="代码"
                            value={str(nodeData.code)}
                            onChange={(e) => updateField('code', e.target.value)}
                            minRows={8}
                            size="xs"
                            placeholder="// 编写代码..."
                            styles={{ input: { fontFamily: 'monospace', fontSize: 12 } }}
                        />
                    </>
                );

            case 'output':
                return (
                    <>
                        <TextInput
                            label="节点名称"
                            value={str(nodeData.label)}
                            onChange={(e) => updateField('label', e.target.value)}
                            size="xs"
                        />
                    </>
                );

            default:
                return <Text size="xs" c="dimmed">未知节点类型</Text>;
        }
    };

    return (
        <Paper withBorder h="100%" style={{ width, overflow: 'hidden' }}>
            <ScrollArea h="100%">
                <Stack gap="md" p="md">
                    <Group gap="xs">
                        <Badge size="sm" variant="light">
                            {nodeType.toUpperCase()}
                        </Badge>
                        <Text size="sm" fw={600} truncate>
                            {str(nodeData.label, '未命名节点')}
                        </Text>
                    </Group>

                    <Divider />

                    {renderConfigForm()}
                </Stack>
            </ScrollArea>
        </Paper>
    );
};

export default NodeConfigPanel;