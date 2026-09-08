'use client';

import React, { Suspense } from 'react';
import { useSearchParams, useRouter } from 'next/navigation';
import Layout from '@/components/Layout';
import { Box, Center, Loader, Text } from '@mantine/core';
import WorkflowEditor from '@/components/workflow/WorkflowEditor';

function WorkflowEditorInner() {
    const searchParams = useSearchParams();
    const router = useRouter();
    const id = searchParams.get('id') || '';

    return (
        <Layout>
            <Box style={{ height: 'calc(100vh - 140px)', display: 'flex', flexDirection: 'column' }}>
                <WorkflowEditor
                    workflowId={id}
                    onSaved={(newId) => {
                        if (newId !== id) {
                            router.replace(`/knowledge/workflow/editor?id=${newId}`);
                        }
                    }}
                />
            </Box>
        </Layout>
    );
}

/**
 * 工作流编辑器。
 *
 * 静态导出（output: "export"，Tauri 桌面端）无法为运行时生成的工作流 ID 预生成
 * 动态路由，因此编辑器使用静态路由 + query 参数（/knowledge/workflow/editor?id=xxx）。
 */
export default function WorkflowEditorPage() {
    return (
        <Suspense
            fallback={
                <Center py="xl">
                    <Loader size="sm" />
                    <Text size="sm" c="dimmed" ml="sm">加载中...</Text>
                </Center>
            }
        >
            <WorkflowEditorInner />
        </Suspense>
    );
}