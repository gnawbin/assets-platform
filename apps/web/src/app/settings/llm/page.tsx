'use client';
import React, { useEffect, useState, useCallback } from 'react';
import Layout from '@/components/Layout';
import {
    Title, Text, Card, Stack, Group, Button, Modal, TextInput, Switch,
    Slider, ActionIcon, Tooltip, Badge, Loader, Alert, Divider, SimpleGrid,
    Select, NumberInput,
} from '@mantine/core';
import { IconPlus, IconEdit, IconTrash, IconRefresh, IconAlertCircle, IconBrain, IconCloudDownload } from '@tabler/icons-react';
import {
    getLlmProviders, createLlmProvider, updateLlmProvider, deleteLlmProvider,
    getLlmModels, createLlmModel, updateLlmModel, deleteLlmModel, fetchLlmModels,
    type LlmProvider, type LlmModel,
} from '@/services/llmProviderService';
import { notifications } from '@mantine/notifications';

export default function LlmConfigPage() {
    const [providers, setProviders] = useState<LlmProvider[]>([]);
    const [models, setModels] = useState<LlmModel[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    // --- 厂商编辑弹窗 ---
    const [providerModal, setProviderModal] = useState(false);
    const [editingProviderId, setEditingProviderId] = useState<string | null>(null);
    const [providerForm, setProviderForm] = useState({
        providerCode: '', providerName: '', baseUrl: '',
        apiKey: '', weight: 10, isLocal: false, enable: true,
    });

    // --- 模型编辑弹窗 ---
    const [modelModal, setModelModal] = useState(false);
    const [editingModelId, setEditingModelId] = useState<string | null>(null);
    const [modelFormProviderId, setModelFormProviderId] = useState<string>('');
    const [modelForm, setModelForm] = useState({
        modelCode: '', modelName: '', modelType: 'chat' as string,
        contextWindow: 4096, temperatureDefault: 0.7, maxTokensDefault: 2048, enable: true,
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

    // ==================== 厂商操作 ====================

    const openProviderEdit = (p?: LlmProvider) => {
        if (p) {
            setEditingProviderId(p.id);
            setProviderForm({
                providerCode: p.provider_code, providerName: p.provider_name,
                baseUrl: p.base_url || '', apiKey: '',
                weight: p.weight || 10, isLocal: p.is_local, enable: p.enable,
            });
        } else {
            setEditingProviderId(null);
            setProviderForm({ providerCode: '', providerName: '', baseUrl: '', apiKey: '', weight: 10, isLocal: false, enable: true });
        }
        setProviderModal(true);
    };

    const handleSaveProvider = async () => {
        try {
            if (editingProviderId) {
                await updateLlmProvider({ id: editingProviderId, ...providerForm, apiKey: providerForm.apiKey || undefined });
            } else {
                await createLlmProvider(providerForm);
            }
            setProviderModal(false);
            await loadData();
            notifications.show({ title: '厂商保存成功', color: 'green', message: '' });
        } catch (err: any) {
            notifications.show({ title: '保存失败', message: err.message, color: 'red' });
        }
    };

    const handleDeleteProvider = async (id: string) => {
        if (!confirm('确定删除此厂商？（关联的模型也会被清空）')) return;
        try {
            await deleteLlmProvider(id);
            await loadData();
            notifications.show({ title: '已删除', color: 'orange', message: '' });
        } catch { /* ignore */ }
    };

    // ==================== 模型操作 ====================

    const getModelsForProvider = (providerId: string) =>
        models.filter(m => m.provider_id === providerId);

    const openModelCreate = (providerId: string) => {
        setEditingModelId(null);
        setModelFormProviderId(providerId);
        setModelForm({ modelCode: '', modelName: '', modelType: 'chat', contextWindow: 4096, temperatureDefault: 0.7, maxTokensDefault: 2048, enable: true });
        setModelModal(true);
    };

    const openModelEdit = (m: LlmModel) => {
        setEditingModelId(m.id);
        setModelFormProviderId(m.provider_id);
        setModelForm({
            modelCode: m.model_code, modelName: m.model_name, modelType: m.model_type,
            contextWindow: m.context_window ?? 4096, temperatureDefault: m.temperature_default ?? 0.7,
            maxTokensDefault: m.max_tokens_default ?? 2048, enable: m.enable,
        });
        setModelModal(true);
    };

    const handleSaveModel = async () => {
        try {
            if (editingModelId) {
                await updateLlmModel({ id: editingModelId, ...modelForm });
            } else {
                await createLlmModel({ providerId: modelFormProviderId, ...modelForm });
            }
            setModelModal(false);
            await loadData();
            notifications.show({ title: '模型保存成功', color: 'green', message: '' });
        } catch (err: any) {
            notifications.show({ title: '保存失败', message: err.message, color: 'red' });
        }
    };

    const handleDeleteModel = async (id: string) => {
        if (!confirm('确定删除此模型？')) return;
        try {
            await deleteLlmModel(id);
            await loadData();
        } catch { /* ignore */ }
    };

    const handleFetchModels = async (providerId: string) => {
        try {
            notifications.show({ title: '正在获取模型列表...', color: 'blue', message: '请稍候', autoClose: false });
            await fetchLlmModels(providerId);
            await loadData();
            notifications.show({ title: '模型获取完成', color: 'green', message: '' });
        } catch (err: any) {
            notifications.show({ title: '获取失败', message: err.message, color: 'red' });
        }
    };

    const modelTypeColor = (t: string) =>
        t === 'chat' ? 'blue' : t === 'embedding' ? 'teal' : 'violet';

    const modelTypeLabel = (t: string) =>
        t === 'chat' ? '对话' : t === 'embedding' ? '向量' : t;

    if (loading) return <Layout><Group justify="center" py="xl"><Loader /></Group></Layout>;

    return (
        <Layout>
            <Stack gap="lg">
                {/* ===== 页面标题 ===== */}
                <Group justify="space-between">
                    <Group><IconBrain size={28} /><Title order={2}>🧠 LLM 厂商与模型</Title></Group>
                    <Button variant="light" leftSection={<IconRefresh size={16} />} onClick={loadData}>刷新</Button>
                </Group>

                {error && <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red">{error}</Alert>}

                {/* ===== 厂商列表 ===== */}
                <Group justify="space-between">
                    <Text fw={600} size="lg">厂商管理</Text>
                    <Button leftSection={<IconPlus size={16} />} onClick={() => openProviderEdit()}>新增厂商</Button>
                </Group>

                {providers.length === 0 ? (
                    <Card withBorder padding="xl" radius="md">
                        <Text c="dimmed" ta="center">暂无厂商，点击"新增厂商"开始配置</Text>
                    </Card>
                ) : (
                    <SimpleGrid cols={{ base: 1, md: 2, lg: 3 }} spacing="md">
                        {providers.map(p => {
                            const providerModels = getModelsForProvider(p.id);
                            return (
                                <Card key={p.id} withBorder padding="lg" radius="md">
                                    {/* 厂商头部 */}
                                    <Group justify="space-between" mb="sm">
                                        <Group>
                                            <Badge variant="light" color={p.enable ? 'green' : 'gray'} size="lg">
                                                {p.provider_code}
                                            </Badge>
                                            {p.is_local && <Badge variant="light" color="orange" size="sm">本地</Badge>}
                                        </Group>
                                        <Group gap={4}>
                                            <Tooltip label="从 API 获取模型"><ActionIcon variant="subtle" color="cyan"
                                                onClick={() => handleFetchModels(p.id)}><IconCloudDownload size={14} /></ActionIcon></Tooltip>
                                            <Tooltip label="编辑厂商"><ActionIcon variant="subtle" color="blue"
                                                onClick={() => openProviderEdit(p)}><IconEdit size={14} /></ActionIcon></Tooltip>
                                            <Tooltip label="删除厂商"><ActionIcon variant="subtle" color="red"
                                                onClick={() => handleDeleteProvider(p.id)}><IconTrash size={14} /></ActionIcon></Tooltip>
                                        </Group>
                                    </Group>

                                    {/* 厂商详情 */}
                                    <Text fw={500} size="md">{p.provider_name}</Text>
                                    <Text size="xs" c="dimmed" mb="sm">{p.base_url || '默认地址'}</Text>
                                    <Group gap="xs" mb="sm">
                                        <Text size="xs" c="dimmed">权重: {p.weight || 10}</Text>
                                        <Badge variant="light" color={p.enable ? 'green' : 'red'} size="xs">
                                            {p.enable ? '已启用' : '已禁用'}
                                        </Badge>
                                    </Group>

                                    {/* 模型列表 */}
                                    <Divider mb="sm" />
                                    <Group justify="space-between" mb="xs">
                                        <Text size="xs" fw={500}>模型 ({providerModels.length})</Text>
                                        <Button size="compact-xs" variant="light"
                                            onClick={() => openModelCreate(p.id)}>添加模型</Button>
                                    </Group>

                                    {providerModels.length === 0 ? (
                                        <Text size="xs" c="dimmed">暂无模型，点击"获取模型"从 API 拉取，或手动添加</Text>
                                    ) : (
                                        providerModels.map(m => (
                                            <Group key={m.id} gap="xs" mb={2} justify="space-between">
                                                <Group gap="xs">
                                                    <Badge variant="light" color={modelTypeColor(m.model_type)} size="xs">
                                                        {modelTypeLabel(m.model_type)}
                                                    </Badge>
                                                    <Text size="xs">{m.model_name}</Text>
                                                    {m.context_window && (
                                                        <Text size="xs" c="dimmed">{(m.context_window / 1000).toFixed(0)}K</Text>
                                                    )}
                                                </Group>
                                                <Group gap={2}>
                                                    <ActionIcon variant="subtle" color="blue" size="xs"
                                                        onClick={() => openModelEdit(m)}><IconEdit size={12} /></ActionIcon>
                                                    <ActionIcon variant="subtle" color="red" size="xs"
                                                        onClick={() => handleDeleteModel(m.id)}><IconTrash size={12} /></ActionIcon>
                                                </Group>
                                            </Group>
                                        ))
                                    )}
                                </Card>
                            );
                        })}
                    </SimpleGrid>
                )}
            </Stack>

            {/* ===== 厂商编辑弹窗 ===== */}
            <Modal opened={providerModal} onClose={() => setProviderModal(false)}
                title={editingProviderId ? '编辑厂商' : '新增厂商'} size="lg">
                <Stack gap="md">
                    <TextInput label="厂商编码" required value={providerForm.providerCode}
                        onChange={(e) => setProviderForm({ ...providerForm, providerCode: e.target.value })} />
                    <TextInput label="厂商名称" required value={providerForm.providerName}
                        onChange={(e) => setProviderForm({ ...providerForm, providerName: e.target.value })} />
                    <TextInput label="API 地址" value={providerForm.baseUrl}
                        onChange={(e) => setProviderForm({ ...providerForm, baseUrl: e.target.value })} />
                    <TextInput label="API Key" value={providerForm.apiKey} type="password"
                        onChange={(e) => setProviderForm({ ...providerForm, apiKey: e.target.value })}
                        placeholder={editingProviderId ? '留空不修改' : '必填'} />
                    <Text size="sm">权重: {providerForm.weight}</Text>
                    <Slider min={1} max={100} value={providerForm.weight}
                        onChange={(v) => setProviderForm({ ...providerForm, weight: v })} />
                    <Switch label="本地部署" checked={providerForm.isLocal}
                        onChange={(e) => setProviderForm({ ...providerForm, isLocal: e.currentTarget.checked })} />
                    <Switch label="启用" checked={providerForm.enable}
                        onChange={(e) => setProviderForm({ ...providerForm, enable: e.currentTarget.checked })} />
                    <Group justify="flex-end" mt="md">
                        <Button variant="default" onClick={() => setProviderModal(false)}>取消</Button>
                        <Button onClick={handleSaveProvider}>保存</Button>
                    </Group>
                </Stack>
            </Modal>

            {/* ===== 模型编辑弹窗 ===== */}
            <Modal opened={modelModal} onClose={() => setModelModal(false)}
                title={editingModelId ? '编辑模型' : '添加模型'} size="md">
                <Stack gap="md">
                    <TextInput label="模型编码" required value={modelForm.modelCode}
                        onChange={(e) => setModelForm({ ...modelForm, modelCode: e.target.value })}
                        placeholder="如 gpt-4o / nomic-embed-text" />
                    <TextInput label="模型名称" required value={modelForm.modelName}
                        onChange={(e) => setModelForm({ ...modelForm, modelName: e.target.value })} />
                    <Select label="模型类型" required data={[
                        { value: 'chat', label: '对话 (chat)' },
                        { value: 'embedding', label: '向量 (embedding)' },
                    ]} value={modelForm.modelType}
                        onChange={(v) => setModelForm({ ...modelForm, modelType: v || 'chat' })} />
                    <NumberInput label="上下文窗口" value={modelForm.contextWindow}
                        onChange={(v) => setModelForm({ ...modelForm, contextWindow: Number(v) || 4096 })} min={1} max={1000000} />
                    <Text size="sm">默认温度: {modelForm.temperatureDefault.toFixed(1)}</Text>
                    <Slider min={0} max={2} step={0.1} value={modelForm.temperatureDefault}
                        onChange={(v) => setModelForm({ ...modelForm, temperatureDefault: v })}
                        label={(v) => v.toFixed(1)} />
                    <NumberInput label="最大 Token" value={modelForm.maxTokensDefault}
                        onChange={(v) => setModelForm({ ...modelForm, maxTokensDefault: Number(v) || 2048 })} min={1} max={128000} />
                    <Switch label="启用" checked={modelForm.enable}
                        onChange={(e) => setModelForm({ ...modelForm, enable: e.currentTarget.checked })} />
                    <Group justify="flex-end" mt="md">
                        <Button variant="default" onClick={() => setModelModal(false)}>取消</Button>
                        <Button onClick={handleSaveModel}>保存</Button>
                    </Group>
                </Stack>
            </Modal>
        </Layout>
    );
}