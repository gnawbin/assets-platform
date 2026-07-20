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
    value: string;
    onChange: (value: string) => void;
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

    // 当外部 departments 变化时重建树
    React.useEffect(() => {
        setTreeData(buildTree(departments));
    }, [departments]);

    // 已选部门名称显示
    const displayValue = value
        ? getDepartmentPath(departments, value)
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
    const selectedDeptName = value
        ? departments.find((d) => d.id === value)?.department_name || ''
        : '';

    // 渲染树节点
    const renderTreeNode = (node: TreeNode, depth: number = 0) => {
        const hasChildren = node.children.length > 0;
        const isSelected = value === node.id;

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
                    onClick={() => {
                        onChange(node.id);
                        setModalOpen(false);
                    }}
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
                                onChange('');
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

                    {selectedDeptName && (
                        <Text size="sm" c="dimmed">
                            已选: <Text span fw={500} c="blue">{selectedDeptName}</Text>
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

                    <Group justify="end">
                        <Button variant="default" onClick={() => setModalOpen(false)}>
                            取消
                        </Button>
                        <Button onClick={() => setModalOpen(false)}>
                            确认
                        </Button>
                    </Group>
                </Stack>
            </Modal>
        </>
    );
};

export { getDescendantIds };
export default DepartmentTreeSelector;