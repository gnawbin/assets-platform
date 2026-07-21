'use client';
import React, { useEffect, useState, useCallback, useRef } from 'react';
import Layout from '@/components/Layout';
import {
    Title, Text, Card, Stack, Group, Button, TextInput, ScrollArea, Box,
    Divider, ActionIcon, Tooltip, Loader, Alert, Avatar,
} from '@mantine/core';
import {
    IconMessage, IconPlus, IconTrash, IconEdit, IconBrain,
    IconSend, IconAlertCircle,
} from '@tabler/icons-react';
import {
    createConversation, sendMessage, getConversations,
    getConversationMessages, deleteConversation, updateConversationTitle,
    type ConversationResponse, type ConversationSummary, type AssetInfo,
} from '@/services/conversationService';
import { useAuthStore } from '@/store/authStore';
import { notifications } from '@mantine/notifications';
import MessageBubble from '@/components/Chat/MessageBubble';
// SSE 流式对话 Hook（当前未接入，后续可替换 handleSend）
// import { useChatStream } from '@/hooks/useChatStream';

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

        // 添加用户消息到本地
        setMessages(prev => [...prev, { role: 'user', content: question }]);

        try {
            let resp: ConversationResponse;
            if (currentConvId) {
                resp = await sendMessage({ convId: currentConvId, userId: user.id.toString(), question });
            } else {
                resp = await createConversation({ userId: user.id.toString(), question });
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

            // 滚动到底部
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
                <Group><IconBrain size={28} /><Title order={2}>智能问答</Title></Group>
                {error && <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red">{error}</Alert>}
                <Group gap="lg" align="flex-start" grow wrap="nowrap">
                    {/* 左侧会话列表 */}
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

                    {/* 右侧对话区域 */}
                    <Card withBorder padding="lg" radius="md" style={{ flex: 1 }}>
                        {currentConvId || messages.length > 0 || isNewConversation ? (
                            <Stack gap="md" style={{ height: 600 }}>
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