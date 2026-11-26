'use client';

import React, { useEffect, useState } from 'react';
import {
    Alert,
    Badge,
    Button,
    Divider,
    Group,
    NumberInput,
    SegmentedControl,
    Select,
    Stack,
    Text,
} from '@mantine/core';
import { IconInfoCircle } from '@tabler/icons-react';
import type { ConditionFields, ElementPatch, SelectedBpmnElement } from './types';
import { parseConditionExpression } from './mapping/helpers';
import { CONDITION_OPERATORS } from './mapping/helpers';
import { ConfigField, TextField } from './formFields';
import ApprovalAssigneePicker from '../ApprovalAssigneePicker';
import { getDepartments, type Department } from '@/services/departmentService';
import { useAuthStore } from '@/store/authStore';

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
function ApprovalEditor({
    element,
    onUpdate,
}: {
    element: SelectedBpmnElement;
    onUpdate: (patch: ElementPatch) => void;
}) {
    const tenantId = useAuthStore(
        (state) => state.selectedTenantId ?? state.user?.tenant_id ?? null
    );
    const [departments, setDepartments] = useState<Department[]>([]);

    useEffect(() => {
        let mounted = true;
        getDepartments(tenantId)
            .then((list) => {
                if (mounted) setDepartments(Array.isArray(list) ? list : []);
            })
            .catch(() => {
                if (mounted) setDepartments([]);
            });
        return () => {
            mounted = false;
        };
    }, [tenantId]);

    const engine = element.engine;
    const mode = engine.assigneeMode || 'user';
    const approvalType = engine.approvalType || 'single';
    const ids = (engine.assigneeIds || engine.assignee || '')
        .split(',')
        .map((item) => item.trim())
        .filter(Boolean);
    const names = (engine.assigneeNames || engine.assigneeName || '')
        .split(',')
        .map((item) => item.trim())
        .filter(Boolean);

    const update = (value: Record<string, unknown>) =>
        onUpdate({ type: 'engine', value: value as never });

    const changeApprovalType = (value: string) => {
        if (value === 'single') {
            update({
                approvalType: value,
                assignee: ids[0] || '',
                assigneeIds: ids[0] || '',
                assigneeName: names[0] || '',
                assigneeNames: names[0] || '',
            });
        } else {
            update({ approvalType: value });
        }
    };

    const missingHint =
        mode === 'user' && ids.length === 0
            ? '请选择审批人'
            : mode === 'department' && !engine.assigneeDeptId
              ? '请选择部门'
              : mode === 'form' && !engine.assigneeFormField
                ? '请填写表单字段路径'
                : null;

    return (
        <Stack gap="sm">
            {missingHint && (
                <Alert variant="light" color="yellow" p="xs">
                    {missingHint}
                </Alert>
            )}

            <ConfigField label="审批方式">
                <SegmentedControl
                    size="xs"
                    fullWidth
                    data={[
                        { value: 'user', label: '指定用户' },
                        { value: 'department', label: '指定部门' },
                        { value: 'form', label: '表单字段' },
                    ]}
                    value={mode}
                    onChange={(value) => update({ assigneeMode: value })}
                />
            </ConfigField>

            {mode === 'user' && (
                <ApprovalAssigneePicker
                    value={ids}
                    valueNames={names}
                    multiple={approvalType !== 'single'}
                    tenantId={tenantId}
                    onChange={(nextIds, options) =>
                        update({
                            assignee: nextIds[0] || '',
                            assigneeIds: nextIds.join(','),
                            assigneeName: options[0]?.name || '',
                            assigneeNames: options.map((item) => item.name).join(','),
                        })
                    }
                />
            )}

            {mode === 'department' && (
                <>
                    <ConfigField label="部门">
                        <Select
                            size="xs"
                            placeholder="请选择部门"
                            searchable
                            clearable
                            data={departments.map((dept) => ({
                                value: String(dept.id),
                                label: dept.department_name,
                            }))}
                            value={engine.assigneeDeptId || null}
                            onChange={(value) => {
                                const dept = departments.find(
                                    (item) => String(item.id) === value
                                );
                                update({
                                    assigneeDeptId: value || '',
                                    assigneeDeptName: dept?.department_name || '',
                                });
                            }}
                        />
                    </ConfigField>
                    <ConfigField label="部门范围">
                        <SegmentedControl
                            size="xs"
                            fullWidth
                            data={[
                                { value: 'leader', label: '部门负责人' },
                                { value: 'members', label: '部门成员' },
                            ]}
                            value={engine.assigneeDeptScope || 'leader'}
                            onChange={(value) =>
                                update({
                                    assigneeDeptScope: value,
                                    ...(value === 'members' && approvalType === 'single'
                                        ? { approvalType: 'all' }
                                        : {}),
                                })
                            }
                        />
                    </ConfigField>
                </>
            )}

            {mode === 'form' && (
                <TextField
                    label="表单字段路径"
                    value={engine.assigneeFormField}
                    placeholder="form.approver_id / approver_ids"
                    hint="执行 inputData 或流程变量的顶层字段 / 点路径；数组按审批类型解析"
                    onCommit={(value) => update({ assigneeFormField: value })}
                />
            )}

            <ConfigField label="审批类型">
                <SegmentedControl
                    size="xs"
                    fullWidth
                    data={[
                        { value: 'single', label: '单人' },
                        { value: 'all', label: '会签' },
                        { value: 'any', label: '或签' },
                    ]}
                    value={approvalType}
                    onChange={changeApprovalType}
                />
            </ConfigField>

            <ConfigField label="拒绝后">
                <SegmentedControl
                    size="xs"
                    fullWidth
                    data={[
                        { value: 'terminate', label: '终止流程' },
                        { value: 'continue', label: '继续' },
                    ]}
                    value={
                        engine.rejectAction ||
                        (mode === 'user' && approvalType === 'single'
                            ? 'continue'
                            : 'terminate')
                    }
                    onChange={(value) => update({ rejectAction: value })}
                />
            </ConfigField>

            <TextField
                label="任务标题"
                value={engine.approvalTitle}
                placeholder="请审批"
                onCommit={(value) => update({ approvalTitle: value })}
            />
            <TextField
                label="任务说明"
                value={engine.approvalDescription}
                textarea
                minRows={2}
                onCommit={(value) => update({ approvalDescription: value })}
            />
            <TextField
                label="审批结果变量"
                value={engine.approvalVariable}
                placeholder="approved"
                hint="complete_user_task 的 variables.approved 会写回该变量"
                onCommit={(value) => update({ approvalVariable: value })}
            />
            <Alert variant="light" color="blue" p="xs">
                会签：全部通过才通过，任一拒绝终止；或签：任一通过即通过，全部拒绝才终止。
            </Alert>
        </Stack>
    );
}

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
        return <ApprovalEditor element={element} onUpdate={onUpdate} />;
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


