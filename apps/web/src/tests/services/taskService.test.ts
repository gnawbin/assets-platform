/**
 * taskService ↔ 后端路由契约测试
 *
 * 断言四件事：
 * 1. 3 个命令都已登记 `COMMAND_ROUTE_MAP`，且 method / path 与
 *    `crates/api/src/task_routes.rs` 一致；
 * 2. `listParseTasks` 的参数落在 query string（GET 不发明文 body）；
 * 3. `pushParseTask` 的 body 键名与后端 serde 的 camelCase 一致（`assetId` / `fileUrl`），
 *    且 `assetId` 是**字符串**（避免 JS 大整数精度丢失）；
 * 4. 展示辅助（状态标签 / 终态判断）与后端状态字面量一致。
 */

import { COMMAND_ROUTE_MAP } from '@/utils/adapters/httpAdapter';
import {
    TASK_STAGE_LABEL,
    TASK_STATUS_LABEL,
    getParseTask,
    isTaskTerminal,
    listParseTasks,
    pushParseTask,
} from '@/services/taskService';

const API_ORIGIN = 'http://test.local';

/** 构造成功的 `ApiResponse<T>` 响应 */
function okResponse(data: unknown): Response {
    return {
        ok: true,
        status: 200,
        statusText: 'OK',
        json: jest.fn().mockResolvedValue({ code: 0, message: 'ok', data }),
        text: jest.fn().mockResolvedValue(''),
    } as unknown as Response;
}

/** 取第 n 次 fetch 调用的 (url, init) */
function callAt(index = 0): [string, RequestInit] {
    const calls = (global.fetch as jest.Mock).mock.calls;
    return [calls[index][0], calls[index][1] as RequestInit];
}

type ApiWindow = Window & { __API_BASE__?: unknown };

describe('taskService 与 COMMAND_ROUTE_MAP 契约', () => {
    beforeEach(() => {
        (window as ApiWindow).__API_BASE__ = API_ORIGIN;
        localStorage.clear();
        (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(okResponse([]));
    });

    afterEach(() => {
        delete (window as ApiWindow).__API_BASE__;
    });

    it('3 个命令都已登记 HTTP 路由', () => {
        expect(COMMAND_ROUTE_MAP.push_parse_task).toEqual({
            method: 'POST',
            path: '/api/tasks/parse',
        });
        expect(COMMAND_ROUTE_MAP.list_parse_tasks).toEqual({
            method: 'GET',
            path: '/api/tasks',
        });
        expect(COMMAND_ROUTE_MAP.get_parse_task).toEqual({
            method: 'GET',
            path: '/api/tasks/{taskId}',
        });
    });

    it('listParseTasks 无参时不带 query，有参时拼 query string', async () => {
        await listParseTasks();
        expect(callAt()[0]).toBe(`${API_ORIGIN}/api/tasks`);
        expect(callAt()[1].method).toBe('GET');

        (global.fetch as jest.Mock).mockClear();
        await listParseTasks({ status: 'running', limit: 10 });
        expect(callAt()[0]).toBe(`${API_ORIGIN}/api/tasks?status=running&limit=10`);
    });

    it('getParseTask 把 taskId 放进路径', async () => {
        await getParseTask('task-123');
        expect(callAt()[0]).toBe(`${API_ORIGIN}/api/tasks/task-123`);
        expect(callAt()[1].method).toBe('GET');
    });

    it('pushParseTask 使用 camelCase body 且 assetId 为字符串', async () => {
        await pushParseTask({
            assetId: '9007199254740993',
            fileUrl: 's3://bucket/a.mp4',
            fileName: 'a.mp4',
            title: '演示视频',
            okfType: 'raw_source',
            tags: ['demo'],
            fileMd5: 'abc123',
        });

        expect(callAt()[0]).toBe(`${API_ORIGIN}/api/tasks/parse`);
        expect(callAt()[1].method).toBe('POST');

        const body = JSON.parse(String(callAt()[1].body)) as Record<string, unknown>;
        expect(body.assetId).toBe('9007199254740993');
        expect(typeof body.assetId).toBe('string');
        expect(body.fileUrl).toBe('s3://bucket/a.mp4');
        expect(body.fileName).toBe('a.mp4');
        expect(body.title).toBe('演示视频');
        expect(body.okfType).toBe('raw_source');
        expect(body.tags).toEqual(['demo']);
        expect(body.fileMd5).toBe('abc123');
        // 未传时给出确定的空值（后端 serde 默认值兜底）
        expect(body.treeNodeId).toBeNull();
        expect(body.options).toEqual({});
    });

    it('状态/阶段/终态辅助与后端字面量一致', () => {
        expect(TASK_STATUS_LABEL.pending).toBe('排队中');
        expect(TASK_STATUS_LABEL.running).toBe('执行中');
        expect(TASK_STATUS_LABEL.completed).toBe('已完成');
        expect(TASK_STATUS_LABEL.failed).toBe('失败');

        expect(TASK_STAGE_LABEL.downloading).toBe('下载文件');
        expect(TASK_STAGE_LABEL.chunking).toBe('分块/向量化');

        expect(isTaskTerminal('completed')).toBe(true);
        expect(isTaskTerminal('failed')).toBe(true);
        expect(isTaskTerminal('running')).toBe(false);
        expect(isTaskTerminal('pending')).toBe(false);
    });
});
