/**
 * 工作流编排 API 服务
 *
 * 封装与后端工作流引擎的通信接口。
 * 目前后端 Rust CRUD 尚未实现，使用前端 mock 数据。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

/** 工作流节点类型枚举 */
export type WorkflowNodeType =
    | 'trigger'
    | 'skill'
    | 'llm'
    | 'condition'
    | 'code'
    | 'output';

/** 触发节点配置 */
export interface TriggerConfig {
    trigger_type: 'file_upload' | 'manual' | 'scheduled' | 'webhook';
    accept?: string;
    max_size_mb?: number;
}

/** Skill 节点配置 */
export interface SkillConfig {
    skill_id: string;
    skill_name?: string;
    skill_icon?: string;
    [key: string]: unknown;
}

/** LLM 节点配置 */
export interface LLMConfig {
    prompt: string;
    model?: string;
    temperature?: number;
    max_tokens?: number;
    output_schema?: Record<string, unknown>;
}

/** 条件分支节点配置 */
export interface ConditionConfig {
    field: string;
    operator: '>' | '<' | '>=' | '<=' | '==' | '!=' | 'contains' | 'is_empty';
    value: unknown;
    yes_label?: string;
    no_label?: string;
}

/** 代码节点配置 */
export interface CodeConfig {
    language: 'javascript' | 'python';
    code: string;
}

/** 工作流节点 */
export interface WorkflowNode {
    id: string;
    type: WorkflowNodeType;
    label: string;
    position: { x: number; y: number };
    config?: Record<string, unknown>;
}

/** 工作流边（连线） */
export interface WorkflowEdge {
    id: string;
    source: string;
    target: string;
    label?: string;
}

/** 工作流变量映射 */
export interface WorkflowVariable {
    [key: string]: string;
}

/** 工作流执行配置 */
export interface WorkflowConfig {
    max_execution_time?: number;
    retry_on_failure?: boolean;
    max_retries?: number;
}

/** 完整工作流定义 */
export interface WorkflowDefinition {
    name: string;
    description?: string;
    version?: string;
    nodes: WorkflowNode[];
    edges: WorkflowEdge[];
    variables?: WorkflowVariable;
    config?: WorkflowConfig;
}

/** 工作流列表项（元数据） */
export interface WorkflowMeta {
    id: string;
    name: string;
    description?: string;
    version: string;
    status: 'draft' | 'published' | 'archived';
    node_types: string[];
    use_count: number;
    last_executed_at?: string;
    created_at: string;
    updated_at: string;
}

/** 节点执行结果 */
export interface NodeExecutionResult {
    node_id: string;
    label?: string;
    status: 'pending' | 'running' | 'success' | 'failed' | 'skipped';
    input?: unknown;
    output?: unknown;
    error?: string;
    duration_ms?: number;
}

/** 工作流执行记录 */
export interface WorkflowExecution {
    id: string;
    workflow_id: string;
    workflow_name?: string;
    trigger_type: 'manual' | 'scheduled' | 'webhook';
    status: 'running' | 'success' | 'failed' | 'cancelled';
    input_data?: unknown;
    result_data?: unknown;
    error_message?: string;
    node_results?: NodeExecutionResult[];
    total_duration_ms?: number;
    total_tokens?: number;
    total_cost?: number;
    created_at: string;
    finished_at?: string;
}

// ======================== Mock 数据 ========================

const MOCK_WORKFLOWS: WorkflowMeta[] = [
    {
        id: '1',
        name: '合同智能分析',
        description: '上传合同PDF → 提取要素 → 生成摘要 → 推审批',
        version: '1.0.0',
        status: 'published',
        node_types: ['skill:doc-parse', 'llm', 'condition', 'workflow:push-approval'],
        use_count: 23,
        last_executed_at: '2026-07-21T14:30:00Z',
        created_at: '2026-06-15T10:00:00Z',
        updated_at: '2026-07-20T09:00:00Z',
    },
    {
        id: '2',
        name: '文档批量翻译',
        description: '上传文档 → 分段 → 翻译成中/英 → 合并输出',
        version: '1.0.0',
        status: 'published',
        node_types: ['skill:doc-parse', 'skill:translate-en', 'skill:translate-zh'],
        use_count: 15,
        last_executed_at: '2026-07-19T16:00:00Z',
        created_at: '2026-06-20T10:00:00Z',
        updated_at: '2026-07-18T11:00:00Z',
    },
    {
        id: '3',
        name: '知识库自动打标签',
        description: '读取新文档 → 自动生成标签 → 关联知识图谱',
        version: '0.1.0',
        status: 'draft',
        node_types: ['skill:doc-parse', 'skill:auto-tag', 'skill:discover-relations'],
        use_count: 0,
        created_at: '2026-07-10T10:00:00Z',
        updated_at: '2026-07-10T10:00:00Z',
    },
];

const MOCK_EXECUTIONS: WorkflowExecution[] = [
    {
        id: '101',
        workflow_id: '1',
        workflow_name: '合同智能分析',
        trigger_type: 'manual',
        status: 'success',
        input_data: { file_path: '/tmp/upload/contract_001.pdf' },
        result_data: { final: '分析完成' },
        node_results: [
            { node_id: 'node_1', label: '文档解析', status: 'success', duration_ms: 450, output: { text: '合同内容...', pages: 5 } },
            { node_id: 'node_2', label: '提取合同要素', status: 'success', duration_ms: 3200, output: { party_a: 'XX科技', amount: 150000 } },
            { node_id: 'node_3', label: '金额判断', status: 'success', duration_ms: 50, output: { decision: 'yes' } },
        ],
        total_duration_ms: 3700,
        total_tokens: 2500,
        created_at: '2026-07-22T10:00:00Z',
        finished_at: '2026-07-22T10:01:00Z',
    },
    {
        id: '102',
        workflow_id: '1',
        workflow_name: '合同智能分析',
        trigger_type: 'manual',
        status: 'failed',
        error_message: '文档解析超时',
        input_data: { file_path: '/tmp/upload/large_doc.pdf' },
        node_results: [
            { node_id: 'node_1', label: '文档解析', status: 'failed', duration_ms: 30000, error: '解析超时' },
        ],
        total_duration_ms: 30000,
        created_at: '2026-07-21T15:00:00Z',
        finished_at: '2026-07-21T15:00:30Z',
    },
];

const MOCK_DEFINITIONS: Record<string, WorkflowDefinition> = {
    '1': {
        name: '合同智能分析',
        description: '上传合同PDF → 提取要素 → 生成摘要 → 推审批',
        version: '1.0.0',
        nodes: [
            { id: 'node_1', type: 'trigger', label: '文件上传', position: { x: 250, y: 0 }, config: { trigger_type: 'file_upload', accept: '.pdf,.docx' } },
            { id: 'node_2', type: 'skill', label: '文档解析', position: { x: 250, y: 150 }, config: { skill_id: 'doc-parse' } },
            { id: 'node_3', type: 'llm', label: '提取合同要素', position: { x: 250, y: 300 }, config: { prompt: '提取甲方、乙方、金额、签署日期', temperature: 0.1 } },
            { id: 'node_4', type: 'condition', label: '金额判断', position: { x: 250, y: 450 }, config: { field: 'amount', operator: '>', value: 100000 } },
            { id: 'node_5', type: 'skill', label: '推审批', position: { x: 100, y: 600 }, config: { skill_id: 'asset-sync' } },
            { id: 'node_6', type: 'output', label: '输出结果', position: { x: 400, y: 600 } },
        ],
        edges: [
            { id: 'edge_1', source: 'node_1', target: 'node_2' },
            { id: 'edge_2', source: 'node_2', target: 'node_3' },
            { id: 'edge_3', source: 'node_3', target: 'node_4' },
            { id: 'edge_4', source: 'node_4', target: 'node_5', label: 'yes' },
            { id: 'edge_5', source: 'node_4', target: 'node_6', label: 'no' },
        ],
    },
    '2': {
        name: '文档批量翻译',
        description: '上传文档 → 分段 → 翻译成中/英 → 合并输出',
        version: '1.0.0',
        nodes: [
            { id: 'node_1', type: 'trigger', label: '上传文档', position: { x: 250, y: 0 } },
            { id: 'node_2', type: 'skill', label: '文档解析', position: { x: 250, y: 150 }, config: { skill_id: 'doc-parse' } },
            { id: 'node_3', type: 'skill', label: '翻译成英文', position: { x: 100, y: 300 }, config: { skill_id: 'translate-en' } },
            { id: 'node_4', type: 'skill', label: '翻译成中文', position: { x: 400, y: 300 }, config: { skill_id: 'translate-zh' } },
            { id: 'node_5', type: 'output', label: '合并输出', position: { x: 250, y: 450 } },
        ],
        edges: [
            { id: 'edge_1', source: 'node_1', target: 'node_2' },
            { id: 'edge_2', source: 'node_2', target: 'node_3' },
            { id: 'edge_3', source: 'node_2', target: 'node_4' },
            { id: 'edge_4', source: 'node_3', target: 'node_5' },
            { id: 'edge_5', source: 'node_4', target: 'node_5' },
        ],
    },
};

// ======================== 服务方法 ========================

/**
 * 获取工作流列表
 */
export async function listWorkflows(): Promise<WorkflowMeta[]> {
    try {
        // 尝试调用后端 API
        return await api.get<WorkflowMeta[]>('list_workflows');
    } catch {
        // 后端未实现，返回 mock 数据
        console.warn('[workflowService] 后端 list_workflows 未实现，使用 mock 数据');
        return MOCK_WORKFLOWS;
    }
}

/**
 * 获取单个工作流定义
 */
export async function getWorkflow(id: string): Promise<WorkflowDefinition | null> {
    try {
        return await api.get<WorkflowDefinition>('get_workflow', { id });
    } catch {
        console.warn('[workflowService] 后端 get_workflow 未实现，使用 mock 数据');
        return MOCK_DEFINITIONS[id] ?? null;
    }
}

/**
 * 保存工作流（创建/更新）
 */
export async function saveWorkflow(params: {
    id?: string;
    name: string;
    description?: string;
    definition: WorkflowDefinition;
    status?: 'draft' | 'published';
}): Promise<{ id: string; version: string }> {
    try {
        return await api.post<{ id: string; version: string }>('save_workflow', params);
    } catch {
        console.warn('[workflowService] 后端 save_workflow 未实现，返回 mock');
        return {
            id: params.id ?? String(Date.now()),
            version: '1.0.0',
        };
    }
}

/**
 * 删除工作流（软删除）
 */
export async function deleteWorkflow(id: string): Promise<void> {
    try {
        await api.post('delete_workflow', { id });
    } catch {
        console.warn('[workflowService] 后端 delete_workflow 未实现');
    }
}

/**
 * 执行工作流
 */
export async function executeWorkflow(params: {
    workflowId: string;
    inputData?: Record<string, unknown>;
}): Promise<{ executionId: string; status: string }> {
    try {
        return await api.post<{ executionId: string; status: string }>('execute_workflow', params);
    } catch {
        console.warn('[workflowService] 后端 execute_workflow 未实现，返回 mock');
        return {
            executionId: String(Date.now()),
            status: 'running',
        };
    }
}

/**
 * 获取执行详情
 */
export async function getExecution(executionId: string): Promise<WorkflowExecution | null> {
    try {
        return await api.get<WorkflowExecution>('get_execution', { executionId });
    } catch {
        console.warn('[workflowService] 后端 get_execution 未实现，使用 mock 数据');
        return MOCK_EXECUTIONS.find((e) => e.id === executionId) ?? null;
    }
}

/**
 * 获取执行历史列表
 */
export async function listExecutions(workflowId?: string): Promise<WorkflowExecution[]> {
    try {
        return await api.get<WorkflowExecution[]>('list_executions', workflowId ? { workflowId } : undefined);
    } catch {
        console.warn('[workflowService] 后端 list_executions 未实现，使用 mock 数据');
        return workflowId
            ? MOCK_EXECUTIONS.filter((e) => e.workflow_id === workflowId)
            : MOCK_EXECUTIONS;
    }
}

/**
 * 取消正在执行的工作流
 */
export async function cancelExecution(executionId: string): Promise<void> {
    try {
        await api.post('cancel_execution', { executionId });
    } catch {
        console.warn('[workflowService] 后端 cancel_execution 未实现');
    }
}