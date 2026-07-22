'use client';
import React, { useEffect, useState, useCallback, useRef } from 'react';
import Layout from '@/components/Layout';
import {
    Title, Text, Card, Stack, Group, Button, TextInput, ScrollArea, Box,
    Divider, ActionIcon, Tooltip, Loader, Alert, Avatar, Select, Badge,
} from '@mantine/core';
import {
    IconMessage, IconPlus, IconTrash, IconBrain,
    IconSend, IconAlertCircle, IconSettings,
} from '@tabler/icons-react';
import {
    createConversation, sendMessage, getConversations,
    getConversationMessages, deleteConversation,
    type ConversationResponse, type ConversationSummary, type AssetInfo,
} from '@/services/conversationService';
import {
    getLlmProviders, getLlmModels, getUserLLmSetting, saveUserLLmSetting,
    type LlmProvider, type LlmModel,
} from '@/services/llmProviderService';
import { useAuthStore } from '@/store/authStore';
import { notifications } from '@mantine/notifications';
import MessageBubble from '@/components/Chat/MessageBubble';

export default function ChatPage() {
    const { user } = useAuthStore();
    const [convList, setConvList] = useState<ConversationSummary[]>([]);
    const [currentConvId, setCurrentConvId] = useState<string | null>(null);
    const [messages, setMessages] = useState<Array<{
        role: 'user' | 'assistant'; content: string;
        citedAssets?: AssetInfo[]; referenceText?: string;
        metadata?: { model?: string; durationMs?: number; };
    }>>([]);
    const [input, setInput] = useState('');
    const [loading, setLoading] = useState(false);
    const [sending, setSending] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [isNewConversation, setIsNewConversation] = useState(false);
    const viewport = useRef<HTMLDivElement>(null);

    // 厂商/模型选择
    const [providers, setProviders] = useState<LlmProvider[]>([]);
    const [models, setModels] = useState<LlmModel[]>([]);
    const [selectedProviderId, setSelectedProviderId] = useState<string | null>(null);
    const [selectedModelId, setSelectedModelId] = useState<string | null>(null);

    const loadConvList = useCallback(async () => {
        if (!user) return;
        try {
            setLoading(true);
            const data = await getConversations({ userId: user.id.toString() });
            setConvList(data.items);
        } catch { setConvList([]); }
        finally { setLoading(false); }
    }, [user]);

    useEffect(() => { loadConvList(); }, [loadConvList]);

    // 加载厂商列表
    useEffect(() => {
        getLlmProviders().then(setProviders).catch(() => setProviders([]));
    }, []);

    // 加载用户偏好
    useEffect(() => {
        if (!user) return;
        getUserLLmSetting(user.id.toString()).then(setting => {
            if (setting) {
                setSelectedProviderId(setting.default_provider_id);
                setSelectedModelId(setting.default_chat_model_id);
            }
        }).catch(() => { });
    }, [user]);

    // 当选中的厂商变化时加载对应模型
    useEffect(() => {
        if (!selectedProviderId) {
            setModels([]);
            setSelectedModelId(null);
            return;
        }
        getLlmModels(selectedProviderId).then(allModels => {
            const chatModels = allModels.filter(m => m.model_type === 'chat');
            setModels(chatModels);
            if (selectedModelId && !chatModels.find(m => m.id === selectedModelId)) {
                setSelectedModelId(null);
            }
        }).catch(() => setModels([]));
    }, [selectedProviderId]);

    // 保存用户偏好
    const savePreference = useCallback(async (providerId: string | null, modelId: string | null) => {
        if (!user) return;
        try {
            await saveUserLLmSetting({
                userId: user.id.toString(),
                defaultProviderId: providerId ?? undefined,
                defaultChatModelId: modelId ?? undefined,
            });
        } catch { /* 静默 */ }
    }, [user]);

    const handleProviderChange = (value: string | null) => {
        setSelectedProviderId(value);
        setSelectedModelId(null);
        savePreference(value, null);
    };

    const handleModelChange = (value: string | null) => {
        setSelectedModelId(value);
        savePreference(selectedProviderId, value);
    };

    // 获取用户选择的 model_name（显示名 → 模型代码）
    const getSelectedModelName = useCallback((): string | undefined => {
        if (!selectedModelId) return undefined;
        const m = models.find(x => x.id === selectedModelId);
        return m?.model_code;
    }, [selectedModelId, models]);

    const loadMessages = useCallback(async (convId: string) => {
        try {
            const msgs = await getConversationMessages({ convId });
            setMessages(msgs.map(m => ({
                role: m.role as 'user' | 'assistant',
                content: m.content,
                citedAssets: m.citedAssets,
                referenceText: m.referenceText,
                metadata: m.metadata,
            })));
        } catch { setMessages([]); }
    }, []);

    const handleSelectConv = async (id: string) => {
        setCurrentConvId(id);
        await loadMessages(id);
    };

    const handleNewConv = async () => {
        if (!user) return;
        setCurrentConvId(null);
        setMessages([]);
        setInput('');
        setIsNewConversation(true);
    };

    const handleSend = async () => {
        if (!input.trim() || !user) return;
        const question = input.trim();
        setInput('');
        setSending(true);

        setMessages(prev => [...prev, { role: 'user', content: question }]);

        try {
            let resp: ConversationResponse;
            const extraParams = {
                providerId: selectedProviderId ?? undefined,
                modelName: getSelectedModelName(),
            };
            if (currentConvId) {
                resp = await sendMessage({
                    convId: currentConvId,
                    userId: user.id.toString(),
                    question,
                    ...extraParams,
                } as any);
            } else {
                resp = await createConversation({
                    userId: user.id.toString(),
                    question,
                    ...extraParams,
                } as any);
                setCurrentConvId(resp.convId);
                setIsNewConversation(false);
                await loadConvList();
            }

            setMessages(prev => [...prev, {
                role: 'assistant',
                content: resp.answer,
                citedAssets: resp.citedAssets,
                metadata: { durationMs: 0 },
            }]);

            setTimeout(() => viewport.current?.scrollTo({ top: viewport.current.scrollHeight, behavior: 'smooth' }), 100);
        } catch (err: any) {
            notifications.show({ title: '发送失败', message: err.message || '请稍后重试', color: 'red' });
            setMessages(prev => prev.slice(0, -1));
        } finally { setSending(false); }
    };

    const handleDeleteConv = async (id: string) => {
        try {
            await deleteConversation(id);
            if (currentConvId === id) { setCurrentConvId(null); setMessages([]); }
            await loadConvList();
        } catch { /* ignore */ }
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleSend(); }
    };

    return (
        <Layout>
            <Stack gap="lg">
                <Group>
                    <IconBrain size={28} /><Title order={2}>智能问答</Title>
                    <div style={{ flex: 1 }} />
                    <Select
                        placeholder="选择厂商"
                        data={providers.filter(p => p.enable).map(p => ({ value: p.id, label: p.provider_name }))}
                        value={selectedProviderId}
                        onChange={handleProviderChange}
                        clearable
                        size="sm"
                        style={{ width: 160 }}
                        leftSection={<IconSettings size={14} />}
                        nothingFoundMessage="无可用厂商"
                    />
                    <Select
                        placeholder="选择模型"
                        data={models.map(m => ({ value: m.id, label: m.model_name }))}
                        value={selectedModelId}
                        onChange={handleModelChange}
                        clearable
                        size="sm"
                        style={{ width: 180 }}
                        disabled={!selectedProviderId || models.length === 0}
                        nothingFoundMessage="无可用模型"
                    />
                </Group>
                {error && <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red">{error}</Alert>}
                <Group gap="lg" align="flex-start" grow wrap="nowrap">
                    <Card withBorder padding="lg" radius="md" style={{ maxWidth: 280, minWidth: 240 }}>
                        <Group justify="space-between" mb="md">
                            <Text fw={600} size="sm">📋 会话列表</Text>
                            <Tooltip label="新对话">
                                <ActionIcon variant="light" color="blue" size="sm" onClick={handleNewConv}>
                                    <IconPlus size={14} />
                                </ActionIcon>
                            </Tooltip>
                        </Group>
                        <Divider mb="md" />
                        {loading ? <Group justify="center" py="xl"><Loader /></Group> : convList.length === 0 ? (
                            <Text ta="center" c="dimmed" py="xl" size="sm">暂无会话</Text>
                        ) : (
                            <ScrollArea h={500}>
                                {convList.map((conv) => (
                                    <Box key={conv.id}
                                        style={{
                                            display: 'flex', alignItems: 'center', gap: 4,
                                            padding: '8px 8px', cursor: 'pointer', borderRadius: 6,
                                            backgroundColor: currentConvId === conv.id
                                                ? 'var(--mantine-color-blue-light)' : 'transparent',
                                            color: currentConvId === conv.id
                                                ? 'var(--mantine-color-blue-filled)' : 'var(--mantine-color-gray-7)',
                                        }}
                                        onClick={() => handleSelectConv(conv.id)}
                                    >
                                        <IconMessage size={14} style={{ flexShrink: 0 }} />
                                        <Text size="sm" style={{
                                            flex: 1, overflow: 'hidden', textOverflow: 'ellipsis',
                                            whiteSpace: 'nowrap', marginLeft: 6,
                                        }}>{conv.title || '新对话'}</Text>
                                        <Group gap={2} style={{ opacity: 0.5 }} onClick={(e) => e.stopPropagation()}>
                                            <Tooltip label="删除">
                                                <ActionIcon variant="subtle" color="red" size="sm"
                                                    onClick={() => handleDeleteConv(conv.id)}>
                                                    <IconTrash size={12} />
                                                </ActionIcon>
                                            </Tooltip>
                                        </Group>
                                    </Box>
                                ))}
                            </ScrollArea>
                        )}
                    </Card>

                    <Card withBorder padding="lg" radius="md" style={{ flex: 1 }}>
                        {currentConvId || messages.length > 0 || isNewConversation ? (
                            <Stack gap="md" style={{ height: 600 }}>
                                {selectedProviderId && selectedModelId && (
                                    <Group gap="xs" mb={-8}>
                                        {(() => {
                                            const p = providers.find(x => x.id === selectedProviderId);
                                            const m = models.find(x => x.id === selectedModelId);
                                            return (
                                                <>
                                                    {p && <Badge size="sm" variant="light" color="blue">{p.provider_name}</Badge>}
                                                    {m && <Badge size="sm" variant="light" color="teal">{m.model_name}</Badge>}
                                                </>
                                            );
                                        })()}
                                    </Group>
                                )}
                                <ScrollArea h={500} viewportRef={viewport}>
                                    {messages.length === 0 ? (
                                        <Text ta="center" c="dimmed" py="xl">开始您的第一个问题</Text>
                                    ) : messages.map((msg, i) => (
                                        <MessageBubble key={i}
                                            role={msg.role} content={msg.content}
                                            citedAssets={msg.citedAssets} referenceText={msg.referenceText}
                                            metadata={msg.metadata}
                                        />
                                    ))}
                                    {sending && (
                                        <Group justify="flex-start">
                                            <Avatar color="violet" radius="xl">AI</Avatar>
                                            <Text size="sm" c="dimmed" fs="italic">思考中...</Text>
                                        </Group>
                                    )}
                                </ScrollArea>
                                <Group gap="sm">
                                    <TextInput
                                        placeholder="输入问题..."
                                        value={input}
                                        onChange={(e) => setInput(e.target.value)}
                                        onKeyDown={handleKeyDown}
                                        disabled={sending}
                                        style={{ flex: 1 }}
                                        size="md"
                                    />
                                    <Button onClick={handleSend} loading={sending}
                                        leftSection={<IconSend size={16} />}>发送</Button>
                                </Group>
                            </Stack>
                        ) : (
                            <Stack align="center" justify="center" style={{ height: 600 }}>
                                <IconBrain size={64} stroke={1} color="var(--mantine-color-gray-4)" />
                                <Text size="lg" c="dimmed">选择一个会话或创建新对话</Text>
                                <Text size="sm" c="dimmed">AI 将基于知识库内容回答您的问题</Text>
                                <Button leftSection={<IconPlus size={16} />} onClick={handleNewConv} mt="md">
                                    新对话
                                </Button>
                            </Stack>
                        )}
                    </Card>
                </Group>
            </Stack>
        </Layout>
    );
}