'use client';

import React from 'react';
import Layout from '@/components/Layout';
import { Box } from '@mantine/core';
import { useRouter } from 'next/navigation';
import WorkflowEditor from '@/components/workflow/WorkflowEditor';

export default function NewWorkflowPage() {
    const router = useRouter();

    return (
        <Layout>
            <Box style={{ height: 'calc(100vh - 140px)', display: 'flex', flexDirection: 'column' }}>
                <WorkflowEditor onSaved={(id) => router.push(`/knowledge/workflow/${id}/edit`)} />
            </Box>
        </Layout>
    );
}