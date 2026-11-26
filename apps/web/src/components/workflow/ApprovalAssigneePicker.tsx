'use client';

import React, { useEffect, useMemo, useState } from 'react';
import {
    ActionIcon,
    Alert,
    Badge,
    Box,
    Button,
    Checkbox,
    Divider,
    Group,
    Loader,
    Modal,
    ScrollArea,
    Stack,
    Table,
    Text,
    TextInput,
} from '@mantine/core';
import {
    IconAlertCircle,
    IconBuilding,
    IconChevronDown,
    IconChevronRight,
    IconSearch,
    IconSelector,
    IconUser,
    IconX,
} from '@tabler/icons-react';
import { getDepartments, type Department } from '@/services/departmentService';
import { getUsers, type User } from '@/services/userService';
import {
    buildDepartmentTree,
    departmentMatches,
    getDepartmentDescendantIds,
    getDepartmentPath,
    type DepartmentTreeNode,
} from './departmentTreeUtils';

export interface AssigneeOption {
    id: string;
    name: string;
    departmentId?: string | null;
}

export interface ApprovalAssigneePickerProps {
    value: string[];
    valueNames?: string[];
    onChange: (ids: string[], options: AssigneeOption[]) => void;
    multiple?: boolean;
    tenantId?: string | null;
    disabled?: boolean;
    label?: string;
    placeholder?: string;
}

function toOption(user: User): AssigneeOption {
    return {
        id: String(user.id),
        name: user.real_name || user.username,
        departmentId: user.department_id ? String(user.department_id) : null,
    };
}

export default function ApprovalAssigneePicker({
    value,
    valueNames,
    onChange,
    multiple = false,
    tenantId,
    disabled = false,
    label = '审批人',
    placeholder = '请选择审批人',
}: ApprovalAssigneePickerProps) {
    const [opened, setOpened] = useState(false);
    const [departments, setDepartments] = useState<Department[]>([]);
    const [users, setUsers] = useState<User[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [deptKeyword, setDeptKeyword] = useState('');
    const [userKeyword, setUserKeyword] = useState('');
    const [selectedDeptId, setSelectedDeptId] = useState<string | null>(null);
    const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());
    const [selectedIds, setSelectedIds] = useState<string[]>(value);

    useEffect(() => {
        let mounted = true;
        setLoading(true);
        setError(null);
        Promise.all([getDepartments(tenantId), getUsers(tenantId)])
            .then(([deptList, userList]) => {
                if (!mounted) return;
                setDepartments(Array.isArray(deptList) ? deptList : []);
                setUsers(Array.isArray(userList) ? userList : []);
                setExpandedIds(
                    new Set((Array.isArray(deptList) ? deptList : []).map((dept) => String(dept.id)))
                );
            })
            .catch((err) => {
                if (!mounted) return;
                console.error('加载部门/用户失败:', err);
                setError(typeof err === 'string' ? err : '加载部门/用户失败');
            })
            .finally(() => {
                if (mounted) setLoading(false);
            });
        return () => {
            mounted = false;
        };
    }, [tenantId]);

    useEffect(() => {
        setSelectedIds(value);
    }, [value]);

    const tree = useMemo(() => buildDepartmentTree(departments), [departments]);
    const selectedSet = useMemo(() => new Set(selectedIds), [selectedIds]);

    const selectedOptions = useMemo(() => {
        return selectedIds.map((id, index) => {
            const user = users.find((item) => String(item.id) === id);
            if (user) return toOption(user);
            return {
                id,
                name: valueNames?.[index] || `用户#${id}`,
                departmentId: null,
            };
        });
    }, [selectedIds, users, valueNames]);

    const selectedText =
        selectedOptions.length > 0
            ? selectedOptions.map((item) => item.name).join('、')
            : placeholder;

    const filteredUsers = useMemo(() => {
        const departmentIds = selectedDeptId
            ? getDepartmentDescendantIds(departments, selectedDeptId)
            : null;
        const keyword = userKeyword.trim().toLowerCase();
        return users.filter((user) => {
            if (user.status === 0) return false;
            if (
                departmentIds &&
                (!user.department_id || !departmentIds.has(String(user.department_id)))
            ) {
                return false;
            }
            if (!keyword) return true;
            return (
                (user.real_name || '').toLowerCase().includes(keyword) ||
                (user.username || '').toLowerCase().includes(keyword)
            );
        });
    }, [users, departments, selectedDeptId, userKeyword]);

    const toggleExpanded = (id: string) => {
        setExpandedIds((prev) => {
            const next = new Set(prev);
            if (next.has(id)) next.delete(id);
            else next.add(id);
            return next;
        });
    };

    const toggleUser = (user: User) => {
        const id = String(user.id);
        setSelectedIds((prev) => {
            if (multiple) {
                return prev.includes(id) ? prev.filter((item) => item !== id) : [...prev, id];
            }
            return prev.includes(id) ? [] : [id];
        });
    };

    const removeSelected = (id: string) => {
        setSelectedIds((prev) => prev.filter((item) => item !== id));
    };

    const close = () => {
        setSelectedIds(value);
        setOpened(false);
    };

    const confirm = () => {
        onChange(selectedIds, selectedOptions);
        setOpened(false);
    };

    const nodeVisible = (node: DepartmentTreeNode): boolean => {
        if (!deptKeyword.trim()) return true;
        return (
            departmentMatches(departments, node, deptKeyword) ||
            node.children.some((child) => nodeVisible(child))
        );
    };

    const renderNode = (node: DepartmentTreeNode, depth = 0): React.ReactNode => {
        if (!nodeVisible(node)) return null;
        const id = String(node.id);
        const hasChildren = node.children.length > 0;
        const expanded = Boolean(deptKeyword.trim()) || expandedIds.has(id);
        const active = selectedDeptId === id;
        return (
            <React.Fragment key={id}>
                <Group
                    gap={4}
                    wrap="nowrap"
                    onClick={() => setSelectedDeptId(id)}
                    style={{
                        padding: `4px 6px 4px ${8 + depth * 16}px`,
                        borderRadius: 6,
                        cursor: 'pointer',
                        backgroundColor: active
                            ? 'var(--mantine-color-blue-light)'
                            : 'transparent',
                    }}
                >
                    {hasChildren ? (
                        <ActionIcon
                            size="xs"
                            variant="subtle"
                            onClick={(event) => {
                                event.stopPropagation();
                                toggleExpanded(id);
                            }}
                        >
                            {expanded ? (
                                <IconChevronDown size={12} />
                            ) : (
                                <IconChevronRight size={12} />
                            )}
                        </ActionIcon>
                    ) : (
                        <Box w={18} />
                    )}
                    <IconBuilding size={14} />
                    <Text size="sm" lineClamp={1} style={{ flex: 1 }}>
                        {node.department_name}
                    </Text>
                </Group>
                {hasChildren && expanded && (
                    <>{node.children.map((child) => renderNode(child, depth + 1))}</>
                )}
            </React.Fragment>
        );
    };

    return (
        <>
            <Stack gap={4}>
                <Text size="xs" fw={500} c="dimmed">
                    {label}
                </Text>
                <Button
                    variant="default"
                    fullWidth
                    disabled={disabled}
                    rightSection={<IconSelector size={16} />}
                    onClick={() => setOpened(true)}
                    styles={{
                        root: {
                            justifyContent: 'space-between',
                            fontWeight: 400,
                            color: selectedOptions.length > 0 ? undefined : '#868e96',
                        },
                    }}
                >
                    <Text size="sm" lineClamp={1}>
                        {selectedText}
                    </Text>
                </Button>
            </Stack>

            <Modal
                opened={opened}
                onClose={close}
                title={label}
                size="xl"
            >
                <Stack gap="sm">
                    {error && (
                        <Alert color="red" icon={<IconAlertCircle size={16} />}>
                            {error}
                        </Alert>
                    )}
                    {loading ? (
                        <Group justify="center" py="xl">
                            <Loader size="sm" />
                            <Text size="sm" c="dimmed">
                                加载部门和用户...
                            </Text>
                        </Group>
                    ) : (
                        <Group align="flex-start" wrap="nowrap" gap="md">
                            <Stack gap="xs" w={280} style={{ flexShrink: 0 }}>
                                <TextInput
                                    size="xs"
                                    placeholder="搜索部门"
                                    leftSection={<IconSearch size={14} />}
                                    value={deptKeyword}
                                    onChange={(event) => setDeptKeyword(event.currentTarget.value)}
                                />
                                <ScrollArea h={360} type="auto">
                                    <Stack gap={2}>
                                        <Group
                                            gap={6}
                                            onClick={() => setSelectedDeptId(null)}
                                            style={{
                                                padding: '4px 6px',
                                                borderRadius: 6,
                                                cursor: 'pointer',
                                                backgroundColor:
                                                    selectedDeptId === null
                                                        ? 'var(--mantine-color-blue-light)'
                                                        : 'transparent',
                                            }}
                                        >
                                            <IconUser size={14} />
                                            <Text size="sm">全部部门</Text>
                                        </Group>
                                        {tree.map((node) => renderNode(node))}
                                    </Stack>
                                </ScrollArea>
                            </Stack>

                            <Divider orientation="vertical" />

                            <Stack gap="xs" style={{ flex: 1, minWidth: 0 }}>
                                <TextInput
                                    size="xs"
                                    placeholder="搜索姓名 / 用户名"
                                    leftSection={<IconSearch size={14} />}
                                    value={userKeyword}
                                    onChange={(event) => setUserKeyword(event.currentTarget.value)}
                                />
                                <Text size="xs" c="dimmed">
                                    {selectedDeptId
                                        ? `当前部门：${getDepartmentPath(departments, selectedDeptId)}`
                                        : '全部部门'}
                                    {' · '}
                                    {filteredUsers.length} 位用户
                                </Text>
                                <ScrollArea h={330} type="auto">
                                    <Table highlightOnHover striped withTableBorder>
                                        <Table.Thead>
                                            <Table.Tr>
                                                {multiple && <Table.Th w={40} />}
                                                <Table.Th>姓名</Table.Th>
                                                <Table.Th>用户名</Table.Th>
                                                <Table.Th>部门</Table.Th>
                                            </Table.Tr>
                                        </Table.Thead>
                                        <Table.Tbody>
                                            {filteredUsers.map((user) => {
                                                const id = String(user.id);
                                                const checked = selectedSet.has(id);
                                                return (
                                                    <Table.Tr
                                                        key={id}
                                                        onClick={() => toggleUser(user)}
                                                        style={{ cursor: 'pointer' }}
                                                    >
                                                        {multiple && (
                                                            <Table.Td>
                                                                <Checkbox
                                                                    checked={checked}
                                                                    onChange={() => toggleUser(user)}
                                                                    onClick={(event) =>
                                                                        event.stopPropagation()
                                                                    }
                                                                />
                                                            </Table.Td>
                                                        )}
                                                        <Table.Td>
                                                            <Text size="sm">
                                                                {user.real_name || user.username}
                                                            </Text>
                                                        </Table.Td>
                                                        <Table.Td>
                                                            <Text size="sm" c="dimmed">
                                                                {user.username}
                                                            </Text>
                                                        </Table.Td>
                                                        <Table.Td>
                                                            <Text size="xs" c="dimmed">
                                                                {user.department_id
                                                                    ? getDepartmentPath(
                                                                          departments,
                                                                          String(user.department_id)
                                                                      )
                                                                    : '-'}
                                                            </Text>
                                                        </Table.Td>
                                                    </Table.Tr>
                                                );
                                            })}
                                            {filteredUsers.length === 0 && (
                                                <Table.Tr>
                                                    <Table.Td colSpan={multiple ? 4 : 3}>
                                                        <Text ta="center" c="dimmed" py="md">
                                                            没有匹配的用户
                                                        </Text>
                                                    </Table.Td>
                                                </Table.Tr>
                                            )}
                                        </Table.Tbody>
                                    </Table>
                                </ScrollArea>
                            </Stack>
                        </Group>
                    )}

                    <Divider />
                    <Group justify="space-between" align="flex-start">
                        <Group gap={4} style={{ flex: 1 }}>
                            {selectedOptions.length === 0 ? (
                                <Text size="xs" c="dimmed">
                                    尚未选择
                                </Text>
                            ) : (
                                selectedOptions.map((item) => (
                                    <Badge
                                        key={item.id}
                                        variant="light"
                                        rightSection={
                                            <IconX
                                                size={10}
                                                style={{ cursor: 'pointer' }}
                                                onClick={() => removeSelected(item.id)}
                                            />
                                        }
                                    >
                                        {item.name}
                                    </Badge>
                                ))
                            )}
                        </Group>
                        <Group gap="xs">
                            <Button
                                variant="subtle"
                                color="red"
                                size="xs"
                                disabled={selectedIds.length === 0}
                                onClick={() => setSelectedIds([])}
                            >
                                清空
                            </Button>
                            <Button variant="default" onClick={close}>
                                取消
                            </Button>
                            <Button onClick={confirm}>确认 ({selectedIds.length})</Button>
                        </Group>
                    </Group>
                </Stack>
            </Modal>
        </>
    );
}




