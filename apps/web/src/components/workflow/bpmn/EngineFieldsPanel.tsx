'use client';

import React, { useState } from 'react';
import {
    Alert,
    Badge,
    Button,
    Divider,
    Group,
    NumberInput,
    Select,
    Stack,
    Text,
} from '@mantine/core';
import { IconInfoCircle } from '@tabler/icons-react';
import type { ConditionFields, ElementPatch, SelectedBpmnElement } from './types';
import { parseConditionExpression } from './mapping/helpers';
import { CONDITION_OPERATORS } from './mapping/helpers';
import { ConfigField, TextField } from './formFields';

interface EngineFieldsPanelProps {
    element: SelectedBpmnElement;
    onUpdate: (patch: ElementPatch) => void;
}

function initialCondition(element: SelectedBpmnElement): ConditionFields {
    const parsed = parseConditionExpression(
        element.outgoing.find((flow) => flow.condition)?.condition
    );
    return {
        field: element.engine.field || parsed?.field || '',
        operator: element.engine.operator || parsed?.operator || '>',
        value:
            element.engine.value !== undefined
                ? element.engine.value
                : parsed
                  ? String(parsed.value)
                  : '',
        yesLabel: element.engine.yesLabel || 'yes',
        noLabel: element.engine.noLabel || 'no',
    };
}

function ConditionEditor({ element, onUpdate }: EngineFieldsPanelProps) {
    const [condition, setCondition] = useState<ConditionFields>(() => initialCondition(element));

    return (
        <Stack gap="sm">
            <Alert variant="light" color="orange" icon={<IconInfoCircle size={16} />} p="xs">
                条件会写入标准 BPMN 的 conditionExpression；第一个出线为 yes，第二个为 no
                （并设为 default flow）。
            </Alert>

            <TextField
                label="判断字段"
                value={condition.field}
                placeholder="例如 amount"
                onCommit={(value) => setCondition((prev) => ({ ...prev, field: value }))}
            />
            <ConfigField label="运算符">
                <Select
                    size="xs"
                    data={CONDITION_OPERATORS.map((operator) => ({
                        value: operator,
                        label: operator,
                    }))}
                    value={condition.operator}
                    onChange={(value) =>
                        setCondition((prev) => ({ ...prev, operator: value || '>' }))
                    }
                />
            </ConfigField>
            <TextField
                label="比较值"
                value={condition.value}
                placeholder="100000 / done / true"
                onCommit={(value) => setCondition((prev) => ({ ...prev, value }))}
            />
            <Group grow>
                <TextField
                    label="yes 出线名称"
                    value={condition.yesLabel}
                    onCommit={(value) =>
                        setCondition((prev) => ({ ...prev, yesLabel: value || 'yes' }))
                    }
                />
                <TextField
                    label="no 出线名称"
                    value={condition.noLabel}
                    onCommit={(value) =>
                        setCondition((prev) => ({ ...prev, noLabel: value || 'no' }))
                    }
                />
            </Group>

            <Button
                size="xs"
                variant="light"
                onClick={() => onUpdate({ type: 'condition', value: condition })}
            >
                应用到网关与出线
            </Button>

            <Divider label="当前出线" labelPosition="left" />
            {element.outgoing.length === 0 && (
                <Text size="xs" c="dimmed">
                    还没有出线，请从网关拖出两条连线。
                </Text>
            )}
            <Stack gap={4}>
                {element.outgoing.map((flow) => (
                    <Group key={flow.id} gap="xs" wrap="nowrap">
                        <Badge size="xs" variant="light">
                            {flow.name || '(未命名)'}
                        </Badge>
                        <Text size="xs" c="dimmed" lineClamp={1}>
                            {flow.condition || '默认分支'}
                        </Text>
                    </Group>
                ))}
            </Stack>
        </Stack>
    );
}


/**
 * 执行参数面板：只写 `assets:EngineConfig`，不改变 BPMN 元素类型。
 */
export default function EngineFieldsPanel({ element, onUpdate }: EngineFieldsPanelProps) {
    if (element.kind === 'condition') {
        return <ConditionEditor element={element} onUpdate={onUpdate} />;
    }

    if (element.kind === 'trigger') {
        return (
            <Stack gap="sm">
                <ConfigField label="触发方式" hint="定时/Webhook 会同步标准的 timer / message 事件定义">
                    <Select
                        size="xs"
                        data={[
                            { value: 'manual', label: '手动触发' },
                            { value: 'file_upload', label: '文件上传' },
                            { value: 'scheduled', label: '定时触发' },
                            { value: 'webhook', label: 'Webhook' },
                        ]}
                        value={element.engine.triggerType || 'manual'}
                        onChange={(value) =>
                            onUpdate({
                                type: 'engine',
                                value: { triggerType: value || 'manual' },
                            })
                        }
                    />
                </ConfigField>
                <TextField
                    label="接受的文件类型"
                    value={element.engine.accept}
                    placeholder=".pdf,.docx"
                    onCommit={(value) => onUpdate({ type: 'engine', value: { accept: value } })}
                />
                <ConfigField label="单文件大小上限（MB）">
                    <NumberInput
                        size="xs"
                        min={0}
                        value={element.engine.maxSizeMb}
                        onChange={(value) =>
                            onUpdate({
                                type: 'engine',
                                value: {
                                    maxSizeMb: typeof value === 'number' ? value : undefined,
                                },
                            })
                        }
                    />
                </ConfigField>
            </Stack>
        );
    }

    if (element.kind === 'skill') {
        return (
            <Stack gap="sm">
                {!element.engine.skillId && (
                    <Alert variant="light" color="red" p="xs">
                        请选择 Skill，否则执行时会校验失败。
                    </Alert>
                )}
                <TextField
                    label="Skill ID"
                    value={element.engine.skillId}
                    placeholder="doc-parse"
                    onCommit={(value) => onUpdate({ type: 'engine', value: { skillId: value } })}
                />
                <TextField
                    label="Skill 名称"
                    value={element.engine.skillName}
                    onCommit={(value) => onUpdate({ type: 'engine', value: { skillName: value } })}
                />
                <TextField
                    label="扩展参数"
                    value={element.engine.params}
                    textarea
                    minRows={3}
                    placeholder={'{"top_k": 5}'}
                    hint="JSON，会合并进节点 config"
                    onCommit={(value) => onUpdate({ type: 'engine', value: { params: value } })}
                />
                <Alert variant="light" color="yellow" p="xs">
                    后端 Skill 执行器目前是占位实现，保存与导出不受影响。
                </Alert>
            </Stack>
        );
    }

    if (element.kind === 'agent') {
        return (
            <Stack gap="sm">
                {!element.engine.prompt && (
                    <Alert variant="light" color="red" p="xs">
                        Agent 需要系统指令（Prompt），否则执行时会校验失败。
                    </Alert>
                )}
                <TextField
                    label="系统指令（Prompt）"
                    value={element.engine.prompt}
                    textarea
                    minRows={5}
                    placeholder="你是一名合同审阅助手，请结合知识库回答……"
                    onCommit={(value) => onUpdate({ type: 'engine', value: { prompt: value } })}
                />
                <TextField
                    label="可用工具"
                    value={element.engine.tools}
                    placeholder="knowledge_search,current_time"
                    hint="逗号分隔；留空使用默认工具集"
                    onCommit={(value) => onUpdate({ type: 'engine', value: { tools: value } })}
                />
                <Group grow>
                    <ConfigField label="最大迭代次数">
                        <NumberInput
                            size="xs"
                            min={1}
                            max={30}
                            value={element.engine.maxIterations ?? 6}
                            onChange={(value) =>
                                onUpdate({
                                    type: 'engine',
                                    value: {
                                        maxIterations: typeof value === 'number' ? value : undefined,
                                    },
                                })
                            }
                        />
                    </ConfigField>
                    <ConfigField label="中断策略" hint="未设置时后端按「调用工具前挂起」执行">
                        <Select
                            size="xs"
                            data={[
                                { value: 'never', label: '不中断' },
                                { value: 'before_tool', label: '调用工具前挂起' },
                                { value: 'before_final', label: '输出前挂起' },
                            ]}
                            value={element.engine.interruptPolicy || 'before_tool'}
                            onChange={(value) =>
                                onUpdate({
                                    type: 'engine',
                                    value: { interruptPolicy: value || 'before_tool' },
                                })
                            }
                        />
                    </ConfigField>
                </Group>
                <Group grow>
                    <TextField
                        label="模型"
                        value={element.engine.model}
                        placeholder="gpt-4o-mini"
                        onCommit={(value) => onUpdate({ type: 'engine', value: { model: value } })}
                    />
                    <ConfigField label="温度">
                        <NumberInput
                            size="xs"
                            min={0}
                            max={2}
                            step={0.1}
                            value={element.engine.temperature}
                            onChange={(value) =>
                                onUpdate({
                                    type: 'engine',
                                    value: {
                                        temperature: typeof value === 'number' ? value : undefined,
                                    },
                                })
                            }
                        />
                    </ConfigField>
                </Group>
                <Alert variant="light" color="blue" p="xs">
                    Agent 由 adk-graph 运行工具循环；选择「调用工具前挂起」时，审批通过
                    complete_user_task 后才会继续执行工具。
                </Alert>
            </Stack>
        );
    }

    if (element.kind === 'approval') {
        return (
            <Stack gap="sm">
                {!element.engine.assignee && (
                    <Alert variant="light" color="yellow" p="xs">
                        未设置审批人时，任务会进入未分配队列。
                    </Alert>
                )}
                <TextField
                    label="审批人"
                    value={element.engine.assignee}
                    placeholder="admin / 用户 ID"
                    hint="写入 bpmn:UserTask data-assignee 与节点 config，引擎按此生成待办"
                    onCommit={(value) => onUpdate({ type: 'engine', value: { assignee: value } })}
                />
                <TextField
                    label="任务标题"
                    value={element.engine.approvalTitle}
                    placeholder="请审批"
                    onCommit={(value) =>
                        onUpdate({ type: 'engine', value: { approvalTitle: value } })
                    }
                />
                <TextField
                    label="任务说明"
                    value={element.engine.approvalDescription}
                    textarea
                    minRows={2}
                    onCommit={(value) =>
                        onUpdate({ type: 'engine', value: { approvalDescription: value } })
                    }
                />
                <TextField
                    label="审批结果变量"
                    value={element.engine.approvalVariable}
                    placeholder="approved"
                    hint="complete_user_task 的 variables.approved 会写回该变量"
                    onCommit={(value) =>
                        onUpdate({ type: 'engine', value: { approvalVariable: value } })
                    }
                />
                <Alert variant="light" color="blue" p="xs">
                    流程运行到该节点会挂起为 waiting；审批通过后引擎从当前节点继续。
                </Alert>
            </Stack>
        );
    }

    if (element.kind === 'llm') {
        return (
            <Stack gap="sm">
                <TextField
                    label="Prompt"
                    value={element.engine.prompt}
                    textarea
                    minRows={4}
                    placeholder="请从输入中提取……"
                    onCommit={(value) => onUpdate({ type: 'engine', value: { prompt: value } })}
                />
                <TextField
                    label="模型"
                    value={element.engine.model}
                    placeholder="gpt-4o-mini"
                    onCommit={(value) => onUpdate({ type: 'engine', value: { model: value } })}
                />
                <Group grow>
                    <ConfigField label="Temperature">
                        <NumberInput
                            size="xs"
                            min={0}
                            max={2}
                            step={0.1}
                            decimalScale={2}
                            value={element.engine.temperature}
                            onChange={(value) =>
                                onUpdate({
                                    type: 'engine',
                                    value: {
                                        temperature: typeof value === 'number' ? value : undefined,
                                    },
                                })
                            }
                        />
                    </ConfigField>
                    <ConfigField label="Max Tokens">
                        <NumberInput
                            size="xs"
                            min={1}
                            value={element.engine.maxTokens}
                            onChange={(value) =>
                                onUpdate({
                                    type: 'engine',
                                    value: {
                                        maxTokens: typeof value === 'number' ? value : undefined,
                                    },
                                })
                            }
                        />
                    </ConfigField>
                </Group>
                <TextField
                    label="输出 Schema"
                    value={element.engine.outputSchema}
                    textarea
                    minRows={3}
                    placeholder={'{"type": "object"}'}
                    hint="JSON Schema，可选"
                    onCommit={(value) =>
                        onUpdate({ type: 'engine', value: { outputSchema: value } })
                    }
                />
            </Stack>
        );
    }

    if (element.kind === 'code') {
        return (
            <Stack gap="sm">
                <ConfigField label="脚本语言">
                    <Select
                        size="xs"
                        data={[
                            { value: 'python', label: 'Python' },
                            { value: 'javascript', label: 'JavaScript' },
                        ]}
                        value={element.scriptFormat === 'javascript' ? 'javascript' : 'python'}
                        onChange={(value) =>
                            onUpdate({
                                type: 'script',
                                scriptFormat: value || 'python',
                                script: element.script || '',
                            })
                        }
                    />
                </ConfigField>
                <TextField
                    label="代码"
                    value={element.script}
                    textarea
                    minRows={8}
                    placeholder={'def run(inputs):\n    return inputs'}
                    hint="脚本内容写入标准 bpmn:script"
                    onCommit={(value) =>
                        onUpdate({
                            type: 'script',
                            scriptFormat: element.scriptFormat || 'python',
                            script: value,
                        })
                    }
                />
            </Stack>
        );
    }

    if (element.kind === 'workflow') {
        return (
            <Stack gap="sm">
                <TextField
                    label="子工作流类型"
                    value={element.engine.wfType}
                    placeholder="例如 contract-analysis"
                    hint="写入标准 calledElement 与 assets:wfType"
                    onCommit={(value) => onUpdate({ type: 'engine', value: { wfType: value } })}
                />
                <Alert variant="light" color="yellow" p="xs">
                    后端子工作流执行器目前是占位实现，保存与导出不受影响。
                </Alert>
            </Stack>
        );
    }

    return null;
}


