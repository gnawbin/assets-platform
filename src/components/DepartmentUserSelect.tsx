'use client';

import React, { useEffect, useState, useMemo } from 'react';
import {
    Paper,
    Stack,
    Group,
    Text,
    Radio,
    Select,
    Loader,
    Alert,
    ScrollArea,
    Divider,
    Button,
    Modal,
} from '@mantine/core';
import { IconAlertCircle, IconSelector } from '@tabler/icons-react';
import { getDepartments, type Department } from '@/services/departmentService';
import { getUsers, type User } from '@/services/userService';

// ======================== 类型定义 ========================

export interface DepartmentUserSelectProps {
    /** 当前选中的部门ID (null=ALL) */
    departmentId: string | null;
    /** 当前选中的用户ID (null=未选择) */
    userId: string | null;
    /** 部门选择变化回调 */
    onDepartmentChange: (departmentId: string | null) => void;
    /** 用户选择变化回调 */
    onUserChange: (userId: string | null) => void;
    /** 左侧面板标题 */
    departmentLabel?: string;
    /** 右侧面板标题 */
    userLabel?: string;
    /** 是否禁用 */
    disabled?: boolean;
}

// ======================== 组件 ========================

const DepartmentUserSelect: React.FC<DepartmentUserSelectProps> = ({
    departmentId,
    userId,
    onDepartmentChange,
    onUserChange,
    departmentLabel = '部门选择',
    userLabel = '用户选择',
    disabled = false,
}) => {
    const [departments, setDepartments] = useState<Department[]>([]);
    const [users, setUsers] = useState<User[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [opened, setOpened] = useState(false);

    // 加载部门列表和用户列表
    useEffect(() => {
        const loadData = async () => {
            setLoading(true);
            setError(null);
            try {
                const [deptData, userData] = await Promise.all([
                    getDepartments(),
                    getUsers(),
                ]);
                setDepartments(deptData);
                setUsers(userData);
            } catch (err) {
                console.error('加载部门/用户数据失败:', err);
                setError(typeof err === 'string' ? err : '加载数据失败');
            } finally {
                setLoading(false);
            }
        };
        loadData();
    }, []);

    // 根据选中的部门过滤用户
    const filteredUsers = useMemo(() => {
        if (!departmentId) {
            // ALL: 显示所有用户
            return users;
        }
        const deptIdNum = Number(departmentId);
        return users.filter((u) => u.department_id === deptIdNum);
    }, [users, departmentId]);

    // 用户 Select 数据
    const userSelectData = useMemo(() => {
        return filteredUsers.map((u) => ({
            value: String(u.id),
            label: `${u.real_name || u.username}${u.department_id ? ` (${getDepartmentName(u.department_id)})` : ''}`,
        }));
    }, [filteredUsers, departments]);

    // 获取部门名称
    function getDepartmentName(deptId: number): string {
        const dept = departments.find((d) => d.id === deptId);
        return dept ? dept.department_name : `部门#${deptId}`;
    }

    // 获取已选用户的显示文本
    const getSelectedLabel = (): string => {
        if (!userId) return userLabel;
        const user = users.find((u) => String(u.id) === userId);
        if (!user) return `${userLabel} (已选)`;
        const deptName = user.department_id
            ? ` (${getDepartmentName(user.department_id)})`
            : '';
        return `${user.real_name || user.username}${deptName}`;
    };

    // 弹窗内容
    const modalBody = loading ? (
        <Group justify="center" py="xl">
            <Loader />
            <Text c="dimmed">加载部门与用户数据...</Text>
        </Group>
    ) : error ? (
        <Alert icon={<IconAlertCircle size={16} />} title="加载失败" color="red">
            {error}
        </Alert>
    ) : (
        <Group align="flex-start" gap="xl" wrap="nowrap">
            {/* ===== 左侧：部门 Radio 列表 ===== */}
            <Stack gap="xs" style={{ minWidth: 180, maxWidth: 240 }}>
                <Text size="sm" fw={600} mb={4}>
                    {departmentLabel}
                </Text>
                <Radio.Group
                    value={departmentId ?? '__ALL__'}
                    onChange={(val) => {
                        onDepartmentChange(val === '__ALL__' ? null : val);
                        // 切换部门时清空用户选择
                        onUserChange(null);
                    }}
                    disabled={disabled}
                >
                    <Stack gap={4}>
                        <Radio
                            value="__ALL__"
                            label={
                                <Text size="sm" fw={500}>
                                    全部部门 (ALL)
                                </Text>
                            }
                        />
                        <Divider my={4} />
                        <ScrollArea h={280} type="auto">
                            <Stack gap={4}>
                                {departments.map((dept) => (
                                    <Radio
                                        key={dept.id}
                                        value={String(dept.id)}
                                        label={
                                            <Text size="sm">{dept.department_name}</Text>
                                        }
                                    />
                                ))}
                            </Stack>
                        </ScrollArea>
                    </Stack>
                </Radio.Group>
            </Stack>

            {/* ===== 右侧：用户 Select ===== */}
            <Stack gap="xs" style={{ flex: 1, minWidth: 200 }}>
                <Text size="sm" fw={600} mb={4}>
                    {userLabel}
                </Text>
                <Select
                    placeholder={
                        departmentId
                            ? '请选择用户'
                            : '全部部门 - 请选择用户'
                    }
                    data={userSelectData}
                    value={userId}
                    onChange={(val) => onUserChange(val ?? null)}
                    searchable
                    clearable
                    disabled={disabled}
                    nothingFoundMessage="无匹配用户"
                />
                <Text size="xs" c="dimmed">
                    {departmentId
                        ? `当前部门: ${getDepartmentName(Number(departmentId))}`
                        : '当前: 全部部门'}
                    {' | '}
                    共 {filteredUsers.length} 位用户
                </Text>
            </Stack>
        </Group>
    );

    return (
        <>
            {/* 触发按钮 */}
            <Button
                variant="default"
                fullWidth
                rightSection={<IconSelector size={16} />}
                onClick={() => setOpened(true)}
                disabled={disabled}
                styles={{
                    root: {
                        justifyContent: 'space-between',
                        fontWeight: 400,
                        color: userId ? undefined : '#868e96',
                    },
                }}
            >
                {getSelectedLabel()}
            </Button>

            {/* 选择弹窗 */}
            <Modal
                opened={opened}
                onClose={() => setOpened(false)}
                title={userLabel}
                size="lg"
            >
                {modalBody}
            </Modal>
        </>
    );
};

export default DepartmentUserSelect;
