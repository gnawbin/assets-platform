/**
 * `assets` BPMN 扩展命名空间。
 *
 * 只承载执行器参数（skillId / prompt / triggerType ...），不新增节点类型。
 * `bpmn:extensionElements` 是 BPMN 2.0 规范定义的扩展容器，导出的文件仍是
 * 标准 BPMN 2.0 XML，可在 bpmn.io / Camunda Modeler 中正常打开。
 */

export const ASSETS_PREFIX = 'assets';
export const ASSETS_URI = 'https://assets-platform.local/schema/bpmn/assets';

export const assetsModdle = {
    name: 'Assets',
    uri: ASSETS_URI,
    prefix: ASSETS_PREFIX,
    xml: {
        tagAlias: 'lowerCase',
    },
    types: [
        {
            name: 'EngineConfig',
            superClass: ['Element'],
            properties: [
                { name: 'kind', isAttr: true, type: 'String' },
                { name: 'skillId', isAttr: true, type: 'String' },
                { name: 'skillName', isAttr: true, type: 'String' },
                { name: 'triggerType', isAttr: true, type: 'String' },
                { name: 'accept', isAttr: true, type: 'String' },
                { name: 'maxSizeMb', isAttr: true, type: 'Integer' },
                { name: 'model', isAttr: true, type: 'String' },
                { name: 'temperature', isAttr: true, type: 'Number' },
                { name: 'maxTokens', isAttr: true, type: 'Integer' },
                { name: 'field', isAttr: true, type: 'String' },
                { name: 'operator', isAttr: true, type: 'String' },
                { name: 'value', isAttr: true, type: 'String' },
                { name: 'yesLabel', isAttr: true, type: 'String' },
                { name: 'noLabel', isAttr: true, type: 'String' },
                { name: 'wfType', isAttr: true, type: 'String' },
                { name: 'prompt', isAttr: true, type: 'String' },
                { name: 'outputSchema', isAttr: true, type: 'String' },
                { name: 'params', isAttr: true, type: 'String' },
                // AI Agent 节点
                { name: 'tools', isAttr: true, type: 'String' },
                { name: 'maxIterations', isAttr: true, type: 'Integer' },
                { name: 'interruptPolicy', isAttr: true, type: 'String' },
                // 人工审批节点（bpmn:UserTask）
                { name: 'assignee', isAttr: true, type: 'String' },
                { name: 'approvalTitle', isAttr: true, type: 'String' },
                { name: 'approvalDescription', isAttr: true, type: 'String' },
                { name: 'approvalVariable', isAttr: true, type: 'String' },
            ],
        },
    ],
};

export const ENGINE_CONFIG_TYPE = 'assets:EngineConfig';
