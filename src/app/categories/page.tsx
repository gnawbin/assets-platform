'use client';
import React, { useEffect, useState } from 'react';
import Layout from '@/components/Layout';
import {
  Title,
  Text,
  Card,
  Stack,
  Group,
  Button,
  Modal,
  TextInput,
  Textarea,
  NumberInput,
  Select,
  Loader,
  Alert,
  SimpleGrid,
  Box,
  Paper,
  ScrollArea,
  ActionIcon,
  Tooltip,
  Divider,
  Badge,
} from '@mantine/core';
import {
  IconAlertCircle,
  IconTrash,
  IconEdit,
  IconPlus,
  IconFolder,
  IconFolderOpen,
  IconRefresh,
  IconChevronRight,
  IconChevronDown,
} from '@tabler/icons-react';
import { invoke } from '@tauri-apps/api/core';

interface Category {
  id: number;
  category_name: string;
  asset_type: string;
  parent_id: number;
  sort: number;
  description: string | null;
  created_by: number | null;
  created_at: string | null;
  updated_by: number | null;
  updated_at: string | null;
  deleted: number | null;
}

// 树节点接口
interface TreeNode {
  id: number;
  category_name: string;
  asset_type: string;
  parent_id: number;
  sort: number;
  description: string | null;
  children: TreeNode[];
  expanded: boolean;
}

const CategoriesPage: React.FC = () => {
  const [categories, setCategories] = useState<Category[]>([]);
  const [treeData, setTreeData] = useState<TreeNode[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // 选中的分类
  const [selectedCategory, setSelectedCategory] = useState<Category | null>(null);

  // 新增/编辑弹窗
  const [formModalOpen, setFormModalOpen] = useState(false);
  const [formMode, setFormMode] = useState<'add' | 'edit'>('add');
  const [formParentId, setFormParentId] = useState<number>(0);
  const [formName, setFormName] = useState('');
  const [formAssetType, setFormAssetType] = useState('hardware');
  const [formSort, setFormSort] = useState<number>(0);
  const [formDesc, setFormDesc] = useState('');
  const [saving, setSaving] = useState(false);

  // 删除确认弹窗
  const [deleteModalOpen, setDeleteModalOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);

  useEffect(() => {
    fetchCategories();
  }, []);

  const fetchCategories = async () => {
    try {
      setLoading(true);
      setError(null);
      const data = await invoke<Category[]>('get_categories');
      setCategories(data);
      buildTree(data);
    } catch (err) {
      console.error('获取分类列表失败:', err);
      setError(typeof err === 'string' ? err : '获取分类列表失败');
    } finally {
      setLoading(false);
    }
  };

  // 构建树结构
  const buildTree = (cats: Category[]) => {
    const map = new Map<number, TreeNode>();
    const roots: TreeNode[] = [];

    // 先创建所有节点
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

    // 构建父子关系
    cats.forEach((cat) => {
      const node = map.get(cat.id)!;
      if (cat.parent_id !== 0 && map.has(cat.parent_id)) {
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

    setTreeData(roots);
  };

  // 获取分类全路径名称
  const getCategoryPath = (cat: Category): string => {
    const parts: string[] = [cat.category_name];
    let current = cat;
    const visited = new Set<number>();
    visited.add(current.id);

    while (current.parent_id !== 0) {
      const parent = categories.find((c) => c.id === current.parent_id);
      if (!parent || visited.has(parent.id)) break;
      parts.unshift(parent.category_name);
      visited.add(parent.id);
      current = parent;
    }
    return parts.join(' / ');
  };

  // 获取父分类名称
  const getParentName = (parentId: number): string => {
    if (parentId === 0) return '（顶级分类）';
    const parent = categories.find((c) => c.id === parentId);
    return parent ? parent.category_name : '（未知）';
  };

  // 打开新增弹窗
  const openAddModal = (parentId: number = 0) => {
    setFormMode('add');
    setFormParentId(parentId);
    setFormName('');
    setFormAssetType('hardware');
    setFormSort(0);
    setFormDesc('');
    setFormModalOpen(true);
  };

  // 打开编辑弹窗
  const openEditModal = () => {
    if (!selectedCategory) return;
    setFormMode('edit');
    setFormParentId(selectedCategory.parent_id);
    setFormName(selectedCategory.category_name);
    setFormAssetType(selectedCategory.asset_type);
    setFormSort(selectedCategory.sort);
    setFormDesc(selectedCategory.description || '');
    setFormModalOpen(true);
  };

  // 保存分类
  const handleSave = async () => {
    if (!formName.trim()) {
      alert('请输入分类名称');
      return;
    }

    setSaving(true);
    try {
      if (formMode === 'add') {
        const newCategory: Category = {
          id: 0,
          category_name: formName.trim(),
          asset_type: formAssetType,
          parent_id: formParentId,
          sort: formSort,
          description: formDesc.trim() || null,
          created_by: null,
          created_at: null,
          updated_by: null,
          updated_at: null,
          deleted: null,
        };
        await invoke('insert_category', { category: newCategory });
        alert('分类添加成功！');
      } else {
        if (!selectedCategory) return;
        const updatedCategory: Category = {
          ...selectedCategory,
          category_name: formName.trim(),
          asset_type: formAssetType,
          parent_id: formParentId,
          sort: formSort,
          description: formDesc.trim() || null,
        };
        await invoke('update_category', { category: updatedCategory });
        alert('分类更新成功！');
      }
      setFormModalOpen(false);
      fetchCategories();
    } catch (err) {
      console.error('保存分类失败:', err);
      alert(typeof err === 'string' ? err : '保存分类失败');
    } finally {
      setSaving(false);
    }
  };

  // 打开删除确认
  const openDeleteModal = () => {
    setDeleteModalOpen(true);
  };

  // 确认删除
  const handleDelete = async () => {
    if (!selectedCategory) return;
    setDeleting(true);
    try {
      await invoke('delete_category', { id: selectedCategory.id });
      setDeleteModalOpen(false);
      setSelectedCategory(null);
      alert('分类删除成功！');
      fetchCategories();
    } catch (err) {
      console.error('删除分类失败:', err);
      alert(typeof err === 'string' ? err : '删除分类失败');
    } finally {
      setDeleting(false);
    }
  };

  // 递归渲染树节点
  const renderTreeNode = (node: TreeNode, depth: number = 0) => {
    const isSelected = selectedCategory?.id === node.id;
    const hasChildren = node.children.length > 0;

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
            const cat = categories.find((c) => c.id === node.id) || null;
            setSelectedCategory(cat);
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
            {node.category_name}
          </Text>
          <Badge
            size="xs"
            variant="light"
            color={node.asset_type === 'hardware' ? 'blue' : 'violet'}
            ml="auto"
          >
            {node.asset_type === 'hardware' ? '固定资产' : '无形资产'}
          </Badge>
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
    <Layout>
      <Stack gap="lg">
        <Group justify="space-between">
          <div>
            <Title order={2}>资产分类</Title>
            <Text c="dimmed">管理资产分类结构</Text>
          </div>
          <Group>
            <Button
              variant="light"
              leftSection={<IconRefresh size={16} />}
              onClick={fetchCategories}
              loading={loading}
            >
              刷新
            </Button>
            <Button
              leftSection={<IconPlus size={16} />}
              onClick={() => openAddModal(0)}
            >
              新增根分类
            </Button>
          </Group>
        </Group>

        {error && (
          <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red">
            {error}
          </Alert>
        )}

        <SimpleGrid cols={{ base: 1, md: 2 }} spacing="lg">
          {/* 左侧：分类树 */}
          <Card withBorder padding="lg" radius="md" h={600}>
            <Group justify="space-between" mb="md">
              <Text fw={600} size="sm">
                分类结构
              </Text>
              <Text size="xs" c="dimmed">
                {categories.length} 个分类
              </Text>
            </Group>
            <Divider mb="md" />
            {loading ? (
              <Group justify="center" py="xl">
                <Loader />
              </Group>
            ) : treeData.length === 0 ? (
              <Text ta="center" c="dimmed" py="xl">
                暂无分类数据，请新增根分类
              </Text>
            ) : (
              <ScrollArea h={500}>
                {treeData.map((node) => renderTreeNode(node))}
              </ScrollArea>
            )}
          </Card>

          {/* 右侧：分类详情 */}
          <Card withBorder padding="lg" radius="md" h={600}>
            {selectedCategory ? (
              <Stack gap="md">
                <Group justify="space-between">
                  <Text fw={600} size="sm">
                    分类详情
                  </Text>
                  <Group gap="xs">
                    <Tooltip label="新增子分类">
                      <Button
                        size="xs"
                        variant="light"
                        leftSection={<IconPlus size={14} />}
                        onClick={() => openAddModal(selectedCategory.id)}
                      >
                        新增子分类
                      </Button>
                    </Tooltip>
                    <Tooltip label="编辑">
                      <ActionIcon
                        variant="light"
                        color="blue"
                        onClick={openEditModal}
                      >
                        <IconEdit size={16} />
                      </ActionIcon>
                    </Tooltip>
                    <Tooltip label="删除">
                      <ActionIcon
                        variant="light"
                        color="red"
                        onClick={openDeleteModal}
                      >
                        <IconTrash size={16} />
                      </ActionIcon>
                    </Tooltip>
                  </Group>
                </Group>
                <Divider />

                <Paper p="md" withBorder radius="sm">
                  <Stack gap="sm">
                    <Group>
                      <Text size="sm" c="dimmed" w={80}>
                        分类名称
                      </Text>
                      <Text size="sm" fw={500}>
                        {selectedCategory.category_name}
                      </Text>
                    </Group>
                    <Group>
                      <Text size="sm" c="dimmed" w={80}>
                        资产类型
                      </Text>
                      <Badge
                        variant="light"
                        color={
                          selectedCategory.asset_type === 'hardware'
                            ? 'blue'
                            : 'violet'
                        }
                      >
                        {selectedCategory.asset_type}
                      </Badge>
                    </Group>
                    <Group>
                      <Text size="sm" c="dimmed" w={80}>
                        所属上级
                      </Text>
                      <Text size="sm">
                        {getParentName(selectedCategory.parent_id)}
                      </Text>
                    </Group>
                    <Group>
                      <Text size="sm" c="dimmed" w={80}>
                        全路径
                      </Text>
                      <Text size="sm" c="blue">
                        {getCategoryPath(selectedCategory)}
                      </Text>
                    </Group>
                    <Group>
                      <Text size="sm" c="dimmed" w={80}>
                        排序
                      </Text>
                      <Text size="sm">{selectedCategory.sort}</Text>
                    </Group>
                    <Group>
                      <Text size="sm" c="dimmed" w={80}>
                        描述
                      </Text>
                      <Text size="sm">
                        {selectedCategory.description || '暂无描述'}
                      </Text>
                    </Group>
                  </Stack>
                </Paper>

                <Text size="xs" c="dimmed">
                  创建时间:{' '}
                  {selectedCategory.created_at
                    ? new Date(selectedCategory.created_at).toLocaleString(
                        'zh-CN'
                      )
                    : '-'}
                </Text>
                <Text size="xs" c="dimmed">
                  更新时间:{' '}
                  {selectedCategory.updated_at
                    ? new Date(selectedCategory.updated_at).toLocaleString(
                        'zh-CN'
                      )
                    : '-'}
                </Text>
              </Stack>
            ) : (
              <Stack align="center" justify="center" h="100%" gap="md">
                <IconFolder size={48} color="var(--mantine-color-gray-4)" />
                <Text c="dimmed">请从左侧选择一个分类查看详情</Text>
              </Stack>
            )}
          </Card>
        </SimpleGrid>
      </Stack>

      {/* 新增/编辑分类弹窗 */}
      <Modal
        opened={formModalOpen}
        onClose={() => setFormModalOpen(false)}
        title={formMode === 'add' ? '新增分类' : '编辑分类'}
        size="md"
      >
        <Stack gap="md">
          {formMode === 'add' && formParentId !== 0 && (
            <Text size="sm" c="dimmed">
              父级分类：{getParentName(formParentId)}
            </Text>
          )}
          <TextInput
            label="分类名称"
            placeholder="请输入分类名称"
            required
            value={formName}
            onChange={(e) => setFormName(e.target.value)}
          />
          <Select
            label="资产类型"
            placeholder="请选择资产类型"
            required
            data={[
              { value: 'hardware', label: '硬件资产' },
              { value: 'intangible', label: '无形资产' },
            ]}
            value={formAssetType}
            onChange={(val) => setFormAssetType(val || 'hardware')}
          />
          <NumberInput
            label="排序"
            placeholder="请输入排序号"
            value={formSort}
            onChange={(val) => setFormSort(Number(val) || 0)}
            min={0}
          />
          <Textarea
            label="描述"
            placeholder="请输入分类描述（可选）"
            minRows={3}
            value={formDesc}
            onChange={(e) => setFormDesc(e.target.value)}
          />
          <Group justify="flex-end" mt="md">
            <Button variant="default" onClick={() => setFormModalOpen(false)}>
              取消
            </Button>
            <Button onClick={handleSave} loading={saving}>
              {formMode === 'add' ? '保存' : '保存修改'}
            </Button>
          </Group>
        </Stack>
      </Modal>

      {/* 删除确认弹窗 */}
      <Modal
        opened={deleteModalOpen}
        onClose={() => setDeleteModalOpen(false)}
        title="确认删除"
        size="sm"
      >
        <Stack gap="md">
          <Text>
            确定要删除分类 <strong>{selectedCategory?.category_name}</strong>{' '}
            吗？
          </Text>
          <Text size="sm" c="dimmed">
            此操作将软删除该分类。
          </Text>
          <Group justify="flex-end" mt="md">
            <Button variant="default" onClick={() => setDeleteModalOpen(false)}>
              取消
            </Button>
            <Button color="red" onClick={handleDelete} loading={deleting}>
              确认删除
            </Button>
          </Group>
        </Stack>
      </Modal>
    </Layout>
  );
};

export default CategoriesPage;
