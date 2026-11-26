/**
 * 容器运维抽屉的统一目标描述
 *
 * 由 `asset_ops_link` 行解析而来，Docker / K8s 共用同一套面板。
 */

import {
    parseOpsResourceTarget,
    type OpsConnectionView,
    type OpsResourceLinkView,
} from '@/services/opsService';

export interface ContainerWorkTarget {
    provider: 'docker' | 'k8s';
    connectionId: string;
    connectionName: string | null;
    uid: string;
    displayName: string;
    /** Docker：容器 ID / 名称 */
    dockerContainer?: string;
    /** K8s：命名空间与 Pod */
    namespace?: string;
    pod?: string;
    /** 当前选中的容器（K8s 多容器） */
    containerName: string | null;
    /** Pod 内全部容器名（K8s 容器下拉） */
    containers: string[];
    /** 该连接是否允许终端（exec_enabled） */
    execEnabled: boolean;
}

function readContainerNames(attributes: Record<string, unknown> | null): string[] {
    const raw = attributes?.container_names;
    if (!Array.isArray(raw)) return [];
    return raw.filter((item): item is string => typeof item === 'string' && item.length > 0);
}

/**
 * 资源行 → 抽屉目标
 *
 * 解析失败（不是 container / pod 的 uid）返回 `null`，由页面展示「资源标识无法解析」。
 */
export function workTargetFromLink(
    link: OpsResourceLinkView,
    connections: OpsConnectionView[],
): ContainerWorkTarget | null {
    const parsed = parseOpsResourceTarget(link.resource_uid);
    if (!parsed) return null;
    const connection = connections.find((item) => item.id === link.connection_id) ?? null;
    const containers = readContainerNames(link.attributes);
    if (parsed.provider === 'docker') {
        return {
            provider: 'docker',
            connectionId: link.connection_id,
            connectionName: link.connection_name ?? connection?.name ?? null,
            uid: link.resource_uid,
            displayName: link.display_name ?? parsed.containerId,
            dockerContainer: parsed.containerId,
            containerName: null,
            containers: [],
            execEnabled: connection?.exec_enabled ?? false,
        };
    }
    return {
        provider: 'k8s',
        connectionId: link.connection_id,
        connectionName: link.connection_name ?? connection?.name ?? null,
        uid: link.resource_uid,
        displayName: link.display_name ?? parsed.pod,
        namespace: parsed.namespace,
        pod: parsed.pod,
        containerName: containers[0] ?? null,
        containers,
        execEnabled: connection?.exec_enabled ?? false,
    };
}
