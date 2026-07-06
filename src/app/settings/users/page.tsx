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
  TextInput,
  Select,
  Switch,
  Loader,
  Alert,
  Badge,
  PasswordInput,
  Checkbox,
} from '@mantine/core';
import {
  IconAlertCircle,
  IconTrash,
  IconEdit,
  IconUserPlus,
  IconKey,
  IconRefresh,
  IconShield,
  IconBuildingStore,
} from '@tabler/icons-react';
import { notifySuccess, notifyError } from '@/utils/notify';
import { useApi } from '@/hooks/useApi';
import {
  getUsers,
  insertUser,
  updateUser,
  deleteUser,
  resetPassword,
  type User,
} from '@/services/userService';
import { getDepartments, type Department } from '@/services/departmentService';
import { getRoles, getUserRoleIds, assignUserRoles, type Role } from '@/services/permissionService';
import { getTenants, getUserTenants, assignUserTenants, type Tenant } from '@/services/tenantService';
import { useAuthStore } from '@/store/authStore';

const UsersPage: React.FC = () => {
  const currentUser = useAuthStore((s) => s.user);
  const isSuperAdmin = currentUser?.is_super_admin ?? false;

  const [users, setUsers] = useState<User[]>([]);
  const [departments, setDepartments] = useState<Department[]>([]);
  const [tenants, setTenants] = useState<Tenant[]>([]);

  // 搜索筛选
  const [searchKeyword, setSearchKeyword] = useState('');
  const [filterTenantId, setFilterTenantId] = useState<number | null>(null);


  // 使用 useApi 管理数据获取
  const {
    data: fetchedUsers,
    loading,
    error,
    execute: fetchUsers,
  } = useApi(getUsers);

  // 使用 useApi 管理增删改操作
  const { execute: doInsertUser, loading: adding } = useApi(insertUser);
  const { execute: doUpdateUser, loading: editing } = useApi(updateUser);
  const { execute: doDeleteUser, loading: deleting } = useApi(deleteUser);
  const { execute: doResetPassword, loading: resetting } = useApi(resetPassword);

  // 新增用户弹窗
  const [addModalOpen, setAddModalOpen] = useState(false);
  const [newUser, setNewUser] = useState({
    username: '',
    password: '',
    real_name: '',
    email: '',
    phone: '',
    department_id: null as number | null,
    tenant_id: null as number | null,
    status: 1,
    nickname: '',
    person_code: '',
  });

  // 编辑用户弹窗
  const [editModalOpen, setEditModalOpen] = useState(false);
  const [editingUser, setEditingUser] = useState<User | null>(null);
  const [editForm, setEditForm] = useState({
    username: '',
    real_name: '',
    email: '',
    phone: '',
    department_id: null as number | null,
    status: 1,
    nickname: '',
    person_code: '',
  });

  // 删除确认弹窗
  const [deleteModalOpen, setDeleteModalOpen] = useState(false);
  const [deleteTargetUser, setDeleteTargetUser] = useState<User | null>(null);

  // 重置密码弹窗
  const [resetPwdModalOpen, setResetPwdModalOpen] = useState(false);
  const [resetPwdUser, setResetPwdUser] = useState<User | null>(null);
  const [newPassword, setNewPassword] = useState('');

  // 分配租户弹窗
  const [tenantModalOpen, setTenantModalOpen] = useState(false);
  const [tenantModalUser, setTenantModalUser] = useState<User | null>(null);
  const [selectedTenantIds, setSelectedTenantIds] = useState<number[]>([]);
  const [tenantModalLoading, setTenantModalLoading] = useState(false);
  const { execute: doAssignUserTenants, loading: assigningTenants } = useApi(assignUserTenants);

  // 分配角色弹窗
  const [roleModalOpen, setRoleModalOpen] = useState(false);
  const [roleModalUser, setRoleModalUser] = useState<User | null>(null);
  const [roles, setRoles] = useState<Role[]>([]);
  const [selectedRoleIds, setSelectedRoleIds] = useState<string[]>([]);
  const [roleModalLoading, setRoleModalLoading] = useState(false);
  const { execute: doAssignUserRoles, loading: assigningRoles } = useApi(assignUserRoles);

  // 加载用户列表（带搜索筛选）
  const loadUsers = React.useCallback(() => {
    // 非超级管理员只查询自己机构的用户
    if (!isSuperAdmin && currentUser) {
      fetchUsers(currentUser.tenant_id, searchKeyword || undefined);
    } else {
      fetchUsers(filterTenantId, searchKeyword || undefined);
    }
  }, [fetchUsers, isSuperAdmin, currentUser, searchKeyword, filterTenantId]);


  // 当 fetchedUsers 变化时更新本地状态
  useEffect(() => {
    if (fetchedUsers) {
      setUsers(fetchedUsers);
    }
  }, [fetchedUsers]);

  useEffect(() => {
    loadUsers();
    fetchDepartments();
    if (isSuperAdmin) {
      fetchTenants();
    }
  }, []);

  const fetchDepartments = async () => {
    try {
      const data = await getDepartments();
      setDepartments(data);
    } catch {
      console.warn('获取部门列表失败，部门选择将不可用');
    }
  };

  const fetchTenants = async () => {
    try {
      const data = await getTenants();
      setTenants(data);
    } catch {
      console.warn('获取机构列表失败');
    }
  };

  const departmentOptions = departments.map((d) => ({
    value: String(d.id),
    label: d.department_name,
  }));

  const tenantOptions = tenants.map((t) => ({
    value: String(t.id),
    label: t.tenant_name,
  }));

  // 新增用户
  const handleAddUser = async () => {
    if (!newUser.username.trim()) {
      notifyError('验证失败', '请输入用户名');
      return;
    }
    if (!newUser.password.trim()) {
      notifyError('验证失败', '请输入密码');
      return;
    }
    if (!newUser.real_name.trim()) {
      notifyError('验证失败', '请输入真实姓名');
      return;
    }

    try {
      await doInsertUser({
        username: newUser.username.trim(),
        password: newUser.password,
        realName: newUser.real_name.trim(),
        email: newUser.email.trim() || null,
        phone: newUser.phone.trim() || null,
        departmentId: newUser.department_id,
        status: newUser.status,
        nickname: newUser.nickname.trim() || null,
        personId: null,
        personCode: newUser.person_code.trim() || null,
        superUserId: null,
        tenantId: newUser.tenant_id,
        createdBy: null,
      });
      setAddModalOpen(false);
      setNewUser({
        username: '',
        password: '',
        real_name: '',
        email: '',
        phone: '',
        department_id: null,
        tenant_id: null,
        status: 1,
        nickname: '',
        person_code: '',
      });
      notifySuccess('用户添加成功');
      loadUsers();
    } catch (err) {
      console.error('新增用户失败:', err);
      notifyError('新增用户失败', typeof err === 'string' ? err : undefined);
    }
  };

  // 打开编辑弹窗
  const openEditModal = (user: User) => {
    setEditingUser(user);
    setEditForm({
      username: user.username,
      real_name: user.real_name,
      email: user.email || '',
      phone: user.phone || '',
      department_id: user.department_id,
      status: user.status,
      nickname: user.nickname || '',
      person_code: user.person_code || '',
    });
    setEditModalOpen(true);
  };

  // 编辑用户
  const handleEditUser = async () => {
    if (!editingUser) return;
    if (!editForm.username.trim()) {
      notifyError('验证失败', '请输入用户名');
      return;
    }
    if (!editForm.real_name.trim()) {
      notifyError('验证失败', '请输入真实姓名');
      return;
    }

    try {
      await doUpdateUser({
        id: editingUser.id,
        username: editForm.username.trim(),
        realName: editForm.real_name.trim(),
        email: editForm.email.trim() || null,
        phone: editForm.phone.trim() || null,
        departmentId: editForm.department_id,
        status: editForm.status,
        nickname: editForm.nickname.trim() || null,
        personId: null,
        personCode: editForm.person_code.trim() || null,
        superUserId: null,
        updatedBy: null,
      });
      setEditModalOpen(false);
      setEditingUser(null);
      notifySuccess('用户更新成功');
      loadUsers();
    } catch (err) {
      console.error('更新用户失败:', err);
      notifyError('更新用户失败', typeof err === 'string' ? err : undefined);
    }
  };

  // 打开删除确认弹窗
  const openDeleteModal = (user: User) => {
    setDeleteTargetUser(user);
    setDeleteModalOpen(true);
  };

  // 确认删除用户
  const handleDeleteUser = async () => {
    if (!deleteTargetUser || !currentUser) return;
    try {
      await doDeleteUser(deleteTargetUser.id, currentUser.id, isSuperAdmin);
      setDeleteModalOpen(false);
      setDeleteTargetUser(null);
      notifySuccess('用户删除成功');
      loadUsers();
    } catch (err) {
      console.error('删除用户失败:', err);
      notifyError('删除用户失败', typeof err === 'string' ? err : undefined);
    }
  };

  // 打开重置密码弹窗
  const openResetPwdModal = (user: User) => {
    setResetPwdUser(user);
    setNewPassword('');
    setResetPwdModalOpen(true);
  };

  // 确认重置密码
  const handleResetPassword = async () => {
    if (!resetPwdUser) return;
    if (!newPassword.trim()) {
      notifyError('验证失败', '请输入新密码');
      return;
    }
    if (newPassword.length < 6) {
      notifyError('验证失败', '密码长度不能少于6位');
      return;
    }

    try {
      await doResetPassword(resetPwdUser.id, newPassword);
      setResetPwdModalOpen(false);
      setResetPwdUser(null);
      setNewPassword('');
      notifySuccess('密码重置成功');
    } catch (err) {
      console.error('重置密码失败:', err);
      notifyError('重置密码失败', typeof err === 'string' ? err : undefined);
    }
  };

  const getStatusBadge = (status: number) => {
    if (status === 1) {
      return <Badge color="green">启用</Badge>;
    }
    return <Badge color="red">禁用</Badge>;
  };

  // 打开分配角色弹窗
  const openAssignRoleModal = async (user: User) => {
    setRoleModalUser(user);
    setRoleModalOpen(true);
    setRoleModalLoading(true);

    try {
      const roleList = await getRoles();
      setRoles(roleList);
      const userRoleIds = await getUserRoleIds(String(user.id));
      setSelectedRoleIds(userRoleIds.map(String));
    } catch (err) {
      console.error('获取角色数据失败:', err);
      notifyError('获取角色数据失败');
    } finally {
      setRoleModalLoading(false);
    }
  };

  // 保存分配角色
  const handleAssignRoles = async () => {
    if (!roleModalUser) return;
    try {
      await doAssignUserRoles(String(roleModalUser.id), selectedRoleIds);
      setRoleModalOpen(false);
      setRoleModalUser(null);
      setSelectedRoleIds([]);
      notifySuccess('角色分配成功');
    } catch (err) {
      console.error('分配角色失败:', err);
      notifyError('分配角色失败', typeof err === 'string' ? err : undefined);
    }
  };

  // 打开分配租户弹窗
  const openTenantModal = async (user: User) => {
    setTenantModalUser(user);
    setTenantModalOpen(true);
    setTenantModalLoading(true);

    try {
      // 获取所有启用的租户列表
      const allTenants = await getTenants();
      setTenants(allTenants);
      // 获取用户当前已分配的租户（user.id 已经是后端传来的原始 bigint，直接传给服务函数会再转成字符串）
      const userTenants = await getUserTenants(user.id);
      setSelectedTenantIds(userTenants.map((t: Tenant) => t.id));
    } catch (err) {
      console.error('获取租户数据失败:', err);
      notifyError('获取租户数据失败');
    } finally {
      setTenantModalLoading(false);
    }
  };

  // 保存分配租户
  const handleAssignTenants = async () => {
    if (!tenantModalUser || !currentUser) return;
    try {
      // 使用原生的用户ID值（从后端传来的大整数可能已超出 JS number 精度，
      // 直接作为 number 传入 tenantService 会再转成 String）
      // 此处使用显式 String() 转换以匹配后端 i64_to_string 序列化格式
      await doAssignUserTenants(tenantModalUser.id, selectedTenantIds, currentUser.id);
      setTenantModalOpen(false);
      setTenantModalUser(null);
      setSelectedTenantIds([]);
      notifySuccess('租户分配成功');
    } catch (err) {
      console.error('分配租户失败:', err);
      notifyError('分配租户失败', typeof err === 'string' ? err : undefined);
    }
  };

  // 判断当前用户是否有权限删除目标用户
  const canDeleteUser = (targetUser: User): boolean => {
    // 超级管理员不能被任何人删除
    if (targetUser.is_super_admin) return false;
    // 非超级管理员只能删除本机构的用户
    if (!isSuperAdmin && currentUser) {
      return currentUser.tenant_id === targetUser.tenant_id;
    }
    return true;
  };

  return (
    <Layout>
      <Stack gap="lg">
        <Group justify="space-between">
          <div>
            <Title order={2}>用户管理</Title>
            <Text c="dimmed">管理系统用户账号</Text>
          </div>
          <Group>
            <Button
              variant="light"
              leftSection={<IconRefresh size={16} />}
              onClick={loadUsers}
              loading={loading}
            >
              刷新
            </Button>
            <Button
              leftSection={<IconUserPlus size={16} />}
              onClick={() => {
                // 非超级管理员新增用户时，自动填充当前用户的机构
                if (!isSuperAdmin && currentUser) {
                  setNewUser(prev => ({ ...prev, tenant_id: currentUser.tenant_id }));
                }
                setAddModalOpen(true);
              }}
            >
              新增用户
            </Button>
          </Group>
        </Group>

        {error && (
          <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red">
            {error}
          </Alert>
        )}

        {/* 搜索筛选区域 */}
        <Card withBorder padding="sm" radius="md">
          <Group>
            {isSuperAdmin && (
              <Select
                placeholder="选择所属机构"
                clearable
                data={[{ value: '', label: '全部机构' }, ...tenantOptions]}
                value={filterTenantId !== null ? String(filterTenantId) : ''}
                onChange={(value) => {
                  setFilterTenantId(value ? Number(value) : null);
                }}
                style={{ minWidth: 200 }}
              />
            )}
            <TextInput
              placeholder="搜索用户名/真实姓名"
              value={searchKeyword}
              onChange={(e) => setSearchKeyword(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  loadUsers();
                }
              }}
              style={{ minWidth: 250 }}
            />
            <Button variant="light" onClick={loadUsers}>
              搜索
            </Button>
          </Group>
        </Card>

        <Card withBorder padding="lg" radius="md">

          {loading ? (
            <Group justify="center" py="xl">
              <Loader />
            </Group>
          ) : (
            <Table striped highlightOnHover>
              <Table.Thead>
                <Table.Tr>
                  <Table.Th>用户名</Table.Th>
                  <Table.Th>真实姓名</Table.Th>
                  <Table.Th>工号</Table.Th>
                  <Table.Th>邮箱</Table.Th>
                  <Table.Th>手机</Table.Th>
                  <Table.Th>超级管理员</Table.Th>
                  <Table.Th>所属机构</Table.Th>
                  <Table.Th>状态</Table.Th>
                  <Table.Th style={{ width: 480 }}>操作</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {users.length === 0 ? (
                  <Table.Tr>
                    <Table.Td colSpan={9}>
                      <Text ta="center" c="dimmed" py="xl">
                        暂无用户数据
                      </Text>
                    </Table.Td>
                  </Table.Tr>
                ) : (
                  users.map((user) => (
                    <Table.Tr key={user.id}>
                      <Table.Td>
                        <Text fw={500}>{user.username}</Text>
                      </Table.Td>
                      <Table.Td>{user.real_name}</Table.Td>
                      <Table.Td>
                        <Text size="sm" c="dimmed">
                          {user.person_code || '-'}
                        </Text>
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm">{user.email || '-'}</Text>
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm">{user.phone || '-'}</Text>
                      </Table.Td>
                      <Table.Td>
                        {user.is_super_admin ? (
                          <Badge color="red">是</Badge>
                        ) : (
                          <Badge color="gray" variant="light">否</Badge>
                        )}
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm">{user.tenant_name || '-'}</Text>
                      </Table.Td>
                      <Table.Td>{getStatusBadge(user.status)}</Table.Td>
                      <Table.Td>
                        <Group gap="xs">
                          <Button
                            size="xs"
                            variant="light"
                            color="teal"
                            leftSection={<IconBuildingStore size={14} />}
                            onClick={() => openTenantModal(user)}
                            disabled={user.is_super_admin}
                          >
                            分配租户
                          </Button>
                          <Button
                            size="xs"
                            variant="light"
                            color="violet"
                            leftSection={<IconShield size={14} />}
                            onClick={() => openAssignRoleModal(user)}
                          >
                            分配角色
                          </Button>
                          <Button
                            size="xs"
                            variant="light"
                            leftSection={<IconEdit size={14} />}
                            onClick={() => openEditModal(user)}
                          >
                            编辑
                          </Button>
                          <Button
                            size="xs"
                            variant="light"
                            color="yellow"
                            leftSection={<IconKey size={14} />}
                            onClick={() => openResetPwdModal(user)}
                          >
                            重置密码
                          </Button>
                          <Button
                            size="xs"
                            variant="light"
                            color="red"
                            leftSection={<IconTrash size={14} />}
                            onClick={() => openDeleteModal(user)}
                            disabled={!canDeleteUser(user)}
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

      {/* 新增用户弹窗 */}
      <Modal
        opened={addModalOpen}
        onClose={() => setAddModalOpen(false)}
        title="新增用户"
        size="lg"
      >
        <Stack gap="md">
          <TextInput
            label="用户名"
            placeholder="请输入用户名"
            required
            value={newUser.username}
            onChange={(e) =>
              setNewUser({ ...newUser, username: e.target.value })
            }
          />
          <PasswordInput
            label="密码"
            placeholder="请输入密码"
            required
            value={newUser.password}
            onChange={(e) =>
              setNewUser({ ...newUser, password: e.target.value })
            }
          />
          <TextInput
            label="真实姓名"
            placeholder="请输入真实姓名"
            required
            value={newUser.real_name}
            onChange={(e) =>
              setNewUser({ ...newUser, real_name: e.target.value })
            }
          />
          <TextInput
            label="工号"
            placeholder="请输入工号（可选）"
            value={newUser.person_code}
            onChange={(e) =>
              setNewUser({ ...newUser, person_code: e.target.value })
            }
          />
          <TextInput
            label="昵称"
            placeholder="请输入昵称（可选）"
            value={newUser.nickname}
            onChange={(e) =>
              setNewUser({ ...newUser, nickname: e.target.value })
            }
          />
          <TextInput
            label="邮箱"
            placeholder="请输入邮箱（可选）"
            value={newUser.email}
            onChange={(e) =>
              setNewUser({ ...newUser, email: e.target.value })
            }
          />
          <TextInput
            label="手机"
            placeholder="请输入手机号码（可选）"
            value={newUser.phone}
            onChange={(e) =>
              setNewUser({ ...newUser, phone: e.target.value })
            }
          />
          <Select
            label="部门"
            placeholder="请选择部门（可选）"
            clearable
            data={departmentOptions}
            value={
              newUser.department_id !== null
                ? String(newUser.department_id)
                : null
            }
            onChange={(value) =>
              setNewUser({
                ...newUser,
                department_id: value ? Number(value) : null,
              })
            }
          />
          {/* 所属机构选择 */}
          {/* 超级管理员：可选机构（可为空，因为超级管理员不属于任何机构） */}
          {/* 普通管理员：自动从当前登录用户获取，不可更改 */}
          <Select
            label="所属机构"
            placeholder={isSuperAdmin ? "请选择机构（可选）" : "自动获取"}
            clearable={isSuperAdmin}
            disabled={!isSuperAdmin}
            data={tenantOptions}
            value={
              newUser.tenant_id !== null
                ? String(newUser.tenant_id)
                : null
            }
            onChange={(value) =>
              setNewUser({
                ...newUser,
                tenant_id: value ? Number(value) : null,
              })
            }
          />
          <Switch
            label="用户状态"
            checked={newUser.status === 1}
            onLabel="启用"
            offLabel="禁用"
            onChange={(e) =>
              setNewUser({
                ...newUser,
                status: e.currentTarget.checked ? 1 : 0,
              })
            }
          />
          <Group justify="flex-end" mt="md">
            <Button variant="default" onClick={() => setAddModalOpen(false)}>
              取消
            </Button>
            <Button onClick={handleAddUser} loading={adding}>
              保存
            </Button>
          </Group>
        </Stack>
      </Modal>

      {/* 编辑用户弹窗 */}
      <Modal
        opened={editModalOpen}
        onClose={() => setEditModalOpen(false)}
        title={`编辑用户 - ${editingUser?.username || ''}`}
        size="lg"
      >
        <Stack gap="md">
          <TextInput
            label="用户名"
            placeholder="请输入用户名"
            required
            value={editForm.username}
            onChange={(e) =>
              setEditForm({ ...editForm, username: e.target.value })
            }
          />
          <TextInput
            label="真实姓名"
            placeholder="请输入真实姓名"
            required
            value={editForm.real_name}
            onChange={(e) =>
              setEditForm({ ...editForm, real_name: e.target.value })
            }
          />
          <TextInput
            label="工号"
            placeholder="请输入工号（可选）"
            value={editForm.person_code}
            onChange={(e) =>
              setEditForm({ ...editForm, person_code: e.target.value })
            }
          />
          <TextInput
            label="昵称"
            placeholder="请输入昵称（可选）"
            value={editForm.nickname}
            onChange={(e) =>
              setEditForm({ ...editForm, nickname: e.target.value })
            }
          />
          <TextInput
            label="邮箱"
            placeholder="请输入邮箱（可选）"
            value={editForm.email}
            onChange={(e) =>
              setEditForm({ ...editForm, email: e.target.value })
            }
          />
          <TextInput
            label="手机"
            placeholder="请输入手机号码（可选）"
            value={editForm.phone}
            onChange={(e) =>
              setEditForm({ ...editForm, phone: e.target.value })
            }
          />
          <Select
            label="部门"
            placeholder="请选择部门（可选）"
            clearable
            data={departmentOptions}
            value={
              editForm.department_id !== null
                ? String(editForm.department_id)
                : null
            }
            onChange={(value) =>
              setEditForm({
                ...editForm,
                department_id: value ? Number(value) : null,
              })
            }
          />
          {/* 编辑时显示所属机构（只读） */}
          <TextInput
            label="所属机构"
            value={editingUser?.tenant_name || (editingUser?.is_super_admin ? '超级管理员（不属于任何机构）' : '-')}
            disabled
          />
          <Switch
            label="用户状态"
            checked={editForm.status === 1}
            onLabel="启用"
            offLabel="禁用"
            onChange={(e) =>
              setEditForm({
                ...editForm,
                status: e.currentTarget.checked ? 1 : 0,
              })
            }
          />
          <Group justify="flex-end" mt="md">
            <Button variant="default" onClick={() => setEditModalOpen(false)}>
              取消
            </Button>
            <Button onClick={handleEditUser} loading={editing}>
              保存修改
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
            确定要删除用户 <strong>{deleteTargetUser?.real_name}</strong>（
            {deleteTargetUser?.username}）吗？
          </Text>
          <Text size="sm" c="dimmed">
            此操作将软删除该用户，用户将无法登录系统，但数据仍可恢复。
          </Text>
          <Group justify="flex-end" mt="md">
            <Button variant="default" onClick={() => setDeleteModalOpen(false)}>
              取消
            </Button>
            <Button color="red" onClick={handleDeleteUser} loading={deleting}>
              确认删除
            </Button>
          </Group>
        </Stack>
      </Modal>

      {/* 分配角色弹窗 */}
      <Modal
        opened={roleModalOpen}
        onClose={() => setRoleModalOpen(false)}
        title={`分配角色 - ${roleModalUser?.real_name || ''}`}
        size="md"
      >
        <Stack gap="md">
          {roleModalLoading ? (
            <Group justify="center" py="xl">
              <Loader />
            </Group>
          ) : (
            <>
              <Text size="sm" c="dimmed">
                请选择要分配给该用户的角色：
              </Text>
              {roles.length === 0 ? (
                <Text ta="center" c="dimmed" py="md">
                  暂无可用角色
                </Text>
              ) : (
                <Stack gap="xs">
                  {roles.map((role) => (
                    <Checkbox
                      key={role.id}
                      label={role.role_name}
                      description={role.description || ''}
                      checked={selectedRoleIds.includes(role.id)}
                      onChange={(e: React.ChangeEvent<HTMLInputElement>) => {
                        if (e.currentTarget.checked) {
                          setSelectedRoleIds([...selectedRoleIds, role.id]);
                        } else {
                          setSelectedRoleIds(
                            selectedRoleIds.filter((id) => id !== role.id)
                          );
                        }
                      }}
                    />
                  ))}
                </Stack>
              )}
            </>
          )}
          <Group justify="flex-end" mt="md">
            <Button variant="default" onClick={() => setRoleModalOpen(false)}>
              取消
            </Button>
            <Button
              color="violet"
              onClick={handleAssignRoles}
              loading={assigningRoles}
            >
              保存
            </Button>
          </Group>
        </Stack>
      </Modal>

      {/* 分配租户弹窗 */}
      <Modal
        opened={tenantModalOpen}
        onClose={() => setTenantModalOpen(false)}
        title={`分配租户 - ${tenantModalUser?.real_name || ''}`}
        size="md"
      >
        <Stack gap="md">
          {tenantModalLoading ? (
            <Group justify="center" py="xl">
              <Loader />
            </Group>
          ) : (
            <>
              <Text size="sm" c="dimmed">
                请选择该用户可以访问的租户：
              </Text>
              {tenants.length === 0 ? (
                <Text ta="center" c="dimmed" py="md">
                  暂无可用租户
                </Text>
              ) : (
                <Stack gap="xs">
                  {tenants.map((tenant) => (
                    <Checkbox
                      key={tenant.id}
                      label={tenant.tenant_name}
                      description={tenant.schema_name || ''}
                      checked={selectedTenantIds.includes(tenant.id)}
                      onChange={(e: React.ChangeEvent<HTMLInputElement>) => {
                        if (e.currentTarget.checked) {
                          setSelectedTenantIds([...selectedTenantIds, tenant.id]);
                        } else {
                          setSelectedTenantIds(
                            selectedTenantIds.filter((id) => id !== tenant.id)
                          );
                        }
                      }}
                    />
                  ))}
                </Stack>
              )}
            </>
          )}
          <Group justify="flex-end" mt="md">
            <Button variant="default" onClick={() => setTenantModalOpen(false)}>
              取消
            </Button>
            <Button
              color="teal"
              onClick={handleAssignTenants}
              loading={assigningTenants}
            >
              保存
            </Button>
          </Group>
        </Stack>
      </Modal>

      {/* 重置密码弹窗 */}
      <Modal
        opened={resetPwdModalOpen}
        onClose={() => setResetPwdModalOpen(false)}
        title={`重置密码 - ${resetPwdUser?.real_name || ''}`}
        size="sm"
      >
        <Stack gap="md">
          <PasswordInput
            label="新密码"
            placeholder="请输入新密码（至少6位）"
            required
            value={newPassword}
            onChange={(e) => setNewPassword(e.target.value)}
          />
          <Group justify="flex-end" mt="md">
            <Button
              variant="default"
              onClick={() => setResetPwdModalOpen(false)}
            >
              取消
            </Button>
            <Button
              color="yellow"
              onClick={handleResetPassword}
              loading={resetting}
            >
              确认重置
            </Button>
          </Group>
        </Stack>
      </Modal>
    </Layout>
  );
};

export default UsersPage;