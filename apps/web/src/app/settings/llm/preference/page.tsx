'use client';
import React, { useEffect, useState } from 'react';
import Layout from '@/components/Layout';
import {
    Title, Text, Card, Stack, Group, Button, Select, Slider, TextInput,
    Loader, Alert, Divider,
} from '@mantine/core';
import { IconBrain, IconAlertCircle } from '@tabler/icons-react';
import {
    getLlmProviders, getLlmModels, getUserLLmSetting, saveUserLLmSetting,
    type LlmProvider, type LlmModel, type UserLLmSetting,
} from '@/services/llmProviderService';
import { useAuthStore } from '@/store/authStore';
import { notifications } from '@mantine/notifications';

export default function LlmPreferencePage() {
    const { user } = useAuthStore();
    const [providers, setProviders] = useState<LlmProvider[]>([]);
    const [models, setModels] = useState<LlmModel[]>([]);
    const [setting, setSetting] = useState<UserLLmSetting | null>(null);
    const [loading, setLoading] = useState(true);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const [defaultProviderId, setDefaultProviderId] = useState<string | null>(null);
    const [defaultChatModelId, setDefaultChatModelId] = useState<string | null>(null);
    const [defaultEmbedModelId, setDefaultEmbedModelId] = useState<string | null>(null);
    const [customTemp, setCustomTemp] = useState<number>(0.7);
    const [customMaxToken, setCustomMaxToken] = useState<number>(2048);

    useEffect(() => {
        async function load() {
            try {
                setLoading(true);
                const [p, m] = await Promise.all([getLlmProviders(), getLlmModels()]);
                setProviders(p.filter(pv => pv.enable));
                setModels(m.filter(md => md.enable));

                if (user) {
                    const s = await getUserLLmSetting(user.id.toString());
                    if (s) {
                        setSetting(s);
                        setDefaultProviderId(s.default_provider_id);
                        setDefaultChatModelId(s.default_chat_model_id);
                        setDefaultEmbedModelId(s.default_embed_model_id);
                        setCustomTemp(s.custom_temp ?? 0.7);
                        setCustomMaxToken(s.custom_max_token ?? 2048);
                    }
                }
            } catch (err: any) {
                setError(err.message || '加载失败');
            } finally { setLoading(false); }
        }
        load();
    }, [user]);

    const chatModels = models.filter(m => m.model_type === 'chat');
    const embedModels = models.filter(m => m.model_type === 'embedding');
    const providerOptions = providers.map(p => ({ value: p.id, label: p.provider_name }));
    const chatModelOptions = chatModels.map(m => ({ value: m.id, label: `${m.model_name} (${m.model_code})` }));
    const embedModelOptions = embedModels.map(m => ({ value: m.id, label: `${m.model_name} (${m.model_code})` }));

    const handleSave = async () => {
        if (!user) return;
        setSaving(true);
        try {
            await saveUserLLmSetting({
                userId: user.id.toString(),
                defaultProviderId: defaultProviderId || undefined,
                defaultChatModelId: defaultChatModelId || undefined,
                defaultEmbedModelId: defaultEmbedModelId || undefined,
                customTemp,
                customMaxToken,
            });
            notifications.show({ title: '保存成功', color: 'green', message: '模型偏好已更新' });
        } catch (err: any) {
            notifications.show({ title: '保存失败', message: err.message, color: 'red' });
        } finally { setSaving(false); }
    };

    const selectedProvider = providers.find(p => p.id === defaultProviderId);
    const selectedChatModel = chatModels.find(m => m.id === defaultChatModelId);

    if (loading) return <Layout><Group justify="center" py="xl"><Loader /></Group></Layout>;

    return (
        <Layout>
            <Stack gap="lg">
                <Group><IconBrain size={28} /><Title order={2}>👤 模型偏好设置</Title></Group>
                {error && <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red">{error}</Alert>}

                <Card withBorder padding="lg" radius="md">
                    <Stack gap="md">
                        <Text fw={600}>默认厂商与模型</Text>
                        <Divider />

                        <Select label="默认厂商" placeholder="选择厂商"
                            data={providerOptions} value={defaultProviderId}
                            onChange={setDefaultProviderId} clearable />

                        <Select label="默认对话模型" placeholder="选择对话模型"
                            data={chatModelOptions} value={defaultChatModelId}
                            onChange={setDefaultChatModelId} clearable />
                        {selectedChatModel && (
                            <Text size="xs" c="dimmed">
                                上下文: {selectedChatModel.context_window?.toLocaleString() || '未知'} tokens ·
                                温度: {selectedChatModel.temperature_default ?? 0.7} ·
                                最大Token: {selectedChatModel.max_tokens_default ?? 2048}
                            </Text>
                        )}

                        <Select label="默认向量模型" placeholder="选择向量模型"
                            data={embedModelOptions} value={defaultEmbedModelId}
                            onChange={setDefaultEmbedModelId} clearable />

                        <Divider />
                        <Text fw={600}>高级参数</Text>

                        <Text size="sm">温度: {customTemp.toFixed(1)}</Text>
                        <Slider min={0} max={2} step={0.1} value={customTemp}
                            onChange={setCustomTemp}
                            label={(v) => v.toFixed(1)} marks={[
                                { value: 0, label: '0' }, { value: 1, label: '1' }, { value: 2, label: '2' },
                            ]} />

                        <TextInput label="最大输出 Token" type="number"
                            value={customMaxToken.toString()}
                            onChange={(e) => setCustomMaxToken(Number(e.target.value) || 2048)} />

                        <Group justify="flex-end">
                            <Button variant="default" onClick={() => {
                                setDefaultProviderId(null);
                                setDefaultChatModelId(null);
                                setDefaultEmbedModelId(null);
                                setCustomTemp(0.7);
                                setCustomMaxToken(2048);
                            }}>重置为默认</Button>
                            <Button onClick={handleSave} loading={saving}>保存</Button>
                        </Group>
                    </Stack>
                </Card>

                {setting && (
                    <Card withBorder padding="lg" radius="md">
                        <Text fw={600} mb="sm">📊 当前配置</Text>
                        <Text size="sm">默认厂商: {providers.find(p => p.id === setting.default_provider_id)?.provider_name || '未设置'}</Text>
                        <Text size="sm">对话模型: {chatModels.find(m => m.id === setting.default_chat_model_id)?.model_name || '未设置'}</Text>
                        <Text size="sm">向量模型: {embedModels.find(m => m.id === setting.default_embed_model_id)?.model_name || '未设置'}</Text>
                        <Text size="sm">温度: {setting.custom_temp ?? '默认'}</Text>
                        <Text size="sm">最大Token: {setting.custom_max_token ?? '默认'}</Text>
                    </Card>
                )}
            </Stack>
        </Layout>
    );
}