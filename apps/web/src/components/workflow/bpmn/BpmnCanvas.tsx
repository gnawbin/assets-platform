'use client';

import React, { useEffect, useRef, useState } from 'react';
import { Box, Center, Loader, Text } from '@mantine/core';
import 'bpmn-js/dist/assets/diagram-js.css';
import 'bpmn-js/dist/assets/bpmn-js.css';
import 'bpmn-js/dist/assets/bpmn-font/css/bpmn.css';
import './bpmn.css';
import type {
    AssetsDragPayload,
    BpmnCanvasApi,
    SelectedBpmnElement,
} from './types';

interface BpmnCanvasProps {
    xml: string;
    onReady: (api: BpmnCanvasApi | null) => void;
    onSelectionChange: (element: SelectedBpmnElement | null) => void;
    onDirtyChange: (dirty: boolean) => void;
    onError: (message: string) => void;
}

/**
 * bpmn-js 画布容器。
 *
 * bpmn-js 访问 window/document，必须关闭 SSR（由 WorkflowEditor 动态引入）。
 */
export default function BpmnCanvas({
    xml,
    onReady,
    onSelectionChange,
    onDirtyChange,
    onError,
}: BpmnCanvasProps) {
    const containerRef = useRef<HTMLDivElement>(null);
    const apiRef = useRef<BpmnCanvasApi | null>(null);
    const [ready, setReady] = useState(false);
    const callbacksRef = useRef({ onReady, onSelectionChange, onDirtyChange, onError });

    useEffect(() => {
        callbacksRef.current = { onReady, onSelectionChange, onDirtyChange, onError };
    }, [onReady, onSelectionChange, onDirtyChange, onError]);

    useEffect(() => {
        const container = containerRef.current;
        if (!container) return;

        setReady(false);
        let cancelled = false;
        let modeler: any = null;
        let selectionHandler: ((event: any) => void) | null = null;
        let changeHandler: (() => void) | null = null;
        let importing = true;

        (async () => {
            try {
                const [{ createModeler }, { createModelerApi, toSelectedElement }] =
                    await Promise.all([
                        import('./modeler/createModeler'),
                        import('./modeler/modelerApi'),
                    ]);
                if (cancelled) return;

                modeler = createModeler(container);
                const api = createModelerApi(modeler);
                apiRef.current = api;

                const eventBus = modeler.get('eventBus');
                changeHandler = () => {
                    if (!importing) callbacksRef.current.onDirtyChange(true);
                };
                selectionHandler = (event: any) => {
                    const element = event?.newSelection?.[0];
                    callbacksRef.current.onSelectionChange(
                        element ? toSelectedElement(element) : null
                    );
                };
                eventBus.on('commandStack.changed', changeHandler);
                eventBus.on('selection.changed', selectionHandler);

                await modeler.importXML(xml);
                importing = false;
                if (cancelled) return;

                try {
                    modeler.get('canvas').zoom('fit-viewport', 'auto');
                } catch {
                    // 容器尚未布局时忽略
                }

                callbacksRef.current.onDirtyChange(false);
                callbacksRef.current.onReady(api);
                setReady(true);
            } catch (error) {
                importing = false;
                if (cancelled) return;
                callbacksRef.current.onError(
                    error instanceof Error ? error.message : String(error)
                );
            }
        })();

        return () => {
            cancelled = true;
            apiRef.current = null;
            callbacksRef.current.onReady(null);
            if (modeler) {
                const eventBus = modeler.get('eventBus');
                if (changeHandler) eventBus.off('commandStack.changed', changeHandler);
                if (selectionHandler) eventBus.off('selection.changed', selectionHandler);
                try {
                    modeler.destroy();
                } catch {
                    // 忽略销毁异常
                }
            }
        };
    }, [xml]);

    const handleDragOver = (event: React.DragEvent<HTMLDivElement>) => {
        if (event.dataTransfer.types.includes('application/assets-node')) {
            event.preventDefault();
            event.dataTransfer.dropEffect = 'copy';
        }
    };

    const handleDrop = (event: React.DragEvent<HTMLDivElement>) => {
        const raw = event.dataTransfer.getData('application/assets-node');
        const api = apiRef.current;
        if (!raw || !api || !containerRef.current) return;
        event.preventDefault();

        try {
            const payload = JSON.parse(raw) as AssetsDragPayload;
            const modeler = api.getModeler() as any;
            const canvas = modeler.get('canvas');
            const viewbox = canvas.viewbox();
            const rect = containerRef.current.getBoundingClientRect();
            const scale = viewbox.scale || 1;
            api.insertPresetAt(
                payload.presetId,
                {
                    x: Math.round(viewbox.x + (event.clientX - rect.left) / scale - 50),
                    y: Math.round(viewbox.y + (event.clientY - rect.top) / scale - 40),
                },
                { engine: payload.engine, name: payload.name }
            );
        } catch (error) {
            callbacksRef.current.onError(
                error instanceof Error ? error.message : '拖放创建节点失败'
            );
        }
    };

    return (
        <Box
            ref={containerRef}
            className="bpmn-canvas"
            onDragOver={handleDragOver}
            onDrop={handleDrop}
            style={{ position: 'relative', flex: 1, minHeight: 0, height: '100%' }}
        >
            {!ready && (
                <Center style={{ position: 'absolute', inset: 0, zIndex: 2 }}>
                    <Loader size="sm" />
                    <Text size="sm" c="dimmed" ml="sm">
                        正在初始化 BPMN 画布...
                    </Text>
                </Center>
            )}
        </Box>
    );
}

