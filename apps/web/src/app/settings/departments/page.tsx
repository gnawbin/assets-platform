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
  Loader,
  Alert,
  SimpleGrid,
  Box,
  Paper,
  ScrollArea,
  ActionIcon,
  Tooltip,
  Divider,
  Select,
} from '@mantine/core';
import {
  IconAlertCircle,
  IconTrash,
  IconEdit,
  IconPlus,
  IconBuilding,
  IconBuildingCommunity,
  IconRefresh,
  IconChevronRight,
  IconChevronDown,
} from '@tabler/icons-react';
import { notifySuccess, notifyError } from '@/utils/notify';
import { useApi } from '@/hooks/useApi';
import {
  getDepartments,
  insertDepartment,
  updateDepartment,
  deleteDepartment,
  type Department,
} from '@/services/departmentService';
import { getTenants, type Tenant } from '@/services/tenantService';
import { useAuthStore } from '@/store/authStore';

// 树节点接口
interface TreeNode {
  id: string;
  department_name: string;
  parent_id: string | null;
  description: string | null;
  children: TreeNode[];
  expanded: boolean;
}

const DepartmentsPage: React.FC = () => {
  const { user } = useAuthStore();
  const [departments, setDepartments] = useState<Department[]>([]);
  const [treeData, setTreeData] = useState<TreeNode[]>([]);

  // 租户选择（超级管理员用）
  const [tenants, setTenants] = useState<Tenant[]>([]);
  const [selectedTenantId, setSelectedTenantId] = useState<string | null>(null);

  // 判断是否为超级管理员
  const isSuperAdmin = user?.is_super_admin === true;

  // 使用 useApi 管理数据获取
  const {
    data: fetchedDepartments,
    loading,
    error,
    execute: fetchDepartments,
  } = useApi(() => getDepartments(selectedTenantId ?? undefined));

  // 选中的部门
  const [selectedDept, setSelectedDept] = useState<Department | null>(null);

  // 新增/编辑弹窗
  const [formModalOpen, setFormModalOpen] = useState(false);
  const [formMode, setFormMode] = useState<'add' | 'edit'>('add');
  const [formParentId, setFormParentId] = useState<string | null>(null);
  const [formName, setFormName] = useState('');
  const [formDesc, setFormDesc] = useState('');

  // 使用 useApi 管理增删改操作
  const { execute: doInsert, loading: saving } = useApi(insertDepartment);
  const { execute: doUpdate } = useApi(updateDepartment);
  const { execute: doDelete, loading: deleting } = useApi(deleteDepartment);

  // 删除确认弹窗
  const [deleteModalOpen, setDeleteModalOpen] = useState(false);

  // 初始化：加载租户列表（超级管理员用）
  useEffect(() => {
    if (isSuperAdmin) {
      getTenants().then((list) => {
        setTenants(list);
        // 默认选中第一个启用的租户
        const first = list.find((t) => t.enable);
        if (first) {
          setSelectedTenantId(String(first.id));
        }
      }).catch((err) => {
        console.error('加载租户列表失败:', err);
      });
    } else if (user?.tenant_id) {
      // 普通管理员，使用自己的 tenant_id
      setSelectedTenantId(String(user.tenant_id));
    }
  }, [user]);

  // 当 fetchedDepartments 变化时更新本地状态
  useEffect(() => {
    if (fetchedDepartments) {
      setDepartments(fetchedDepartments);
      buildTree(fetchedDepartments);
    }
  }, [fetchedDepartments]);

  // 当 selectedTenantId 变化时重新获取部门
  useEffect(() => {
    if (selectedTenantId) {
      fetchDepartments();
    }
  }, [selectedTenantId]);

  // 构建树结构
  const buildTree = (depts: Department[]) => {
    const map = new Map<string, TreeNode>();
    const roots: TreeNode[] = [];

    // 先创建所有节点
    depts.forEach((dept) => {
      map.set(dept.id, {
        id: dept.id,
        department_name: dept.department_name,
        parent_id: dept.parent_id,
        description: dept.description,
        children: [],
        expanded: true,
      });
    });

    // 构建父子关系
    depts.forEach((dept) => {
      const node = map.get(dept.id)!;
      if (dept.parent_id && map.has(dept.parent_id)) {
        map.get(dept.parent_id)!.children.push(node);
      } else {
        roots.push(node);
      }
    });

    setTreeData(roots);
  };

  // 获取部门全路径名称
  const getDepartmentPath = (dept: Department): string => {
    const parts: string[] = [dept.department_name];
    let current = dept;
    const visited = new Set<string>();
    visited.add(current.id);

    while (current.parent_id) {
      const parent = departments.find((d) => d.id === current.parent_id);
      if (!parent || visited.has(parent.id)) break;
      parts.unshift(parent.department_name);
      visited.add(parent.id);
      current = parent;
    }
    return parts.join(' / ');
  };

  // 获取父部门名称
  const getParentName = (parentId: string | null): string => {
    if (!parentId) return '（顶级部门）';
    const parent = departments.find((d) => d.id === parentId);
    return parent ? parent.department_name : '（未知）';
  };

  // 打开新增弹窗
  const openAddModal = (parentId: string | null = null) => {
    setFormMode('add');
    setFormParentId(parentId);
    setFormName('');
    setFormDesc('');
    setFormModalOpen(true);
  };

  // 打开编辑弹窗
  const openEditModal = () => {
    if (!selectedDept) return;
    setFormMode('edit');
    setFormParentId(selectedDept.parent_id);
    setFormName(selectedDept.department_name);
    setFormDesc(selectedDept.description || '');
    setFormModalOpen(true);
  };

  // 保存部门
  const handleSave = async () => {
    if (!formName.trim()) {
      notifyError('验证失败', '请输入部门名称');
      return;
    }

    if (!selectedTenantId) {
      notifyError('验证失败', '请先选择租户');
      return;
    }

    try {
      if (formMode === 'add') {
        await doInsert({
          departmentName: formName.trim(),
          parentId: formParentId?.toString() ?? null,
          description: formDesc.trim() || null,
          tenantId: selectedTenantId,
        });
        notifySuccess('部门添加成功');
      } else {
        if (!selectedDept) return;
        await doUpdate({
          id: selectedDept.id,
          departmentName: formName.trim(),
          parentId: formParentId?.toString() ?? null,
          description: formDesc.trim() || null,
        });
        notifySuccess('部门更新成功');
      }
      setFormModalOpen(false);
      fetchDepartments();
    } catch (err) {
      console.error('保存部门失败:', err);
      notifyError('保存部门失败', typeof err === 'string' ? err : undefined);
    }
  };

  // 打开删除确认
  const openDeleteModal = () => {
    setDeleteModalOpen(true);
  };

  // 确认删除
  const handleDelete = async () => {
    if (!selectedDept) return;
    try {
      await doDelete(selectedDept.id);
      setDeleteModalOpen(false);
      setSelectedDept(null);
      notifySuccess('部门删除成功');
      fetchDepartments();
    } catch (err) {
      console.error('删除部门失败:', err);
      notifyError('删除部门失败', typeof err === 'string' ? err : undefined);
    }
  };

  // 递归渲染树节点
  const renderTreeNode = (node: TreeNode, depth: number = 0) => {
    const isSelected = selectedDept?.id === node.id;
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
            const dept = departments.find((d) => d.id === node.id) || null;
            setSelectedDept(dept);
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
          {depth === 0 ? (
            <IconBuilding size={16} style={{ flexShrink: 0 }} />
          ) : (
            <IconBuildingCommunity size={16} style={{ flexShrink: 0 }} />
          )}
          <Text size="sm" fw={isSelected ? 600 : 400} lineClamp={1}>
            {node.department_name}
          </Text>
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
            <Title order={2}>部门管理</Title>
            <Text c="dimmed">管理系统组织架构</Text>
          </div>
          <Group>
            <Button
              variant="light"
              leftSection={<IconRefresh size={16} />}
              onClick={fetchDepartments}
              loading={loading}
            >
              刷新
            </Button>
            <Button
              leftSection={<IconPlus size={16} />}
              onClick={() => openAddModal(null)}
              disabled={!selectedTenantId}
            >
              新增根部门
            </Button>
          </Group>
        </Group>

        {/* 超级管理员：组织结构选择器 */}
        {isSuperAdmin && (
          <Select
            label="选择组织结构"
            placeholder="请选择组织结构查看其部门"
            data={tenants
              .filter((t) => t.enable)
              .map((t) => ({
                value: String(t.id),
                label: t.tenant_name,
              }))}
            value={selectedTenantId}
            onChange={(value) => {
              setSelectedTenantId(value);
              setSelectedDept(null);
            }}
            clearable={false}
            w={300}
          />
        )}

        {error && (
          <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red">
            {error}
          </Alert>
        )}

        <SimpleGrid cols={{ base: 1, md: 2 }} spacing="lg">
          {/* 左侧：部门树 */}
          <Card withBorder padding="lg" radius="md" h={600}>
            <Group justify="space-between" mb="md">
              <Text fw={600} size="sm">
                部门结构
              </Text>
              <Text size="xs" c="dimmed">
                {departments.length} 个部门
              </Text>
            </Group>
            <Divider mb="md" />
            {loading ? (
              <Group justify="center" py="xl">
                <Loader />
              </Group>
            ) : !selectedTenantId ? (
              <Text ta="center" c="dimmed" py="xl">
                {isSuperAdmin ? '请先选择组织结构' : '加载中...'}
              </Text>
            ) : treeData.length === 0 ? (
              <Text ta="center" c="dimmed" py="xl">
                暂无部门数据，请新增根部门
              </Text>
            ) : (
              <ScrollArea h={500}>
                {treeData.map((node) => renderTreeNode(node))}
              </ScrollArea>
            )}
          </Card>

          {/* 右侧：部门详情 */}
          <Card withBorder padding="lg" radius="md" h={600}>
            {selectedDept ? (
              <Stack gap="md">
                <Group justify="space-between">
                  <Text fw={600} size="sm">
                    部门详情
                  </Text>
                  <Group gap="xs">
                    <Tooltip label="新增子部门">
                      <Button
                        size="xs"
                        variant="light"
                        leftSection={<IconPlus size={14} />}
                        onClick={() => openAddModal(selectedDept.id)}
                      >
                        新增子部门
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
                        部门名称
                      </Text>
                      <Text size="sm" fw={500}>
                        {selectedDept.department_name}
                      </Text>
                    </Group>
                    <Group>
                      <Text size="sm" c="dimmed" w={80}>
                        所属上级
                      </Text>
                      <Text size="sm">
                        {getParentName(selectedDept.parent_id)}
                      </Text>
                    </Group>
                    <Group>
                      <Text size="sm" c="dimmed" w={80}>
                        全路径
                      </Text>
                      <Text size="sm" c="blue">
                        {getDepartmentPath(selectedDept)}
                      </Text>
                    </Group>
                    <Group>
                      <Text size="sm" c="dimmed" w={80}>
                        描述
                      </Text>
                      <Text size="sm">
                        {selectedDept.description || '暂无描述'}
                      </Text>
                    </Group>
                  </Stack>
                </Paper>

                <Text size="xs" c="dimmed">
                  创建时间:{' '}
                  {selectedDept.created_at
                    ? new Date(selectedDept.created_at).toLocaleString('zh-CN')
                    : '-'}
                </Text>
                <Text size="xs" c="dimmed">
                  更新时间:{' '}
                  {selectedDept.updated_at
                    ? new Date(selectedDept.updated_at).toLocaleString('zh-CN')
                    : '-'}
                </Text>
              </Stack>
            ) : (
              <Stack align="center" justify="center" h="100%" gap="md">
                <IconBuilding size={48} color="var(--mantine-color-gray-4)" />
                <Text c="dimmed">请从左侧选择一个部门查看详情</Text>
              </Stack>
            )}
          </Card>
        </SimpleGrid>
      </Stack>

      {/* 新增/编辑部门弹窗 */}
      <Modal
        opened={formModalOpen}
        onClose={() => setFormModalOpen(false)}
        title={formMode === 'add' ? '新增部门' : '编辑部门'}
        size="md"
      >
        <Stack gap="md">
          {formMode === 'add' && formParentId && (
            <Text size="sm" c="dimmed">
              父级部门：{getParentName(formParentId)}
            </Text>
          )}
          <TextInput
            label="部门名称"
            placeholder="请输入部门名称"
            required
            value={formName}
            onChange={(e) => setFormName(e.target.value)}
          />
          <Textarea
            label="描述"
            placeholder="请输入部门描述（可选）"
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
            确定要删除部门 <strong>{selectedDept?.department_name}</strong> 吗？
          </Text>
          <Text size="sm" c="dimmed">
            此操作将软删除该部门，且仅当部门下没有子部门时才能删除。
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

export default DepartmentsPage;
