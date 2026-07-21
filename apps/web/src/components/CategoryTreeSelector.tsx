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
} from '@tabler/icons-react';
import type { Category } from '@/services/categoryService';

// ======================== 树节点接口 ========================

interface TreeNode {
    id: string;
    category_name: string;
    asset_type: string;
    parent_id: string;
    sort: number;
    description: string | null;
    children: TreeNode[];
    expanded: boolean;
}

// ======================== 组件 Props ========================

interface CategoryTreeSelectorProps {
    categories: Category[];
    value: string;
    onChange: (value: string) => void;
    label?: string;
    placeholder?: string;
    required?: boolean;
    /** 按资产类型过滤：'fixed' 仅显示固定资产，'intangible' 仅显示无形资产，不传则显示全部 */
    assetType?: 'fixed' | 'intangible';
}

// ======================== 工具函数 ========================

/** 获取指定资产类型的所有分类 ID（包括其祖先节点） */
function getFilteredCategoryIds(cats: Category[], assetType: 'fixed' | 'intangible'): Set<string> {
    const targetIds = new Set<string>();
    const catMap = new Map<string, Category>();
    cats.forEach((c) => catMap.set(c.id, c));

    // 找出所有匹配 assetType 的分类
    const matched = cats.filter((c) => c.asset_type === assetType);
    matched.forEach((c) => {
        targetIds.add(c.id);
        // 向上追溯父节点，确保树结构完整
        let parentId = c.parent_id;
        while (parentId !== '0' && catMap.has(parentId)) {
            targetIds.add(parentId);
            parentId = catMap.get(parentId)!.parent_id;
        }
    });

    return targetIds;
}

/** 构建树结构 */
function buildTree(cats: Category[], assetType?: 'fixed' | 'intangible'): TreeNode[] {
    if (assetType) {
        const keepIds = getFilteredCategoryIds(cats, assetType);
        cats = cats.filter((c) => keepIds.has(c.id));
    }
    const map = new Map<string, TreeNode>();
    const roots: TreeNode[] = [];

    cats.forEach((cat) => {
        map.set(cat.id, {
            id: cat.id,
            category_name: cat.category_name,
            asset_type: cat.asset_type,
            parent_id: cat.parent_id,
            sort: cat.sort,
            description: cat.description,
            children: [],
            expanded: true,
        });
    });

    cats.forEach((cat) => {
        const node = map.get(cat.id)!;
        if (cat.parent_id !== '0' && map.has(cat.parent_id)) {
            map.get(cat.parent_id)!.children.push(node);
        } else {
            roots.push(node);
        }
    });

    // 按 sort 排序
    const sortChildren = (nodes: TreeNode[]) => {
        nodes.sort((a, b) => a.sort - b.sort);
        nodes.forEach((n) => sortChildren(n.children));
    };
    sortChildren(roots);

    return roots;
}

/** 获取叶子节点 ID 集合 */
function getLeafIds(cats: Category[]): Set<string> {
    const parentIds = new Set(cats.filter((c) => c.parent_id !== '0').map((c) => c.parent_id));
    const allIds = new Set(cats.map((c) => c.id));
    // 叶子节点 = 不在任何其他节点的 parent_id 中
    return new Set([...allIds].filter((id) => !parentIds.has(id)));
}

/** 获取分类全路径名 */
function getCategoryPath(cats: Category[], id: string): string {
    const parts: string[] = [];
    let current = cats.find((c) => c.id === id);
    const visited = new Set<string>();

    while (current) {
        if (visited.has(current.id)) break;
        visited.add(current.id);
        parts.unshift(current.category_name);
        const parentId = current.parent_id;
        if (parentId !== '0') {
            current = cats.find((c) => c.id === parentId);
        } else {
            current = undefined;
        }
    }

    return parts.join(' / ');
}

// ======================== 组件 ========================

const CategoryTreeSelector: React.FC<CategoryTreeSelectorProps> = ({
    categories,
    value,
    onChange,
    label = '资产分类',
    placeholder = '请选择分类',
    required = false,
    assetType,
}) => {
    const [modalOpen, setModalOpen] = useState(false);
    const [searchText, setSearchText] = useState('');
    const [treeData, setTreeData] = useState<TreeNode[]>(() => buildTree(categories, assetType));

    // 当外部 categories 或 assetType 变化时重建树
    React.useEffect(() => {
        setTreeData(buildTree(categories, assetType));
    }, [categories, assetType]);

    // 叶子节点 ID 集合
    const leafIds = useMemo(() => getLeafIds(categories), [categories]);

    // 已选分类名称显示
    const displayValue = value
        ? getCategoryPath(categories, value)
        : '';

    // 搜索过滤
    const searchFilter = (nodes: TreeNode[]): TreeNode[] => {
        if (!searchText) return nodes;
        return nodes
            .map((node) => {
                const matched =
                    node.category_name.toLowerCase().includes(searchText.toLowerCase());
                const filteredChildren = searchFilter(node.children);
                if (matched || filteredChildren.length > 0) {
                    return { ...node, expanded: true, children: filteredChildren };
                }
                return null;
            })
            .filter(Boolean) as TreeNode[];
    };

    const filteredTree = searchFilter(treeData);
    const selectedCategoryName = value
        ? categories.find((c) => c.id === value)?.category_name || ''
        : '';

    // 渲染树节点
    const renderTreeNode = (node: TreeNode, depth: number = 0) => {
        const hasChildren = node.children.length > 0;
        const isLeaf = !hasChildren;
        const isSelected = value === node.id;

        return (
            <React.Fragment key={node.id}>
                <Box
                    style={{
                        paddingLeft: `${depth * 20 + 8}px`,
                        paddingRight: '8px',
                        paddingTop: '6px',
                        paddingBottom: '6px',
                        cursor: isLeaf ? 'pointer' : 'default',
                        borderRadius: 6,
                        backgroundColor: isSelected
                            ? 'var(--mantine-color-blue-light)'
                            : 'transparent',
                        display: 'flex',
                        alignItems: 'center',
                        gap: '4px',
                        userSelect: 'none',
                        opacity: isLeaf ? 1 : 0.6,
                    }}
                    onClick={() => {
                        if (isLeaf) {
                            onChange(node.id);
                            setModalOpen(false);
                        }
                    }}
                    onMouseEnter={(e) => {
                        if (!isSelected && isLeaf) {
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
                        {node.category_name}
                    </Text>
                    {!assetType && (
                        <Badge
                            size="xs"
                            variant="light"
                            color={node.asset_type === 'fixed' ? 'blue' : 'violet'}
                            ml="auto"
                        >
                            {node.asset_type === 'fixed' ? '固定资产' : '无形资产'}
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
                title="选择资产分类"
                size="md"
            >
                <Stack gap="md">
                    <TextInput
                        placeholder="搜索分类..."
                        leftSection={<IconSearch size={16} />}
                        value={searchText}
                        onChange={(e) => setSearchText(e.target.value)}
                    />

                    {selectedCategoryName && (
                        <Text size="sm" c="dimmed">
                            已选: <Text span fw={500} c="blue">{selectedCategoryName}</Text>
                        </Text>
                    )}

                    <ScrollArea h={400}>
                        {filteredTree.length === 0 ? (
                            <Text ta="center" c="dimmed" py="xl">
                                {searchText ? '未找到匹配的分类' : '暂无分类数据'}
                            </Text>
                        ) : (
                            filteredTree.map((node) => renderTreeNode(node))
                        )}
                    </ScrollArea>

                    <Group justify="space-between">
                        <Text size="xs" c="dimmed">
                            {leafIds.size} 个可选的叶子分类 | 提示: 仅最下级分类可选{assetType ? ` | 当前仅显示${assetType === 'fixed' ? '固定资产' : '无形资产'}` : ''}
                        </Text>
                        <Button variant="default" onClick={() => setModalOpen(false)}>
                            取消
                        </Button>
                    </Group>
                </Stack>
            </Modal>
        </>
    );
};

export default CategoryTreeSelector;