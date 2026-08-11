'use client';

import React from 'react';
import { useParams, useRouter } from 'next/navigation';
import Layout from '@/components/Layout';
import { Box } from '@mantine/core';
import WorkflowEditor from '@/components/workflow/WorkflowEditor';

export default function EditWorkflowPage() {
    const params = useParams();
    const router = useRouter();
    const id = params.id as string;

    return (
        <Layout>
            <Box style={{ height: 'calc(100vh - 140px)', display: 'flex', flexDirection: 'column' }}>
                <WorkflowEditor
                    workflowId={id}
                    onSaved={(newId) => {
                        if (newId !== id) {
                            router.replace(`/knowledge/workflow/${newId}/edit`);
                        }
                    }}
                />
            </Box>
        </Layout>
    );
}