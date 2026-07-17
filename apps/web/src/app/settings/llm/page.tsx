'use client';
import React, { useEffect, useState, useCallback } from 'react';
import Layout from '@/components/Layout';
import {
    Title, Text, Card, Stack, Group, Button, Modal, TextInput, Switch,
    Slider, ActionIcon, Tooltip, Badge, Loader, Alert, Divider, SimpleGrid,
} from '@mantine/core';
import { IconPlus, IconEdit, IconTrash, IconRefresh, IconAlertCircle, IconBrain } from '@tabler/icons-react';
import {
    getLlmProviders, createLlmProvider, updateLlmProvider, deleteLlmProvider,
    getLlmModels, type LlmProvider, type LlmModel,
} from '@/services/llmProviderService';
import { notifications } from '@mantine/notifications';

export default function LlmProvidersPage() {
    const [providers, setProviders] = useState<LlmProvider[]>([]);
    const [models, setModels] = useState<LlmModel[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [editModal, setEditModal] = useState(false);
    const [editingId, setEditingId] = useState<string | null>(null);
    const [form, setForm] = useState({
        providerCode: '', providerName: '', baseUrl: '',
        apiKey: '', weight: 10, isLocal: false, enable: true,
    });

    const loadData = useCallback(async () => {
        try {
            setLoading(true);
            const [p, m] = await Promise.all([getLlmProviders(), getLlmModels()]);
            setProviders(p);
            setModels(m);
        } catch (err: any) {
            setError(err.message || '加载失败');
        } finally { setLoading(false); }
    }, []);

    useEffect(() => { loadData(); }, [loadData]);

    const handleEdit = (p?: LlmProvider) => {
        if (p) {
            setEditingId(p.id);
            setForm({
                providerCode: p.provider_code, providerName: p.provider_name,
                baseUrl: p.base_url || '', apiKey: '',
                weight: p.weight || 10, isLocal: p.is_local, enable: p.enable,
            });
        } else {
            setEditingId(null);
            setForm({ providerCode: '', providerName: '', baseUrl: '', apiKey: '', weight: 10, isLocal: false, enable: true });
        }
        setEditModal(true);
    };

    const handleSave = async () => {
        try {
            if (editingId) {
                await updateLlmProvider({ id: editingId, ...form, apiKey: form.apiKey || undefined });
            } else {
                await createLlmProvider(form);
            }
            setEditModal(false);
            await loadData();
            notifications.show({ title: '保存成功', color: 'green', message: '' });
        } catch (err: any) {
            notifications.show({ title: '保存失败', message: err.message, color: 'red' });
        }
    };

    const handleDelete = async (id: string) => {
        if (!confirm('确定删除此厂商？')) return;
        try {
            await deleteLlmProvider(id);
            await loadData();
        } catch { /* ignore */ }
    };

    const getModelsForProvider = (providerId: string) =>
        models.filter(m => m.provider_id === providerId);

    return (
        <Layout>
            <Stack gap="lg">
                <Group justify="space-between">
                    <Group><IconBrain size={28} /><Title order={2}>🧠 LLM 厂商管理</Title></Group>
                    <Group>
                        <Button variant="light" leftSection={<IconRefresh size={16} />} onClick={loadData}>刷新</Button>
                        <Button leftSection={<IconPlus size={16} />} onClick={() => handleEdit()}>新增厂商</Button>
                    </Group>
                </Group>
                {error && <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red">{error}</Alert>}
                {loading ? <Group justify="center" py="xl"><Loader /></Group> : (
                    <SimpleGrid cols={{ base: 1, md: 2, lg: 3 }} spacing="md">
                        {providers.map(p => (
                            <Card key={p.id} withBorder padding="lg" radius="md">
                                <Group justify="space-between" mb="sm">
                                    <Group>
                                        <Badge variant="light" color={p.enable ? 'green' : 'gray'} size="lg">
                                            {p.provider_code}
                                        </Badge>
                                        {p.is_local && <Badge variant="light" color="orange" size="sm">本地</Badge>}
                                    </Group>
                                    <Group gap={4}>
                                        <Tooltip label="编辑"><ActionIcon variant="subtle" color="blue"
                                            onClick={() => handleEdit(p)}><IconEdit size={14} /></ActionIcon></Tooltip>
                                        <Tooltip label="删除"><ActionIcon variant="subtle" color="red"
                                            onClick={() => handleDelete(p.id)}><IconTrash size={14} /></ActionIcon></Tooltip>
                                    </Group>
                                </Group>
                                <Text fw={500} size="md">{p.provider_name}</Text>
                                <Text size="xs" c="dimmed" mb="sm">{p.base_url || '默认地址'}</Text>
                                <Group gap="xs" mb="sm">
                                    <Text size="xs" c="dimmed">权重: {p.weight || 10}</Text>
                                    <Badge variant="light" color={p.enable ? 'green' : 'red'} size="xs">
                                        {p.enable ? '已启用' : '已禁用'}
                                    </Badge>
                                </Group>
                                <Divider mb="sm" />
                                <Text size="xs" fw={500} mb="xs">关联模型 ({getModelsForProvider(p.id).length})</Text>
                                {getModelsForProvider(p.id).map(m => (
                                    <Group key={m.id} gap="xs" mb={2}>
                                        <Badge variant="light" color={
                                            m.model_type === 'chat' ? 'blue' :
                                                m.model_type === 'embedding' ? 'teal' : 'violet'
                                        } size="xs">{m.model_type}</Badge>
                                        <Text size="xs">{m.model_name}</Text>
                                    </Group>
                                ))}
                            </Card>
                        ))}
                    </SimpleGrid>
                )}
            </Stack>

            <Modal opened={editModal} onClose={() => setEditModal(false)}
                title={editingId ? '编辑厂商' : '新增厂商'} size="lg">
                <Stack gap="md">
                    <TextInput label="厂商编码" required value={form.providerCode}
                        onChange={(e) => setForm({ ...form, providerCode: e.target.value })} />
                    <TextInput label="厂商名称" required value={form.providerName}
                        onChange={(e) => setForm({ ...form, providerName: e.target.value })} />
                    <TextInput label="API 地址" value={form.baseUrl}
                        onChange={(e) => setForm({ ...form, baseUrl: e.target.value })} />
                    <TextInput label="API Key" value={form.apiKey} type="password"
                        onChange={(e) => setForm({ ...form, apiKey: e.target.value })}
                        placeholder={editingId ? '留空不修改' : '必填'} />
                    <Text size="sm">权重: {form.weight}</Text>
                    <Slider min={1} max={100} value={form.weight}
                        onChange={(v) => setForm({ ...form, weight: v })} />
                    <Switch label="本地部署" checked={form.isLocal}
                        onChange={(e) => setForm({ ...form, isLocal: e.currentTarget.checked })} />
                    <Switch label="启用" checked={form.enable}
                        onChange={(e) => setForm({ ...form, enable: e.currentTarget.checked })} />
                    <Group justify="flex-end" mt="md">
                        <Button variant="default" onClick={() => setEditModal(false)}>取消</Button>
                        <Button onClick={handleSave}>保存</Button>
                    </Group>
                </Stack>
            </Modal>
        </Layout>
    );
}