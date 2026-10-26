'use client';

import React from 'react';
import {
    Alert,
    Badge,
    Center,
    Code,
    Divider,
    Group,
    Paper,
    ScrollArea,
    Select,
    Stack,
    Text,
} from '@mantine/core';
import { IconAlertTriangle, IconInfoCircle } from '@tabler/icons-react';
import type { ElementPatch, SelectedBpmnElement } from './bpmn/types';
import {
    KIND_LABELS,
    isActivityType,
    isGatewayType,
    REPLACEABLE_ACTIVITY_TYPES,
    REPLACEABLE_GATEWAY_TYPES,
} from './bpmn/mapping/nodeKinds';
import EngineFieldsPanel from './bpmn/EngineFieldsPanel';
import { ConfigField, TextField } from './bpmn/formFields';

interface NodeConfigPanelProps {
    element: SelectedBpmnElement | null;
    onUpdate: (patch: ElementPatch) => void;
    width?: number;
}

function StandardSection({
    element,
    onUpdate,
}: {
    element: SelectedBpmnElement;
    onUpdate: (patch: ElementPatch) => void;
}) {
    const isStartEvent = element.type === 'bpmn:StartEvent';
    const isServiceTask = element.type === 'bpmn:ServiceTask';
    const isCallActivity = element.type === 'bpmn:CallActivity';
    const isSequenceFlow = element.type === 'bpmn:SequenceFlow';

    const replaceable = isActivityType(element.type) || isGatewayType(element.type);
    const baseTypes = isGatewayType(element.type)
        ? REPLACEABLE_GATEWAY_TYPES
        : REPLACEABLE_ACTIVITY_TYPES;
    const typeOptions = replaceable
        ? baseTypes.some((option) => option.value === element.type)
            ? baseTypes
            : [{ value: element.type, label: element.type }, ...baseTypes]
        : [];

    const standardEvents = [
        { value: 'none', label: '无（None）', disabled: false },
        { value: 'timer', label: '定时（Timer）', disabled: false },
        { value: 'message', label: '消息（Message）', disabled: false },
    ];
    const currentEvent = element.eventDefinition || 'none';
    const eventOptions = standardEvents.some((option) => option.value === currentEvent)
        ? standardEvents
        : [
              {
                  value: currentEvent,
                  label: `${currentEvent}（执行器按 none 处理）`,
                  disabled: true,
              },
              ...standardEvents,
          ];

    return (
        <Stack gap="sm">
            <TextField
                label="名称"
                value={element.name}
                onCommit={(value) => onUpdate({ type: 'name', value })}
            />
            <TextField
                label="说明"
                value={element.documentation}
                textarea
                minRows={2}
                placeholder="bpmn:documentation"
                onCommit={(value) => onUpdate({ type: 'documentation', value })}
            />

            {typeOptions.length > 0 && !isSequenceFlow && (
                <ConfigField label="元素类型" hint="切换为标准 BPMN 类型；语义不兼容时执行器会忽略该节点">
                    <Select
                        size="xs"
                        data={typeOptions}
                        value={element.type}
                        onChange={(value) => {
                            if (value && value !== element.type) {
                                onUpdate({ type: 'replace', value });
                            }
                        }}
                    />
                </ConfigField>
            )}

            {isStartEvent && (
                <ConfigField label="事件定义" hint="timer / message 是标准 BPMN 事件定义">
                    <Select
                        size="xs"
                        data={eventOptions}
                        value={currentEvent}
                        onChange={(value) => {
                            if (value) {
                                onUpdate({
                                    type: 'triggerEvent',
                                    value: value as 'none' | 'timer' | 'message',
                                });
                            }
                        }}
                    />
                </ConfigField>
            )}

            {isServiceTask && (
                <ConfigField label="implementation" hint="标准 BPMN 服务任务属性">
                    <Select
                        size="xs"
                        data={Array.from(
                            new Set(['##Skill', '##LLM', element.implementation || '##Skill'])
                        ).map((value) => ({ value, label: value }))}
                        value={element.implementation || '##Skill'}
                        onChange={(value) =>
                            onUpdate({ type: 'implementation', value: value || '##Skill' })
                        }
                    />
                </ConfigField>
            )}

            {isCallActivity && (
                <TextField
                    label="calledElement"
                    value={element.calledElement}
                    placeholder="被调用的流程 ID"
                    onCommit={(value) => onUpdate({ type: 'calledElement', value })}
                />
            )}

            {isSequenceFlow && (
                <Alert variant="light" color="blue" icon={<IconInfoCircle size={16} />} p="xs">
                    连线标签可直接在画布上双击编辑；条件表达式请选中上游网关后设置。
                </Alert>
            )}
        </Stack>
    );
}

export default function NodeConfigPanel({ element, onUpdate, width = 320 }: NodeConfigPanelProps) {
    if (!element) {
        return (
            <Paper withBorder h="100%" w={width} style={{ display: 'flex', flexDirection: 'column' }}>
                <Center style={{ flex: 1 }} p="md">
                    <Stack align="center" gap={4}>
                        <Text size="sm" fw={500}>
                            未选中元素
                        </Text>
                        <Text size="xs" c="dimmed" ta="center">
                            在画布中选择元素以编辑标准 BPMN 属性与执行参数
                        </Text>
                    </Stack>
                </Center>
            </Paper>
        );
    }

    return (
        <Paper withBorder h="100%" w={width} style={{ display: 'flex', flexDirection: 'column', minHeight: 0 }}>
            <Stack gap={6} p="sm">
                <Group gap="xs" wrap="nowrap">
                    <Badge size="sm" variant="light" color={element.executable ? 'blue' : 'gray'}>
                        {KIND_LABELS[element.kind]}
                    </Badge>
                    <Text size="sm" fw={600} lineClamp={1} style={{ flex: 1 }}>
                        {element.name}
                    </Text>
                </Group>
                <Code style={{ fontSize: 11 }}>{element.id}</Code>
                <Text size="xs" c="dimmed">
                    {element.type}
                </Text>
            </Stack>
            <Divider />

            <ScrollArea style={{ flex: 1, minHeight: 0 }} p="sm">
                <Stack gap="md">
                    {!element.executable && (
                        <Alert
                            variant="light"
                            color="yellow"
                            icon={<IconAlertTriangle size={16} />}
                            title="不参与执行"
                            p="xs"
                        >
                            该元素会保留在标准 BPMN XML 中，但不会进入执行 IR。可切换为
                            ServiceTask / ScriptTask / ExclusiveGateway / CallActivity
                            等受支持的类型。
                        </Alert>
                    )}

                    <Divider label="BPMN 标准属性" labelPosition="left" />
                    <StandardSection element={element} onUpdate={onUpdate} />

                    {element.executable && element.kind !== 'output' && (
                        <>
                            <Divider label="执行参数" labelPosition="left" />
                            <Text size="xs" c="dimmed">
                                写入 bpmn:extensionElements → assets:engineConfig（标准扩展点）。
                            </Text>
                            <EngineFieldsPanel element={element} onUpdate={onUpdate} />
                        </>
                    )}

                    {element.kind === 'output' && (
                        <Alert variant="light" color="gray" icon={<IconInfoCircle size={16} />} p="xs">
                            结束事件是流程终点，不进入执行 IR；其入线会作为上游节点的终止标记。
                        </Alert>
                    )}
                </Stack>
            </ScrollArea>
        </Paper>
    );
}
