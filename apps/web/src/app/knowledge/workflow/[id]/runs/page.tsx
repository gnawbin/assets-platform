'use client';

import React, { useState } from 'react';
import { useParams, useRouter } from 'next/navigation';
import Layout from '@/components/Layout';
import { Stack, Group, Title, Text, Button, Modal } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { IconArrowLeft, IconHistory } from '@tabler/icons-react';
import ExecutionTimeline, { ExecutionDetailTimeline } from '@/components/workflow/ExecutionTimeline';

export default function WorkflowRunsPage() {
    const params = useParams();
    const router = useRouter();
    const id = params.id as string;

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
                    <Button variant="subtle" leftSection={<IconArrowLeft size={16} />} onClick={() => router.push(`/knowledge/workflow/${id}/edit`)}>
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