'use client';

import React, { Suspense, useState } from 'react';
import { useSearchParams, useRouter } from 'next/navigation';
import Layout from '@/components/Layout';
import { Stack, Group, Title, Text, Button, Modal, Center, Loader } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { IconArrowLeft, IconHistory } from '@tabler/icons-react';
import ExecutionTimeline, { ExecutionDetailTimeline } from '@/components/workflow/ExecutionTimeline';

function WorkflowRunsInner() {
    const searchParams = useSearchParams();
    const router = useRouter();
    const id = searchParams.get('id') || '';

    const [selectedExecutionId, setSelectedExecutionId] = useState<string | null>(null);
    const [opened, { open, close }] = useDisclosure(false);

    const handleSelectExecution = (executionId: string) => {
        setSelectedExecutionId(executionId);
        open();
    };

    return (
        <Layout>
            <Stack gap="lg">
                <Group>
                    <Button variant="subtle" leftSection={<IconArrowLeft size={16} />} onClick={() => router.push(`/knowledge/workflow/editor?id=${id}`)}>
                        返回编辑器
                    </Button>
                    <IconHistory size={28} />
                    <div>
                        <Title order={2}>执行历史</Title>
                        <Text c="dimmed">查看工作流的执行记录和详情</Text>
                    </div>
                </Group>

                <ExecutionTimeline workflowId={id} onSelectExecution={handleSelectExecution} />
            </Stack>

            <Modal opened={opened} onClose={close} title="执行详情" size="xl">
                {selectedExecutionId && <ExecutionDetailTimeline executionId={selectedExecutionId} />}
            </Modal>
        </Layout>
    );
}

/**
 * 工作流执行历史。
 *
 * 静态导出（output: "export"，Tauri 桌面端）无法为运行时生成的工作流 ID 预生成
 * 动态路由，因此使用静态路由 + query 参数（/knowledge/workflow/runs?id=xxx）。
 */
export default function WorkflowRunsPage() {
    return (
        <Suspense
            fallback={
                <Center py="xl">
                    <Loader size="sm" />
                </Center>
            }
        >
            <WorkflowRunsInner />
        </Suspense>
    );
}