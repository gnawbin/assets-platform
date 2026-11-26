/**
 * 异步任务 API 服务（任务中心页 / 知识资产解析入口）
 *
 * 后端路由：`crates/api/src/task_routes.rs`
 * - `POST /api/tasks/parse`  解析入队（202）
 * - `GET  /api/tasks`        任务列表（status / limit）
 * - `GET  /api/tasks/{id}`   单任务进度
 *
 * 注意：`assetId` 全程使用 **字符串**（后端 i64 直出会被 JS 丢精度，
 * 见 docs/前端大整数精度丢失问题(Snowflake ID).md）。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

/** 任务状态（与后端 `task_ticket.status` 一致） */
export type TaskStatus = 'pending' | 'running' | 'completed' | 'failed';

/** 任务阶段（worker 按阶段推进） */
export type TaskStage = 'queued' | 'downloading' | 'parsing' | 'chunking' | 'done';

/** 任务视图（后端 `TaskItem`） */
export interface TaskItem {
    /** 任务 ID（UUID） */
    taskId: string;
    /** 任务类型：parse / vectorize */
    taskType: string;
    /** 队列名：parse / parse-video / vectorize */
    queue: string;
    /** 关联资产 ID（字符串，可能为 null） */
    assetId: string | null;
    /** 状态 */
    status: TaskStatus;
    /** 阶段 */
    stage: TaskStage | null;
    /** 进度 0~100 */
    progress: number;
    /** 已尝试次数 */
    attempts: number;
    /** 最近一次失败原因 */
    error: string | null;
    /** 调用链追踪 ID */
    traceId: string | null;
    /** 发起人 ID（字符串） */
    createdBy: string | null;
    /** 入队时间（RFC3339） */
    createdAt: string;
    /** 开始时间（RFC3339） */
    startedAt: string | null;
    /** 结束时间（RFC3339） */
    finishedAt: string | null;
    /** 最近更新时间（RFC3339） */
    updatedAt: string;
}

/** 发起解析任务入参 */
export interface PushParseTaskParams {
    /** 关联资产 ID（字符串） */
    assetId: string;
    /** 对象存储引用：s3://bucket/key */
    fileUrl: string;
    /** 原文件名（决定队列与解析类型） */
    fileName: string;
    /** 资产标题 */
    title?: string;
    /** OKF 类型 */
    okfType?: string;
    /** 标签 */
    tags?: string[];
    /** 知识树节点 ID */
    treeNodeId?: string | null;
    /** 文件 MD5（幂等键） */
    fileMd5?: string | null;
    /** 解析选项（skip_index / frame_interval 等） */
    options?: Record<string, unknown>;
    /** 调用链追踪 ID */
    traceId?: string | null;
}

/** 入队结果 */
export interface PushParseTaskResult {
    taskId: string;
    assetId: string;
    status: string;
}

// ======================== API 方法 ========================

/**
 * 任务列表（按创建时间倒序）
 *
 * 空参数不会拼进 query string（避免 `?status=undefined`）。
 */
export function listParseTasks(params: { status?: TaskStatus; limit?: number } = {}) {
    const args: Record<string, unknown> = {};
    if (params.status) args.status = params.status;
    if (params.limit) args.limit = params.limit;
    return api.get<TaskItem[]>('list_parse_tasks', Object.keys(args).length ? args : undefined);
}

/** 查询单任务进度 */
export function getParseTask(taskId: string) {
    return api.get<TaskItem>('get_parse_task', { taskId });
}

/** 解析任务入队（后端返回 202） */
export function pushParseTask(params: PushParseTaskParams) {
    return api.post<PushParseTaskResult>('push_parse_task', {
        assetId: params.assetId,
        fileUrl: params.fileUrl,
        fileName: params.fileName,
        title: params.title,
        okfType: params.okfType,
        tags: params.tags ?? [],
        treeNodeId: params.treeNodeId ?? null,
        fileMd5: params.fileMd5 ?? null,
        options: params.options ?? {},
        traceId: params.traceId ?? null,
    });
}

// ======================== 展示辅助 ========================

/** 状态 → 中文标签 */
export const TASK_STATUS_LABEL: Record<TaskStatus, string> = {
    pending: '排队中',
    running: '执行中',
    completed: '已完成',
    failed: '失败',
};

/** 状态 → Mantine 颜色 */
export const TASK_STATUS_COLOR: Record<TaskStatus, string> = {
    pending: 'gray',
    running: 'blue',
    completed: 'teal',
    failed: 'red',
};

/** 阶段 → 中文标签 */
export const TASK_STAGE_LABEL: Record<TaskStage, string> = {
    queued: '已入队',
    downloading: '下载文件',
    parsing: '解析中',
    chunking: '分块/向量化',
    done: '完成',
};

/** 任务是否已进入终态（终态无需继续轮询） */
export function isTaskTerminal(status: string): boolean {
    return status === 'completed' || status === 'failed';
}
