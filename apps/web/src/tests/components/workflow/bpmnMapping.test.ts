import {
    collectNodeTypes,
    engineToIrNode,
    engineToNodeConfig,
    formatConditionExpression,
    formatLiteral,
    orderNodesForExecution,
    parseConditionExpression,
    parseLiteral,
    validateDefinition,
} from '@/components/workflow/bpmn/mapping/helpers';
import type { WorkflowDefinition } from '@/services/workflowService';

function baseDefinition(overrides: Partial<WorkflowDefinition> = {}): WorkflowDefinition {
    return {
        name: '测试工作流',
        version: '1.0.0',
        nodes: [{ id: 'start', type: 'trigger', label: '开始', position: { x: 0, y: 0 } }],
        edges: [],
        ...overrides,
    };
}

describe('BPMN 条件表达式映射', () => {
    it('解析比较表达式', () => {
        expect(parseConditionExpression('${amount > 100000}')).toEqual({
            field: 'amount',
            operator: '>',
            value: '100000',
        });
        expect(parseConditionExpression('${status == "done"}')).toEqual({
            field: 'status',
            operator: '==',
            value: 'done',
        });
        expect(parseConditionExpression('count >= 3')).toEqual({
            field: 'count',
            operator: '>=',
            value: '3',
        });
    });

    it('解析 contains / isEmpty 函数', () => {
        expect(parseConditionExpression("${customer.contains('vip')}")).toEqual({
            field: 'customer',
            operator: 'contains',
            value: 'vip',
        });
        expect(parseConditionExpression("${contains(tags, 'urgent')}")).toEqual({
            field: 'tags',
            operator: 'contains',
            value: 'urgent',
        });
        expect(parseConditionExpression('${isEmpty(remark)}')).toEqual({
            field: 'remark',
            operator: 'is_empty',
            value: '',
        });
    });

    it('无法解析时返回 null', () => {
        expect(parseConditionExpression('')).toBeNull();
        expect(parseConditionExpression('${a && b}')).toBeNull();
        expect(parseConditionExpression(undefined)).toBeNull();
    });

    it('生成标准 conditionExpression', () => {
        expect(formatConditionExpression('amount', '>', 100000)).toBe('${amount > 100000}');
        expect(formatConditionExpression('status', '==', 'done')).toBe("${status == 'done'}");
        expect(formatConditionExpression('tags', 'contains', 'urgent')).toBe(
            "${contains(tags, 'urgent')}"
        );
        expect(formatConditionExpression('remark', 'is_empty', '')).toBe('${isEmpty(remark)}');
        expect(formatConditionExpression('', '>', 1)).toBe('');
    });

    it('字面量解析与序列化', () => {
        expect(parseLiteral('100')).toBe(100);
        expect(parseLiteral('true')).toBe(true);
        expect(parseLiteral('null')).toBeNull();
        expect(parseLiteral("'abc'")).toBe('abc');
        expect(parseLiteral('{"a":1}')).toEqual({ a: 1 });

        expect(formatLiteral(100)).toBe('100');
        expect(formatLiteral(false)).toBe('false');
        expect(formatLiteral(null)).toBe('null');
        expect(formatLiteral("it's")).toBe("'it\\'s'");
    });
});

describe('IR 映射辅助函数', () => {
    it('Skill 参数合并到 config', () => {
        expect(
            engineToNodeConfig('skill', {
                kind: 'skill',
                skillId: 'doc-parse',
                skillName: '文档解析',
                params: '{"top_k":5}',
            })
        ).toEqual({
            skill_id: 'doc-parse',
            skill_name: '文档解析',
            top_k: 5,
        });
    });

    it('LLM / workflow / trigger 配置', () => {
        expect(
            engineToNodeConfig('llm', {
                prompt: '提取要素',
                model: 'gpt-4o-mini',
                temperature: 0.1,
                maxTokens: 1024,
                outputSchema: '{"type":"object"}',
            })
        ).toMatchObject({
            prompt: '提取要素',
            model: 'gpt-4o-mini',
            temperature: 0.1,
            max_tokens: 1024,
            output_schema: { type: 'object' },
        });

        expect(engineToNodeConfig('workflow', { wfType: 'contract-analysis' })).toEqual({
            wf_type: 'contract-analysis',
        });

        expect(
            engineToNodeConfig('trigger', {
                triggerType: 'file_upload',
                accept: '.pdf',
                maxSizeMb: 20,
            })
        ).toEqual({
            trigger_type: 'file_upload',
            accept: '.pdf',
            max_size_mb: 20,
        });
    });
});

describe('节点集合与排序', () => {
    it('node_types 展示摘要', () => {
        const definition = baseDefinition({
            nodes: [
                { id: 'start', type: 'trigger', label: '开始', position: { x: 0, y: 0 } },
                {
                    id: 's1',
                    type: 'skill',
                    label: '解析',
                    position: { x: 0, y: 100 },
                    config: { skill_id: 'doc-parse' },
                },
                {
                    id: 's2',
                    type: 'skill',
                    label: '解析2',
                    position: { x: 0, y: 200 },
                    config: { skill_id: 'doc-parse' },
                },
                {
                    id: 'w1',
                    type: 'workflow',
                    label: '子流程',
                    position: { x: 0, y: 300 },
                    config: { wf_type: 'contract-analysis' },
                },
            ],
        });

        expect(collectNodeTypes(definition)).toEqual([
            'trigger',
            'skill:doc-parse',
            'workflow:contract-analysis',
        ]);
    });

    it('拓扑排序让开始事件排在最前', () => {
        const definition = baseDefinition({
            nodes: [
                {
                    id: 'skill',
                    type: 'skill',
                    label: '解析',
                    position: { x: 0, y: 100 },
                    config: { skill_id: 'doc-parse' },
                },
                { id: 'start', type: 'trigger', label: '开始', position: { x: 0, y: 0 } },
            ],
            edges: [{ id: 'e1', source: 'start', target: 'skill' }],
        });

        expect(orderNodesForExecution(definition.nodes, definition.edges).map((n) => n.id)).toEqual([
            'start',
            'skill',
        ]);
    });
});

describe('执行 IR 校验', () => {
    it('缺少开始事件与 Skill 时给出错误', () => {
        const definition = baseDefinition({
            nodes: [
                {
                    id: 'skill',
                    type: 'skill',
                    label: '解析',
                    position: { x: 0, y: 0 },
                    config: {},
                },
            ],
        });

        const issues = validateDefinition(definition);
        expect(
            issues.some((issue) => issue.level === 'error' && issue.message.includes('开始事件'))
        ).toBe(true);
        expect(
            issues.some((issue) => issue.level === 'error' && issue.message.includes('未选择 Skill'))
        ).toBe(true);
    });

    it('条件分支至少需要两条出线', () => {
        const definition = baseDefinition({
            nodes: [
                { id: 'start', type: 'trigger', label: '开始', position: { x: 0, y: 0 } },
                {
                    id: 'cond',
                    type: 'condition',
                    label: '金额判断',
                    position: { x: 0, y: 100 },
                    config: { field: 'amount', operator: '>', value: 100 },
                },
            ],
            edges: [{ id: 'e1', source: 'start', target: 'cond' }],
        });

        const issues = validateDefinition(definition);
        expect(
            issues.some(
                (issue) =>
                    issue.level === 'error' &&
                    issue.elementId === 'cond' &&
                    issue.message.includes('两条出线')
            )
        ).toBe(true);
    });
});


describe('执行器占位提示与终止节点', () => {
    it('占位执行器给出 warning', () => {
        const definition = baseDefinition({
            nodes: [
                { id: 'start', type: 'trigger', label: '开始', position: { x: 0, y: 0 } },
                {
                    id: 'skill',
                    type: 'skill',
                    label: '解析',
                    position: { x: 0, y: 100 },
                    config: { skill_id: 'doc-parse' },
                },
            ],
            edges: [{ id: 'e1', source: 'start', target: 'skill' }],
        });

        expect(
            validateDefinition(definition).some(
                (issue) => issue.level === 'warning' && issue.message.includes('占位实现')
            )
        ).toBe(true);
    });

    it('结束事件终止的节点不报“没有出线”', () => {
        const definition = baseDefinition({
            nodes: [
                { id: 'start', type: 'trigger', label: '开始', position: { x: 0, y: 0 } },
                {
                    id: 'skill',
                    type: 'skill',
                    label: '解析',
                    position: { x: 0, y: 100 },
                    config: { skill_id: 'doc-parse' },
                },
            ],
            edges: [{ id: 'e1', source: 'start', target: 'skill' }],
        });

        const withoutTerminal = validateDefinition(definition);
        expect(withoutTerminal.some((issue) => issue.message.includes('没有出线'))).toBe(true);

        const withTerminal = validateDefinition(definition, { terminalNodeIds: ['skill'] });
        expect(withTerminal.some((issue) => issue.message.includes('没有出线'))).toBe(false);
    });
});


describe('BPMN 元素投影为 IR 节点', () => {
    it('timer 开始事件映射为 scheduled', () => {
        const node = engineToIrNode({
            id: 'StartEvent_1',
            kind: 'trigger',
            label: '定时触发',
            position: { x: 250, y: 0 },
            engine: {},
            eventDefinition: 'timer',
        });

        expect(node.type).toBe('trigger');
        expect(node.config).toMatchObject({ trigger_type: 'scheduled' });
    });

    it('ServiceTask + assets 参数映射为 LLM 节点', () => {
        const node = engineToIrNode({
            id: 'Task_1',
            kind: 'llm',
            label: '提取要素',
            position: { x: 250, y: 150 },
            engine: {
                prompt: '提取合同要素',
                model: 'qwen2.5',
                temperature: 0.1,
                outputSchema: '{"type":"object"}',
            },
        });

        expect(node.type).toBe('llm');
        expect(node.config).toMatchObject({
            prompt: '提取合同要素',
            model: 'qwen2.5',
            temperature: 0.1,
            output_schema: { type: 'object' },
        });
    });

    it('ScriptTask 语言与脚本写入 code 节点', () => {
        const node = engineToIrNode({
            id: 'ScriptTask_1',
            kind: 'code',
            label: '计算',
            position: { x: 0, y: 0 },
            engine: {},
            scriptFormat: 'javascript',
            script: 'input.amount * 2',
        });

        expect(node.type).toBe('code');
        expect(node.config).toEqual({
            language: 'javascript',
            code: 'input.amount * 2',
        });
    });
});

