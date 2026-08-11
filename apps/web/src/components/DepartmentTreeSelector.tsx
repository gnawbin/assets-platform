'use client';
import React, { useMemo, useState } from 'react';
import {
    TextInput,
    Modal,
    Stack,
    Box,
    Text,
    ActionIcon,
    ScrollArea,
    Group,
    Badge,
    Button,
} from '@mantine/core';
import {
    IconFolder,
    IconFolderOpen,
    IconChevronRight,
    IconChevronDown,
    IconSearch,
    IconBuilding,
} from '@tabler/icons-react';
import type { Department } from '@/services/departmentService';

// ======================== 树节点接口 ========================

interface TreeNode {
    id: string;
    department_name: string;
    parent_id: string | null;
    children: TreeNode[];
    expanded: boolean;
}

// ======================== 组件 Props ========================

interface DepartmentTreeSelectorProps {
    departments: Department[];
    value: string[];
    onChange: (value: string[]) => void;
    label?: string;
    placeholder?: string;
    required?: boolean;
}

// ======================== 工具函数 ========================

/** 构建树结构 */
function buildTree(depts: Department[]): TreeNode[] {
    const map = new Map<string, TreeNode>();
    const roots: TreeNode[] = [];

    depts.forEach((dept) => {
        map.set(dept.id, {
            id: dept.id,
            department_name: dept.department_name,
            parent_id: dept.parent_id,
            children: [],
            expanded: true,
        });
    });

    depts.forEach((dept) => {
        const node = map.get(dept.id)!;
        if (dept.parent_id && dept.parent_id !== '0' && map.has(dept.parent_id)) {
            map.get(dept.parent_id)!.children.push(node);
        } else {
            roots.push(node);
        }
    });

    return roots;
}

/** 获取部门全路径名 */
function getDepartmentPath(depts: Department[], id: string): string {
    const parts: string[] = [];
    let current = depts.find((d) => d.id === id);
    const visited = new Set<string>();

    while (current) {
        if (visited.has(current.id)) break;
        visited.add(current.id);
        parts.unshift(current.department_name);
        const parentId = current.parent_id;
        if (parentId && parentId !== '0') {
            current = depts.find((d) => d.id === parentId);
        } else {
            current = undefined;
        }
    }

    return parts.join(' / ');
}

/** 获取所有子部门 ID（包括自身） */
function getDescendantIds(depts: Department[], id: string): Set<string> {
    const ids = new Set<string>([id]);
    const children = depts.filter((d) => d.parent_id === id);
    for (const child of children) {
        const childIds = getDescendantIds(depts, child.id);
        childIds.forEach((cid) => ids.add(cid));
    }
    return ids;
}

// ======================== 组件 ========================

const DepartmentTreeSelector: React.FC<DepartmentTreeSelectorProps> = ({
    departments,
    value,
    onChange,
    label = '使用部门',
    placeholder = '请选择部门',
    required = false,
}) => {
    const [modalOpen, setModalOpen] = useState(false);
    const [searchText, setSearchText] = useState('');
    const [treeData, setTreeData] = useState<TreeNode[]>(() => buildTree(departments));
    const [selectedSet, setSelectedSet] = useState<Set<string>>(new Set(value));

    // 当外部 value 变化时同步 selectedSet
    React.useEffect(() => {
        setSelectedSet(new Set(value));
    }, [value]);

    // 当外部 departments 变化时重建树
    React.useEffect(() => {
        setTreeData(buildTree(departments));
    }, [departments]);

    // 获取所有子部门 ID（包括自身）
    const getAllDescendantIds = (node: TreeNode): string[] => {
        const ids: string[] = [node.id];
        for (const child of node.children) {
            ids.push(...getAllDescendantIds(child));
        }
        return ids;
    };

    // 切换节点选中状态（有子节点则联动子节点）
    const toggleNode = (node: TreeNode) => {
        const newSet = new Set(selectedSet);
        const descendantIds = getAllDescendantIds(node);
        // 判断当前节点是否选中
        const isCurrentlySelected = selectedSet.has(node.id);
        if (isCurrentlySelected) {
            // 取消选中：移除自身及所有子节点
            descendantIds.forEach((id) => newSet.delete(id));
        } else {
            // 选中：添加自身及所有子节点
            descendantIds.forEach((id) => newSet.add(id));
        }
        setSelectedSet(newSet);
        onChange(Array.from(newSet));
    };

    // 已选部门名称显示
    const displayValue = value.length > 0
        ? `已选 ${value.length} 个部门`
        : '';

    // 搜索过滤
    const searchFilter = (nodes: TreeNode[]): TreeNode[] => {
        if (!searchText) return nodes;
        return nodes
            .map((node) => {
                const matched =
                    node.department_name.toLowerCase().includes(searchText.toLowerCase());
                const filteredChildren = searchFilter(node.children);
                if (matched || filteredChildren.length > 0) {
                    return { ...node, expanded: true, children: filteredChildren };
                }
                return null;
            })
            .filter(Boolean) as TreeNode[];
    };

    const filteredTree = searchFilter(treeData);

    // 渲染树节点
    const renderTreeNode = (node: TreeNode, depth: number = 0) => {
        const hasChildren = node.children.length > 0;
        const isSelected = selectedSet.has(node.id);
        const isPartial = hasChildren && !isSelected && node.children.some((child) => selectedSet.has(child.id) || isChildPartiallySelected(child));

        // 辅助判断子节点是否有部分选中
        function isChildPartiallySelected(n: TreeNode): boolean {
            return n.children.some((c) => selectedSet.has(c.id) || isChildPartiallySelected(c));
        }

        return (
            <React.Fragment key={node.id}>
                <Box
                    style={{
                        paddingLeft: `${depth * 20 + 8}px`,
                        paddingRight: '8px',
                        paddingTop: '6px',
                        paddingBottom: '6px',
                        cursor: 'pointer',
                        borderRadius: 6,
                        backgroundColor: isSelected
                            ? 'var(--mantine-color-blue-light)'
                            : 'transparent',
                        display: 'flex',
                        alignItems: 'center',
                        gap: '4px',
                        userSelect: 'none',
                    }}
                    onClick={() => toggleNode(node)}
                    onMouseEnter={(e) => {
                        if (!isSelected) {
                            e.currentTarget.style.backgroundColor =
                                'var(--mantine-color-gray-light)';
                        }
                    }}
                    onMouseLeave={(e) => {
                        if (!isSelected) {
                            e.currentTarget.style.backgroundColor = 'transparent';
                        }
                    }}
                >
                    {/* 复选框 */}
                    <Box
                        style={{
                            width: 18,
                            height: 18,
                            borderRadius: 4,
                            border: '2px solid',
                            borderColor: isSelected || isPartial ? 'var(--mantine-color-blue-5)' : 'var(--mantine-color-gray-4)',
                            backgroundColor: isSelected ? 'var(--mantine-color-blue-5)' : isPartial ? 'var(--mantine-color-blue-1)' : 'transparent',
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'center',
                            flexShrink: 0,
                            color: isSelected ? 'white' : isPartial ? 'var(--mantine-color-blue-5)' : 'transparent',
                            fontSize: 12,
                            fontWeight: 700,
                        }}
                    >
                        {isSelected ? '✓' : isPartial ? '▬' : ''}
                    </Box>
                    {hasChildren ? (
                        <ActionIcon
                            variant="subtle"
                            size="sm"
                            onClick={(e) => {
                                e.stopPropagation();
                                node.expanded = !node.expanded;
                                setTreeData([...treeData]);
                            }}
                        >
                            {node.expanded ? (
                                <IconChevronDown size={14} />
                            ) : (
                                <IconChevronRight size={14} />
                            )}
                        </ActionIcon>
                    ) : (
                        <Box w={22} />
                    )}
                    {node.expanded && hasChildren ? (
                        <IconFolderOpen size={16} style={{ flexShrink: 0 }} />
                    ) : (
                        <IconFolder size={16} style={{ flexShrink: 0 }} />
                    )}
                    <Text size="sm" fw={isSelected ? 600 : 400} lineClamp={1}>
                        {node.department_name}
                    </Text>
                    {hasChildren && (
                        <Badge size="xs" variant="light" color="gray" ml="auto">
                            {node.children.length} 子部门
                        </Badge>
                    )}
                    {isSelected && (
                        <Badge size="xs" variant="light" color="blue" ml={4}>
                            已选
                        </Badge>
                    )}
                </Box>
                {hasChildren && node.expanded && (
                    <>
                        {node.children.map((child) => renderTreeNode(child, depth + 1))}
                    </>
                )}
            </React.Fragment>
        );
    };

    return (
        <>
            <TextInput
                label={label}
                placeholder={placeholder}
                required={required}
                value={displayValue}
                readOnly
                onClick={() => setModalOpen(true)}
                rightSection={value ? (
                    <Group gap={4}>
                        <Text size="xs" c="dimmed" style={{ cursor: 'pointer' }}
                            onClick={(e) => {
                                e.stopPropagation();
                                onChange([]);
                            }}
                        >
                            清除
                        </Text>
                        <IconChevronDown size={14} />
                    </Group>
                ) : (
                    <IconChevronDown size={14} />
                )}
                styles={{
                    input: { cursor: 'pointer' },
                }}
            />

            <Modal
                opened={modalOpen}
                onClose={() => setModalOpen(false)}
                title="选择使用部门"
                size="md"
            >
                <Stack gap="md">
                    <TextInput
                        placeholder="搜索部门..."
                        leftSection={<IconSearch size={16} />}
                        value={searchText}
                        onChange={(e) => setSearchText(e.target.value)}
                    />

                    {value.length > 0 && (
                        <Text size="sm" c="dimmed">
                            已选 <Text span fw={500} c="blue">{value.length}</Text> 个部门
                        </Text>
                    )}

                    <ScrollArea h={400}>
                        {filteredTree.length === 0 ? (
                            <Text ta="center" c="dimmed" py="xl">
                                {searchText ? '未找到匹配的部门' : '暂无部门数据'}
                            </Text>
                        ) : (
                            filteredTree.map((node) => renderTreeNode(node))
                        )}
                    </ScrollArea>

                    <Group justify="space-between">
                        <Button
                            variant="subtle"
                            color="red"
                            size="xs"
                            onClick={() => {
                                setSelectedSet(new Set());
                                onChange([]);
                            }}
                            disabled={value.length === 0}
                        >
                            清空全部
                        </Button>
                        <Group>
                            <Button variant="default" onClick={() => {
                                // 取消时恢复选中状态
                                setSelectedSet(new Set(value));
                                setModalOpen(false);
                            }}>
                                取消
                            </Button>
                            <Button onClick={() => setModalOpen(false)}>
                                确认 ({value.length})
                            </Button>
                        </Group>
                    </Group>
                </Stack>
            </Modal>
        </>
    );
};

export { getDescendantIds };
export default DepartmentTreeSelector;