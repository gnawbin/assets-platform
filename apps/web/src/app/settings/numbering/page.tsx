'use client';

import React, { useState, useEffect } from 'react';
import {
    Title,
    Text,
    Card,
    Group,
    Stack,
    Button,
    Table,
    TextInput,
    Select,
    Switch,
    ActionIcon,
    Tooltip,
    Badge,
    Modal,
    NumberInput,
    LoadingOverlay,
    Alert,
    Notification,
} from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { IconRefresh, IconEdit, IconAlertCircle } from '@tabler/icons-react';
import Layout from '@/components/Layout';
import { useApi } from '@/hooks/useApi';
import { notifySuccess, notifyError } from '@/utils/notify';
import {
    getRules,
    saveRule,
    resetSequence,
    type NumberingRule,
    type NumberingRuleInput,
} from '@/services/numberingService';

const DATE_FORMAT_OPTIONS = [
    { value: 'yyyyMMdd', label: 'yyyyMMdd (20260715)' },
    { value: 'yyMMdd', label: 'yyMMdd (260715)' },
    { value: 'yyyyMM', label: 'yyyyMM (202607)' },
    { value: 'yyyy', label: 'yyyy (2026)' },
    { value: '', label: '无日期' },
];

const RESET_MODE_OPTIONS = [
    { value: 'yearly', label: '按年重置' },
    { value: 'monthly', label: '按月重置' },
    { value: 'never', label: '永不重置' },
];

const DATE_POSITION_OPTIONS = [
    { value: 'after_prefix', label: '前缀后' },
    { value: 'before_serial', label: '流水号前' },
];

const NumberingSettingsPage: React.FC = () => {
    const [editModalOpened, { open: openEditModal, close: closeEditModal }] = useDisclosure(false);
    const [editingRule, setEditingRule] = useState<NumberingRule | null>(null);
    const [editForm, setEditForm] = useState({
        biz_name: '',
        prefix: '',
        date_format: 'yyyyMMdd',
        date_position: 'after_prefix',
        serial_length: 4,
        separator: '-',
        reset_mode: 'yearly',
        is_active: true,
    });

    // 使用 useApi 管理数据获取
    const {
        data: rules,
        loading,
        error,
        execute: fetchRules,
    } = useApi(getRules);

    // 使用 useApi 管理保存和重置操作
    const { execute: doSaveRule, loading: saving } = useApi(saveRule);
    const { execute: doResetSequence } = useApi(resetSequence);

    // 初始加载
    useEffect(() => {
        fetchRules();
    }, []);

    const handleEdit = (rule: NumberingRule) => {
        setEditingRule(rule);
        setEditForm({
            biz_name: rule.biz_name,
            prefix: rule.prefix || '',
            date_format: rule.date_format || 'yyyyMMdd',
            date_position: rule.date_position || 'after_prefix',
            serial_length: rule.serial_length,
            separator: rule.separator || '-',
            reset_mode: rule.reset_mode || 'yearly',
            is_active: rule.is_active,
        });
        openEditModal();
    };

    const computeSample = (form: typeof editForm) => {
        const now = new Date();
        const dateMap: Record<string, string> = {
            yyyyMMdd: `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, '0')}${String(now.getDate()).padStart(2, '0')}`,
            yyMMdd: `${String(now.getFullYear()).slice(-2)}${String(now.getMonth() + 1).padStart(2, '0')}${String(now.getDate()).padStart(2, '0')}`,
            yyyyMM: `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, '0')}`,
            yyyy: `${now.getFullYear()}`,
        };
        const dateStr = dateMap[form.date_format] || '';
        const sep = form.separator || '-';
        const serialStr = String(1).padStart(form.serial_length, '0');
        const parts: string[] = [];
        if (form.prefix) parts.push(form.prefix);
        if (dateStr) parts.push(dateStr);
        parts.push(serialStr);
        return parts.join(sep);
    };

    const handleSave = async () => {
        if (!editingRule) return;

        const input: NumberingRuleInput = {
            biz_type: editingRule.biz_type,
            biz_name: editForm.biz_name,
            prefix: editForm.prefix || null,
            date_format: editForm.date_format || null,
            date_position: editForm.date_position || null,
            serial_length: editForm.serial_length,
            separator: editForm.separator || null,
            reset_mode: editForm.reset_mode || null,
            is_active: editForm.is_active,
        };

        try {
            await doSaveRule({ id: editingRule.id, input });
            notifySuccess('保存成功', `「${editForm.biz_name}」规则已更新`);
            closeEditModal();
            fetchRules();
        } catch (err) {
            notifyError('保存失败', typeof err === 'string' ? err : undefined);
        }
    };

    const handleResetSequence = async (bizType: string) => {
        try {
            await doResetSequence({ bizType, resetKey: '' });
            notifySuccess('操作成功', `「${bizType}」流水号已重置`);
        } catch (err) {
            notifyError('重置失败', typeof err === 'string' ? err : undefined);
        }
    };

    const bizTypeLabels: Record<string, string> = {
        asset: '资产',
        receive: '领用',
        return: '归还',
        transfer: '调拨',
        repair: '维修',
        scrap: '报废',
        purchase: '采购',
    };

    return (
        <Layout>
            <Stack gap="lg">
                <Group justify="space-between">
                    <div>
                        <Title order={2}>编号规则</Title>
                        <Text c="dimmed">配置各类单据编号的生成规则</Text>
                    </div>
                    <Button variant="light" leftSection={<IconRefresh size={16} />} onClick={fetchRules} loading={loading}>
                        刷新
                    </Button>
                </Group>

                {error && (
                    <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red" onClose={() => fetchRules()} withCloseButton>
                        {error}
                    </Alert>
                )}

                <Card withBorder padding="lg" radius="md" pos="relative">
                    <LoadingOverlay visible={loading && !rules} />

                    <Table striped highlightOnHover>
                        <Table.Thead>
                            <Table.Tr>
                                <Table.Th>业务类型</Table.Th>
                                <Table.Th>业务名称</Table.Th>
                                <Table.Th>前缀</Table.Th>
                                <Table.Th>日期格式</Table.Th>
                                <Table.Th>流水号位数</Table.Th>
                                <Table.Th>重置模式</Table.Th>
                                <Table.Th>示例</Table.Th>
                                <Table.Th>状态</Table.Th>
                                <Table.Th>操作</Table.Th>
                            </Table.Tr>
                        </Table.Thead>
                        <Table.Tbody>
                            {(!rules || rules.length === 0) ? (
                                <Table.Tr>
                                    <Table.Td colSpan={9}>
                                        <Text ta="center" c="dimmed" py="xl">
                                            暂无编号规则数据
                                        </Text>
                                    </Table.Td>
                                </Table.Tr>
                            ) : (
                                rules.map((rule) => (
                                    <Table.Tr key={rule.id}>
                                        <Table.Td>
                                            <Badge variant="light" color="blue">
                                                {bizTypeLabels[rule.biz_type] || rule.biz_type}
                                            </Badge>
                                        </Table.Td>
                                        <Table.Td>{rule.biz_name}</Table.Td>
                                        <Table.Td>
                                            <Text ff="monospace" fw={500}>
                                                {rule.prefix || '-'}
                                            </Text>
                                        </Table.Td>
                                        <Table.Td>{rule.date_format || '-'}</Table.Td>
                                        <Table.Td>{rule.serial_length}</Table.Td>
                                        <Table.Td>
                                            {rule.reset_mode === 'yearly'
                                                ? '按年'
                                                : rule.reset_mode === 'monthly'
                                                    ? '按月'
                                                    : '永不'}
                                        </Table.Td>
                                        <Table.Td>
                                            <Text ff="monospace" size="sm" c="dimmed">
                                                {rule.sample_output || '-'}
                                            </Text>
                                        </Table.Td>
                                        <Table.Td>
                                            <Switch
                                                checked={rule.is_active}
                                                readOnly
                                                size="xs"
                                                color={rule.is_active ? 'green' : 'gray'}
                                            />
                                        </Table.Td>
                                        <Table.Td>
                                            <Group gap="xs">
                                                <Tooltip label="编辑规则">
                                                    <ActionIcon variant="subtle" color="blue" onClick={() => handleEdit(rule)}>
                                                        <IconEdit size={16} />
                                                    </ActionIcon>
                                                </Tooltip>
                                                <Tooltip label="重置流水号">
                                                    <ActionIcon
                                                        variant="subtle"
                                                        color="orange"
                                                        onClick={() => handleResetSequence(rule.biz_type)}
                                                    >
                                                        <IconRefresh size={16} />
                                                    </ActionIcon>
                                                </Tooltip>
                                            </Group>
                                        </Table.Td>
                                    </Table.Tr>
                                ))
                            )}
                        </Table.Tbody>
                    </Table>
                </Card>
            </Stack>

            <Modal
                opened={editModalOpened}
                onClose={closeEditModal}
                title={`编辑编号规则 - ${editingRule?.biz_name || ''}`}
                size="lg"
            >
                <Stack gap="md">
                    <TextInput
                        label="业务名称"
                        value={editForm.biz_name}
                        onChange={(e) => setEditForm({ ...editForm, biz_name: e.target.value })}
                        required
                    />
                    <TextInput
                        label="前缀"
                        value={editForm.prefix}
                        onChange={(e) => setEditForm({ ...editForm, prefix: e.target.value })}
                        placeholder="如 ZC、LY"
                        maxLength={10}
                    />
                    <Group grow>
                        <Select
                            label="日期格式"
                            data={DATE_FORMAT_OPTIONS}
                            value={editForm.date_format}
                            onChange={(v) => setEditForm({ ...editForm, date_format: v || 'yyyyMMdd' })}
                        />
                        <Select
                            label="日期位置"
                            data={DATE_POSITION_OPTIONS}
                            value={editForm.date_position}
                            onChange={(v) => setEditForm({ ...editForm, date_position: v || 'after_prefix' })}
                        />
                    </Group>
                    <Group grow>
                        <NumberInput
                            label="流水号位数"
                            value={editForm.serial_length}
                            onChange={(v) => setEditForm({ ...editForm, serial_length: Number(v) || 4 })}
                            min={1}
                            max={10}
                        />
                        <TextInput
                            label="分隔符"
                            value={editForm.separator}
                            onChange={(e) => setEditForm({ ...editForm, separator: e.target.value })}
                            placeholder="-"
                            maxLength={3}
                        />
                    </Group>
                    <Select
                        label="重置模式"
                        data={RESET_MODE_OPTIONS}
                        value={editForm.reset_mode}
                        onChange={(v) => setEditForm({ ...editForm, reset_mode: v || 'yearly' })}
                    />
                    <Switch
                        label="启用"
                        checked={editForm.is_active}
                        onChange={(e) => setEditForm({ ...editForm, is_active: e.currentTarget.checked })}
                    />
                    <TextInput
                        label="预览示例"
                        value={computeSample(editForm)}
                        readOnly
                        styles={{ input: { fontFamily: 'monospace' } }}
                    />
                    <Group justify="flex-end" mt="md">
                        <Button variant="default" onClick={closeEditModal}>
                            取消
                        </Button>
                        <Button onClick={handleSave} loading={saving}>
                            保存
                        </Button>
                    </Group>
                </Stack>
            </Modal>
        </Layout>
    );
};

export default NumberingSettingsPage;