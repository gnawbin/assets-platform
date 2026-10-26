'use client';

import React, { useEffect, useState } from 'react';
import { Stack, Text, TextInput, Textarea } from '@mantine/core';

export interface ConfigFieldProps {
    label: string;
    children: React.ReactNode;
    hint?: string;
}

/** 属性面板里的“标签 + 控件” */
export function ConfigField({ label, children, hint }: ConfigFieldProps) {
    return (
        <Stack gap={4}>
            <Text size="xs" fw={500} c="dimmed">
                {label}
            </Text>
            {children}
            {hint && (
                <Text size="xs" c="dimmed">
                    {hint}
                </Text>
            )}
        </Stack>
    );
}

interface TextFieldProps {
    label: string;
    value: string | undefined;
    onCommit: (value: string) => void;
    placeholder?: string;
    hint?: string;
    textarea?: boolean;
    minRows?: number;
    disabled?: boolean;
}

/**
 * 失焦提交的文本输入，避免每敲一个字符都写入 bpmn-js 命令栈。
 */
export function TextField({
    label,
    value,
    onCommit,
    placeholder,
    hint,
    textarea,
    minRows = 3,
    disabled,
}: TextFieldProps) {
    const [draft, setDraft] = useState(value ?? '');

    useEffect(() => {
        setDraft(value ?? '');
    }, [value]);

    const commit = () => {
        if (draft !== (value ?? '')) onCommit(draft);
    };

    return (
        <ConfigField label={label} hint={hint}>
            {textarea ? (
                <Textarea
                    size="xs"
                    autosize
                    minRows={minRows}
                    value={draft}
                    placeholder={placeholder}
                    disabled={disabled}
                    onChange={(event) => setDraft(event.currentTarget.value)}
                    onBlur={commit}
                />
            ) : (
                <TextInput
                    size="xs"
                    value={draft}
                    placeholder={placeholder}
                    disabled={disabled}
                    onChange={(event) => setDraft(event.currentTarget.value)}
                    onBlur={commit}
                    onKeyDown={(event) => {
                        if (event.key === 'Enter') {
                            commit();
                            event.currentTarget.blur();
                        }
                    }}
                />
            )}
        </ConfigField>
    );
}
