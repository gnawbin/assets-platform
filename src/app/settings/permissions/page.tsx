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
  Table,
  Modal,
  Checkbox,
  Loader,
  Alert,
  TextInput,
} from '@mantine/core';
import { IconAlertCircle, IconTrash, IconShield } from '@tabler/icons-react';
import { invoke } from '@tauri-apps/api/core';

interface Role {
  id: number;
  role_key: string;
  role_name: string;
  description: string | null;
  created_by: number | null;
  created_at: string | null;
  updated_by: number | null;
  updated_at: string | null;
  deleted: number | null;
}

interface MantineTree {
  value: string;
  label: string;
  children: MantineTree[] | null;
  checked?: boolean;
}

const PermissionsPage: React.FC = () => {
  console.log('PermissionsPage RENDERED');
  const [roles, setRoles] = useState<Role[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // 分配权限弹窗
  const [permModalOpen, setPermModalOpen] = useState(false);
  const [selectedRole, setSelectedRole] = useState<Role | null>(null);
  const [menuTree, setMenuTree] = useState<MantineTree[]>([]);
  const [menuTreeLoading, setMenuTreeLoading] = useState(false);
  const [menuTreeError, setMenuTreeError] = useState<string | null>(null);
  const [checkedMenuIds, setCheckedMenuIds] = useState<Set<string>>(new Set());
  const [savingPerms, setSavingPerms] = useState(false);

  // 删除确认弹窗
  const [deleteModalOpen, setDeleteModalOpen] = useState(false);
  const [deleteRole, setDeleteRole] = useState<Role | null>(null);
  const [deleting, setDeleting] = useState(false);

  // 新增角色弹窗
  const [addModalOpen, setAddModalOpen] = useState(false);
  const [newRoleKey, setNewRoleKey] = useState('');
  const [newRoleName, setNewRoleName] = useState('');
  const [newRoleDesc, setNewRoleDesc] = useState('');
  const [adding, setAdding] = useState(false);

  useEffect(() => {
    fetchRoles();
  }, []);

  const fetchRoles = async () => {
    try {
      setLoading(true);
      setError(null);
      const data = await invoke<Role[]>('get_roles');
      setRoles(data);
    } catch (err) {
      console.error('获取角色列表失败:', err);
      setError(typeof err === 'string' ? err : '获取角色列表失败');
    } finally {
      setLoading(false);
    }
  };

  // 打开分配权限弹窗
  const openAssignPermModal = async (role: Role) => {
    console.log('openAssignPermModal called', role);
    setSelectedRole(role);
    setPermModalOpen(true);
    setMenuTree([]);
    setMenuTreeLoading(true);
    setMenuTreeError(null);
    setCheckedMenuIds(new Set());
    try {
      // 获取菜单树
      console.log('invoking get_all_menus_tree...');
      const tree = await invoke<MantineTree[]>('get_all_menus_tree');
      console.log('get_all_menus_tree result:', tree);
      setMenuTree(tree);
      // 获取角色已分配的权限ID
      console.log('invoking get_role_menu_ids...');
      const menuIds = await invoke<number[]>('get_role_menu_ids', { roleId: role.id });
      console.log('get_role_menu_ids result:', menuIds);
      setCheckedMenuIds(new Set(menuIds.map(String)));
    } catch (err) {
      console.error('获取菜单树失败:', err);
      setMenuTreeError(typeof err === 'string' ? err : '获取菜单树失败');
    } finally {
      setMenuTreeLoading(false);
    }
  };

  // 保存权限分配
  const handleSavePerms = async () => {
    if (!selectedRole) return;
    setSavingPerms(true);
    try {
      const menuIds = Array.from(checkedMenuIds).map(Number);
      await invoke('assign_role_menus', { roleId: selectedRole.id, menuIds });
      setPermModalOpen(false);
      alert('权限分配成功！');
    } catch (err) {
      console.error('分配权限失败:', err);
      alert(typeof err === 'string' ? err : '分配权限失败');
    } finally {
      setSavingPerms(false);
    }
  };

  // 打开删除确认弹窗
  const openDeleteModal = (role: Role) => {
    setDeleteRole(role);
    setDeleteModalOpen(true);
  };

  // 确认删除角色
  const handleDeleteRole = async () => {
    if (!deleteRole) return;
    setDeleting(true);
    try {
      await invoke('delete_role', { roleId: deleteRole.id });
      setDeleteModalOpen(false);
      setDeleteRole(null);
      alert('角色删除成功！');
      fetchRoles();
    } catch (err) {
      console.error('删除角色失败:', err);
      alert(typeof err === 'string' ? err : '删除角色失败');
    } finally {
      setDeleting(false);
    }
  };

  // 新增角色
  const handleAddRole = async () => {
    if (!newRoleKey.trim() || !newRoleName.trim()) {
      alert('请输入角色标识和角色名称');
      return;
    }
    setAdding(true);
    try {
      await invoke('insert_role', {
        role: {
          id: 0,
          role_key: newRoleKey.trim(),
          role_name: newRoleName.trim(),
          description: newRoleDesc.trim() || null,
          created_by: null,
          created_at: null,
          updated_by: null,
          updated_at: null,
          deleted: 0,
        },
      });
      setAddModalOpen(false);
      setNewRoleKey('');
      setNewRoleName('');
      setNewRoleDesc('');
      alert('角色添加成功！');
      fetchRoles();
    } catch (err) {
      console.error('新增角色失败:', err);
      alert(typeof err === 'string' ? err : '新增角色失败');
    } finally {
      setAdding(false);
    }
  };

  // 递归渲染菜单树复选框（depth 控制缩进层级）
  const renderMenuTreeCheckboxes = (nodes: MantineTree[], depth: number = 0): React.ReactNode[] => {
    const rows: React.ReactNode[] = [];
    nodes.forEach((node) => {
      const isChecked = checkedMenuIds.has(node.value);
      const hasChildren = node.children && node.children.length > 0;

      const handleToggle = (checked: boolean) => {
        const newChecked = new Set(checkedMenuIds);
        if (checked) {
          newChecked.add(node.value);
          // 勾选所有子节点
          if (hasChildren) {
            addAllChildren(node, newChecked);
          }
        } else {
          newChecked.delete(node.value);
          // 取消所有子节点
          if (hasChildren) {
            removeAllChildren(node, newChecked);
          }
        }
        setCheckedMenuIds(newChecked);
      };

      rows.push(
        <tr key={node.value}>
          <td style={{ paddingLeft: `${depth * 28 + 12}px` }}>
            <Checkbox
              checked={isChecked}
              onChange={(e) => handleToggle(e.currentTarget.checked)}
              label={node.label}
            />
          </td>
        </tr>
      );

      if (hasChildren) {
        rows.push(...renderMenuTreeCheckboxes(node.children!, depth + 1));
      }
    });
    return rows;
  };

  const addAllChildren = (node: MantineTree, set: Set<string>) => {
    if (node.children) {
      node.children.forEach((child) => {
        set.add(child.value);
        addAllChildren(child, set);
      });
    }
  };

  const removeAllChildren = (node: MantineTree, set: Set<string>) => {
    if (node.children) {
      node.children.forEach((child) => {
        set.delete(child.value);
        removeAllChildren(child, set);
      });
    }
  };

  return (
    <Layout>
      <Stack gap="lg">
        <Group justify="space-between">
          <div>
            <Title order={2}>权限管理</Title>
            <Text c="dimmed">管理系统角色及其菜单权限</Text>
          </div>
          <Button onClick={() => setAddModalOpen(true)}>新增角色</Button>
        </Group>

        {error && (
          <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red">
            {error}
          </Alert>
        )}

        <Card withBorder padding="lg" radius="md">
          {loading ? (
            <Group justify="center" py="xl">
              <Loader />
            </Group>
          ) : (
            <Table striped highlightOnHover>
              <Table.Thead>
                <Table.Tr>
                  <Table.Th>角色名称</Table.Th>
                  <Table.Th>角色标识</Table.Th>
                  <Table.Th>描述</Table.Th>
                  <Table.Th style={{ width: 200 }}>操作</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {roles.length === 0 ? (
                  <Table.Tr>
                    <Table.Td colSpan={4}>
                      <Text ta="center" c="dimmed" py="xl">
                        暂无角色数据
                      </Text>
                    </Table.Td>
                  </Table.Tr>
                ) : (
                  roles.map((role) => (
                    <Table.Tr key={role.id}>
                      <Table.Td>{role.role_name}</Table.Td>
                      <Table.Td>
                        <Text size="sm" fs="italic">
                          {role.role_key}
                        </Text>
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm" c="dimmed">
                          {role.description || '-'}
                        </Text>
                      </Table.Td>
                      <Table.Td>
                        <Group gap="xs">
                          <Button
                            size="xs"
                            variant="light"
                            leftSection={<IconShield size={14} />}
                            onClick={() => openAssignPermModal(role)}
                          >
                            分配权限
                          </Button>
                          <Button
                            size="xs"
                            variant="light"
                            color="red"
                            leftSection={<IconTrash size={14} />}
                            onClick={() => openDeleteModal(role)}
                          >
                            删除
                          </Button>
                        </Group>
                      </Table.Td>
                    </Table.Tr>
                  ))
                )}
              </Table.Tbody>
            </Table>
          )}
        </Card>
      </Stack>

      {/* 分配权限弹窗 */}
      <Modal
        opened={permModalOpen}
        onClose={() => setPermModalOpen(false)}
        title={`分配权限 - ${selectedRole?.role_name || ''}`}
        size="lg"
      >
        <Stack gap="md">
          {menuTreeLoading ? (
            <Group justify="center" py="xl">
              <Loader />
              <Text c="dimmed">加载菜单树中...</Text>
            </Group>
          ) : menuTreeError ? (
            <Alert icon={<IconAlertCircle size={16} />} title="加载失败" color="red">
              {menuTreeError}
            </Alert>
          ) : menuTree.length > 0 ? (
            <Table>
              <Table.Tbody>{renderMenuTreeCheckboxes(menuTree)}</Table.Tbody>
            </Table>
          ) : (
            <Text c="dimmed">暂无菜单数据</Text>
          )}
          <Group justify="flex-end" mt="md">
            <Button variant="default" onClick={() => setPermModalOpen(false)}>
              取消
            </Button>
            <Button onClick={handleSavePerms} loading={savingPerms} disabled={menuTreeLoading || !!menuTreeError}>
              保存权限
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
            确定要删除角色 <strong>{deleteRole?.role_name}</strong>（{deleteRole?.role_key}）吗？
          </Text>
          <Text size="sm" c="dimmed">
            此操作将同时删除该角色的所有权限关联和用户关联，且不可恢复。
          </Text>
          <Group justify="flex-end" mt="md">
            <Button variant="default" onClick={() => setDeleteModalOpen(false)}>
              取消
            </Button>
            <Button color="red" onClick={handleDeleteRole} loading={deleting}>
              确认删除
            </Button>
          </Group>
        </Stack>
      </Modal>

      {/* 新增角色弹窗 */}
      <Modal
        opened={addModalOpen}
        onClose={() => setAddModalOpen(false)}
        title="新增角色"
        size="md"
      >
        <Stack gap="md">
          <TextInput
            label="角色标识"
            placeholder="例如：admin、user"
            required
            value={newRoleKey}
            onChange={(e) => setNewRoleKey(e.target.value)}
          />
          <TextInput
            label="角色名称"
            placeholder="例如：管理员、普通用户"
            required
            value={newRoleName}
            onChange={(e) => setNewRoleName(e.target.value)}
          />
          <TextInput
            label="描述"
            placeholder="角色描述（可选）"
            value={newRoleDesc}
            onChange={(e) => setNewRoleDesc(e.target.value)}
          />
          <Group justify="flex-end" mt="md">
            <Button variant="default" onClick={() => setAddModalOpen(false)}>
              取消
            </Button>
            <Button onClick={handleAddRole} loading={adding}>
              保存
            </Button>
          </Group>
        </Stack>
      </Modal>
    </Layout>
  );
};

export default PermissionsPage;
