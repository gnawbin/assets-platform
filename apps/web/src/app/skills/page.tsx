'use client';

import React, { useEffect, useState, useCallback } from 'react';
import Layout from '@/components/Layout';
import {
    Title,
    Text,
    Card,
    Stack,
    Group,
    Button,
    Modal,
    TextInput,
    Textarea,
    Loader,
    Alert,
    SimpleGrid,
    Badge,
    ActionIcon,
    Tooltip,
    Paper,
} from '@mantine/core';
import {
    IconAlertCircle,
    IconPlayerPlay,
    IconTrash,
    IconSearch,
    IconRefresh,
    IconBrain,
    IconPlayerPlayFilled,
} from '@tabler/icons-react';
import {
    listSkills,
    executeSkill,
    unregisterSkill,
    type SkillMeta,
    type SkillResult,
} from '@/services/skillService';
import { useAuthStore } from '@/store/authStore';

// ======================== Skill 卡片组件 ========================

interface SkillCardProps {
    skill: SkillMeta;
    onExecute: (skill: SkillMeta) => void;
    onDelete: (skillId: string) => void;
}

const SkillCard: React.FC<SkillCardProps> = ({ skill, onExecute, onDelete }) => {
    const isBuiltin = skill.skill_type === 'Builtin';

    return (
        <Card withBorder padding="lg" radius="md">
            <Group justify="space-between" align="flex-start" wrap="nowrap">
                <Group gap="sm" align="flex-start" wrap="nowrap">
                    <Text size="xl">{skill.icon}</Text>
                    <div>
                        <Text fw={600} size="sm">
                            {skill.name}
                        </Text>
                        <Text size="xs" c="dimmed" mt={2}>
                            {skill.description}
                        </Text>
                    </div>
                </Group>
                <Badge
                    variant="light"
                    color={isBuiltin ? 'blue' : 'green'}
                    size="sm"
                    style={{ flexShrink: 0 }}
                >
                    {isBuiltin ? '内置' : '自定义'}
                </Badge>
            </Group>

            <Group gap="md" mt="sm">
                <Text size="xs" c="dimmed">
                    v{skill.version}
                </Text>
                <Text size="xs" c="dimmed">
                    作者: {skill.author}
                </Text>
            </Group>

            <Group gap="xs" mt="md">
                <Button
                    size="compact-sm"
                    leftSection={<IconPlayerPlayFilled size={12} />}
                    onClick={() => onExecute(skill)}
                >
                    执行
                </Button>
                {!isBuiltin && (
                    <Tooltip label="移除">
                        <ActionIcon
                            variant="light"
                            color="red"
                            size="sm"
                            onClick={() => onDelete(skill.id)}
                        >
                            <IconTrash size={14} />
                        </ActionIcon>
                    </Tooltip>
                )}
            </Group>
        </Card>
    );
};

// ======================== 主页面 ========================

export default function SkillsPage() {
    const [skills, setSkills] = useState<SkillMeta[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [searchQuery, setSearchQuery] = useState('');
    const { user } = useAuthStore();

    // 执行对话框
    const [showExecuteDialog, setShowExecuteDialog] = useState(false);
    const [executingSkill, setExecutingSkill] = useState<SkillMeta | null>(null);
    const [inputText, setInputText] = useState('');
    const [executing, setExecuting] = useState(false);
    const [result, setResult] = useState<SkillResult | null>(null);

    // 加载 Skill 列表
    const loadSkills = useCallback(async () => {
        try {
            setLoading(true);
            setError(null);
            const data = await listSkills();
            setSkills(data);
        } catch (err: unknown) {
            setError(err instanceof Error ? err.message : '加载 Skill 列表失败');
        } finally {
            setLoading(false);
        }
    }, []);

    useEffect(() => {
        loadSkills();
    }, [loadSkills]);

    // 过滤
    const filteredSkills = skills.filter(
        (s) =>
            s.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
            s.description.toLowerCase().includes(searchQuery.toLowerCase())
    );

    // 按类型分组
    const builtinSkills = filteredSkills.filter((s) => s.skill_type === 'Builtin');
    const customSkills = filteredSkills.filter((s) => s.skill_type === 'Custom');

    // 执行 Skill
    const handleExecute = (skill: SkillMeta) => {
        setExecutingSkill(skill);
        setInputText('');
        setResult(null);
        setShowExecuteDialog(true);
    };

    const handleConfirmExecute = async () => {
        if (!executingSkill || !user) return;
        setExecuting(true);
        setResult(null);
        try {
            const res = await executeSkill({
                skill_id: executingSkill.id,
                input_text: inputText,
                user_id: Number(user.id),
                tenant_id: user.tenant_id ?? '1',
            });
            setResult(res);
        } catch (err: unknown) {
            setResult({
                output: err instanceof Error ? err.message : '执行失败',
                output_type: 'error',
                position: 'after_selection',
            });
        } finally {
            setExecuting(false);
        }
    };

    // 移除 Skill
    const handleDelete = async (skillId: string) => {
        if (!confirm('确定移除这个 Skill？')) return;
        try {
            await unregisterSkill(skillId);
            await loadSkills();
        } catch (err: unknown) {
            alert(err instanceof Error ? err.message : '移除失败');
        }
    };

    // ======================== 渲染 ========================

    return (
        <Layout>
            <Stack gap="lg">
                {/* 页面标题 */}
                <Group justify="space-between">
                    <Group>
                        <IconBrain size={28} />
                        <div>
                            <Title order={2}>Skill 管理</Title>
                            <Text c="dimmed">
                                管理和执行 AI 工作流 Skill（共 {skills.length} 个）
                            </Text>
                        </div>
                    </Group>
                    <Group>
                        <Button
                            variant="light"
                            leftSection={<IconRefresh size={16} />}
                            onClick={loadSkills}
                            loading={loading}
                        >
                            刷新
                        </Button>
                    </Group>
                </Group>

                {error && (
                    <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red">
                        {error}
                    </Alert>
                )}

                {/* 搜索栏 */}
                <TextInput
                    placeholder="搜索 Skill 名称或描述..."
                    leftSection={<IconSearch size={16} />}
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    style={{ maxWidth: 400 }}
                />

                {/* Skill 列表 */}
                {loading ? (
                    <Group justify="center" py="xl">
                        <Loader />
                    </Group>
                ) : filteredSkills.length === 0 ? (
                    <Text ta="center" c="dimmed" py="xl">
                        {searchQuery ? '未找到匹配的 Skill' : '暂无 Skill'}
                    </Text>
                ) : (
                    <Stack gap="xl">
                        {/* 内置 Skill */}
                        {builtinSkills.length > 0 && (
                            <div>
                                <Group gap="xs" mb="md">
                                    <Badge size="sm" circle color="blue" />
                                    <Text fw={600} size="sm" c="dimmed">
                                        内置 Skill ({builtinSkills.length})
                                    </Text>
                                </Group>
                                <SimpleGrid cols={{ base: 1, md: 2, lg: 3 }} spacing="md">
                                    {builtinSkills.map((skill) => (
                                        <SkillCard
                                            key={skill.id}
                                            skill={skill}
                                            onExecute={handleExecute}
                                            onDelete={handleDelete}
                                        />
                                    ))}
                                </SimpleGrid>
                            </div>
                        )}

                        {/* 自定义 Skill */}
                        {customSkills.length > 0 && (
                            <div>
                                <Group gap="xs" mb="md">
                                    <Badge size="sm" circle color="green" />
                                    <Text fw={600} size="sm" c="dimmed">
                                        自定义 Skill ({customSkills.length})
                                    </Text>
                                </Group>
                                <SimpleGrid cols={{ base: 1, md: 2, lg: 3 }} spacing="md">
                                    {customSkills.map((skill) => (
                                        <SkillCard
                                            key={skill.id}
                                            skill={skill}
                                            onExecute={handleExecute}
                                            onDelete={handleDelete}
                                        />
                                    ))}
                                </SimpleGrid>
                            </div>
                        )}
                    </Stack>
                )}
            </Stack>

            {/* ======================== 执行对话框 ======================== */}
            <Modal
                opened={showExecuteDialog}
                onClose={() => setShowExecuteDialog(false)}
                title={
                    <Group gap="sm">
                        <Text size="xl">{executingSkill?.icon}</Text>
                        <Text fw={600}>执行: {executingSkill?.name}</Text>
                    </Group>
                }
                size="lg"
            >
                {executingSkill && (
                    <Stack gap="md">
                        <Text size="sm" c="dimmed">
                            {executingSkill.description}
                        </Text>

                        <Textarea
                            label="输入文本"
                            placeholder="请输入要处理的文本..."
                            minRows={5}
                            value={inputText}
                            onChange={(e) => setInputText(e.target.value)}
                        />

                        {/* 执行结果 */}
                        {result && (
                            <Paper
                                p="sm"
                                withBorder
                                radius="sm"
                                bg={result.output_type === 'error' ? 'red.0' : 'blue.0'}
                            >
                                <Text
                                    size="sm"
                                    c={result.output_type === 'error' ? 'red' : 'dark'}
                                    style={{ whiteSpace: 'pre-wrap' }}
                                >
                                    {result.output}
                                </Text>
                            </Paper>
                        )}

                        <Group justify="flex-end" mt="md">
                            <Button
                                variant="default"
                                onClick={() => setShowExecuteDialog(false)}
                            >
                                关闭
                            </Button>
                            <Button
                                leftSection={
                                    executing ? (
                                        <Loader size="xs" color="white" />
                                    ) : (
                                        <IconPlayerPlay size={14} />
                                    )
                                }
                                onClick={handleConfirmExecute}
                                disabled={executing || !inputText.trim()}
                                loading={executing}
                            >
                                {executing ? '执行中...' : '执行'}
                            </Button>
                        </Group>
                    </Stack>
                )}
            </Modal>
        </Layout>
    );
}
