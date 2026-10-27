/**
 * assets 扩展节点的序列化契约测试。
 *
 * 这里用最小的 moddle 工厂替身，只验证“写业务对象”这一层的行为：
 * - 审批人必须通过 set('data-assignee', …) 落到 $attrs，否则 bpmn-moddle 导出时会丢字段
 *   （标准 bpmn:UserTask 没有 assignee 属性，直接赋值不会进入 $attrs）；
 * - AI 扩展节点用标准 ServiceTask + implementation + assets:EngineConfig 表达，
 *   默认值与后端 AgentConfig 保持一致。
 */

import {
    createBusinessObjectForPreset,
    createEngineConfig,
    normalizeEngineAttrs,
} from '@/components/workflow/bpmn/modeler/createElements';
import { getPreset } from '@/components/workflow/bpmn/mapping/nodeKinds';
import { findEngineConfig, readEngineFields, readUserTaskAssignee } from '@/components/workflow/bpmn/mapping/helpers';
import { ENGINE_CONFIG_TYPE } from '@/components/workflow/bpmn/moddle/assetsModdle';

/** 模拟 bpmn-moddle 的业务对象：未知属性走 set() 进入 $attrs */
function createFakeFactory() {
    return {
        create(type: string, attrs: Record<string, unknown> = {}) {
            const element: any = { $type: type, $attrs: {}, ...attrs };
            element.set = (name: string, value: unknown) => {
                if (value === undefined || value === null || value === '') {
                    delete element.$attrs[name];
                } else {
                    element.$attrs[name] = value;
                }
            };
            return element;
        },
    };
}

describe('审批节点：审批人写入 data-assignee', () => {
    it('同时写入 assets:EngineConfig.assignee 与 data-assignee 属性', () => {
        const factory = createFakeFactory();
        const preset = getPreset('approval');

        expect(preset).toBeDefined();

        const businessObject = createBusinessObjectForPreset(preset!, factory, {
            engine: { assignee: 'legal-team', approvalVariable: 'approved' },
        });

        // 标准 BPMN 元素 + 扩展执行参数
        expect(businessObject.$type).toBe('bpmn:UserTask');
        expect(businessObject.$attrs['data-assignee']).toBe('legal-team');
        expect(readUserTaskAssignee(businessObject)).toBe('legal-team');

        const config = findEngineConfig(businessObject);
        expect(config?.$type).toBe(ENGINE_CONFIG_TYPE);
        expect(readEngineFields(businessObject)).toMatchObject({
            kind: 'approval',
            assignee: 'legal-team',
            approvalVariable: 'approved',
        });
    });

    it('没有审批人时不写入 data-assignee', () => {
        const factory = createFakeFactory();
        const businessObject = createBusinessObjectForPreset(getPreset('approval')!, factory);

        expect(businessObject.$attrs).not.toHaveProperty('data-assignee');
        expect(readUserTaskAssignee(businessObject)).toBe('');
        expect(readEngineFields(businessObject).assignee).toBeUndefined();
    });

    it('清空审批人时移除属性（避免导出 data-assignee=""）', () => {
        const factory = createFakeFactory();
        const businessObject = createBusinessObjectForPreset(getPreset('approval')!, factory, {
            engine: { assignee: 'ops' },
        });

        businessObject.set('data-assignee', undefined);

        expect(businessObject.$attrs).not.toHaveProperty('data-assignee');
    });
});

describe('AI Agent 节点：默认值与后端 AgentConfig 对齐', () => {
    it('生成 ##Agent 的 ServiceTask 并写入默认执行参数', () => {
        const factory = createFakeFactory();
        const preset = getPreset('agent');

        expect(preset).toBeDefined();

        const businessObject = createBusinessObjectForPreset(preset!, factory);

        expect(businessObject.$type).toBe('bpmn:ServiceTask');
        expect(businessObject.implementation).toBe('##Agent');
        expect(readEngineFields(businessObject)).toMatchObject({
            kind: 'agent',
            maxIterations: 6,
            interruptPolicy: 'never',
        });
    });

    it('LLM / Skill 节点仍然映射到各自的 implementation', () => {
        const factory = createFakeFactory();
        const llm = createBusinessObjectForPreset(getPreset('llm')!, factory);
        const skill = createBusinessObjectForPreset(getPreset('skill')!, factory);

        expect(llm.implementation).toBe('##LLM');
        expect(readEngineFields(llm).kind).toBe('llm');
        expect(skill.implementation).toBe('##Skill');
        expect(readEngineFields(skill).kind).toBe('skill');
    });
});

describe('扩展参数序列化', () => {
    it('过滤空值，避免导出空属性', () => {
        expect(normalizeEngineAttrs({ kind: 'llm', model: '', temperature: undefined, maxTokens: 512 })).toEqual({
            kind: 'llm',
            maxTokens: 512,
        });
    });

    it('数字与布尔值原样传给 moddle', () => {
        const factory = createFakeFactory();
        const config = createEngineConfig(factory, { kind: 'agent', maxIterations: 8, temperature: 0.2 });

        expect(config.$type).toBe(ENGINE_CONFIG_TYPE);
        expect(config.maxIterations).toBe(8);
        expect(config.temperature).toBe(0.2);
    });
});
