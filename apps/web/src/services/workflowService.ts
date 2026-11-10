/**
 * 工作流编排 API 服务
 *
 * 封装与后端工作流引擎（bpmninja engine-core + adk-graph）的通信接口。
 * 所有调用直连 Tauri command / HTTP 路由，失败即抛出，由调用方决定提示方式。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

/** 工作流节点类型枚举（与后端 `workflow::types` 的 IR 保持一致） */
export type WorkflowNodeType =
    | 'trigger'
    | 'agent'
    | 'approval'
    | 'skill'
    | 'llm'
    | 'condition'
    | 'code'
    | 'workflow'
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

/** AI Agent 节点配置（adk-graph StateGraph + 工具循环） */
export interface AgentConfig {
    /** 系统提示词，决定 Agent 目标 */
    prompt: string;
    /** 可用工具名列表，如 knowledge_search / current_time */
    tools?: string[];
    /** 单次执行最大工具调用轮数 */
    max_iterations?: number;
    /** before_tool：调用工具前挂起等人工审批；never：直接执行 */
    interrupt_policy?: 'never' | 'before_tool';
    /** 覆盖 LLM 路由的模型名 */
    model?: string;
    temperature?: number;
    max_tokens?: number;
}

/** 人工审批节点配置（BPMN UserTask） */
export interface ApprovalConfig {
    /** 审批人标识，对应 UserTask assignee */
    assignee?: string;
    /** 审批提示文案 */
    title?: string;
    /** 审批说明 */
    description?: string;
    /** 审批变量名，默认 approved */
    variable?: string;
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
    /**
     * 标准 BPMN 2.0 XML（bpmn-js 导出）。
     *
     * nodes/edges 仍是后端执行 IR；bpmn_xml 是编辑器与外部 BPMN 工具之间的
     * 无损交换格式。加载时优先使用 bpmn_xml。
     */
    bpmn_xml?: string;
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
    trigger_type: string;
    /** waiting：引擎挂起中（等待审批 / 定时器 / 服务任务回调） */
    status: 'running' | 'waiting' | 'success' | 'failed' | 'cancelled';
    input_data?: unknown;
    result_data?: unknown;
    error_message?: string;
    node_results?: NodeExecutionResult[];
    total_duration_ms?: number;
    total_tokens?: number;
    total_cost?: number;
    created_at: string;
    finished_at?: string;
    /** bpmninja 引擎实例 ID */
    engine_instance_id?: string;
    /** 部署后的流程定义 key */
    engine_definition_key?: string;
    /** 当前停留节点（挂起时即等待中的节点） */
    current_node_id?: string;
    /** AI Agent 会话线程，用于恢复上下文 */
    agent_thread_id?: string;
    /** 等待中的人工任务 ID（UserTask） */
    waiting_task_id?: string;
    /** Agent 中断载荷（待审批的工具调用、提示词等） */
    interrupt_payload?: unknown;
    /** 引擎原始状态，如 waiting_user_task / completed_with_error */
    bpmn_status?: string;
}

/** 待办工作流任务（引擎 UserTask） */
export interface PendingWorkflowTask {
    task_id: string;
    instance_id: string;
    node_id: string;
    assignee: string;
    created_at: string;
    business_key?: string;
    execution_id?: string;
    workflow_id?: string;
    workflow_name?: string;
    interrupt_payload?: unknown;
}

// ======================== 服务方法 ========================

/**
 * 获取工作流列表
 */
export async function listWorkflows(params?: {
    keyword?: string;
    status?: 'draft' | 'published' | 'archived';
}): Promise<WorkflowMeta[]> {
    return api.get<WorkflowMeta[]>('list_workflows', {
        keyword: params?.keyword,
        status: params?.status,
    });
}

/**
 * 获取单个工作流定义
 *
 * 后端会用独立列回的 `bpmn_xml` / `name` / `version` 补全定义，编辑器可直接渲染。
 */
export async function getWorkflow(id: string): Promise<WorkflowDefinition | null> {
    return api.get<WorkflowDefinition | null>('get_workflow', { id });
}

/**
 * 保存工作流（创建/更新，IR 与 BPMN XML 双写）
 */
export async function saveWorkflow(params: {
    id?: string;
    name: string;
    description?: string;
    definition: WorkflowDefinition;
    status?: 'draft' | 'published';
    /** 标准 BPMN 2.0 XML（与 definition.bpmn_xml 相同，便于后端单独存储） */
    bpmnXml?: string;
    /** 工作流列表展示用的节点类型摘要 */
    node_types?: string[];
}): Promise<{ id: string; version: string }> {
    const { definition, bpmnXml, ...rest } = params;
    return api.post<{ id: string; version: string }>('save_workflow', {
        ...rest,
        definition,
        bpmnXml: bpmnXml ?? definition.bpmn_xml,
    });
}

/**
 * 删除工作流（软删除）
 */
export async function deleteWorkflow(id: string): Promise<void> {
    await api.post<void>('delete_workflow', { id });
}

/**
 * 执行工作流
 *
 * 后端会编译 BPMN → 部署定义 → 启动引擎实例；返回的 `executionId` 可直接轮询。
 */
export async function executeWorkflow(params: {
    /** 已保存工作流 ID；新建流程可先保存再执行 */
    workflowId?: string;
    /** 直接传入定义（未保存草稿可直接按 IR + BPMN XML 执行） */
    definition?: WorkflowDefinition;
    inputData?: Record<string, unknown>;
    /** 执行记录标题，默认取 definition.name */
    title?: string;
}): Promise<{ executionId: string; status: string }> {
    return api.post<{ executionId: string; status: string }>('execute_workflow', {
        workflowId: params.workflowId,
        definition: params.definition,
        inputData: params.inputData,
        title: params.title,
    });
}

/**
 * 获取执行详情
 */
export async function getExecution(executionId: string): Promise<WorkflowExecution | null> {
    return api.get<WorkflowExecution | null>('get_execution', { executionId });
}

/**
 * 获取执行历史列表
 */
export async function listExecutions(
    workflowId?: string,
    options?: { page?: number; pageSize?: number },
): Promise<WorkflowExecution[]> {
    return api.get<WorkflowExecution[]>('list_executions', {
        workflowId,
        page: options?.page,
        pageSize: options?.pageSize,
    });
}

/**
 * 取消正在执行的工作流
 */
export async function cancelExecution(executionId: string): Promise<void> {
    await api.post<void>('cancel_execution', { executionId });
}

/**
 * 获取待办审批任务
 */
export async function listPendingTasks(params?: {
    workflowId?: string;
    executionId?: string;
}): Promise<PendingWorkflowTask[]> {
    return api.get<PendingWorkflowTask[]>('list_pending_tasks', {
        workflowId: params?.workflowId,
        executionId: params?.executionId,
    });
}

/**
 * 完成人工审批任务
 *
 * `variables` 约定：`approved`（布尔）与 `comment`（字符串）。
 * 通过后会唤醒挂起的引擎实例（含 AI Agent 的工具调用中断）。
 */
export async function completeUserTask(params: {
    taskId: string;
    variables?: Record<string, unknown>;
}): Promise<void> {
    await api.post<void>('complete_user_task', {
        taskId: params.taskId,
        variables: params.variables,
    });
}

