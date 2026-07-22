import TriggerNode from './TriggerNode';
import SkillNode from './SkillNode';
import LLMNode from './LLMNode';
import ConditionNode from './ConditionNode';
import CodeNode from './CodeNode';
import OutputNode from './OutputNode';

export const nodeTypes: Record<string, React.ComponentType<any>> = {
    trigger: TriggerNode,
    skill: SkillNode,
    llm: LLMNode,
    condition: ConditionNode,
    code: CodeNode,
    output: OutputNode,
};

export { default as TriggerNode } from './TriggerNode';
export { default as SkillNode } from './SkillNode';
export { default as LLMNode } from './LLMNode';
export { default as ConditionNode } from './ConditionNode';
export { default as CodeNode } from './CodeNode';
export { default as OutputNode } from './OutputNode';