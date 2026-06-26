'use client';
import React from 'react';
import {
    MDXEditor,
    headingsPlugin,
    listsPlugin,
    quotePlugin,
    thematicBreakPlugin,
    codeBlockPlugin,
    codeMirrorPlugin,
    linkPlugin,
    linkDialogPlugin,
    toolbarPlugin,
    UndoRedo,
    BoldItalicUnderlineToggles,
    BlockTypeSelect,
    CreateLink,
    ListsToggle,
    InsertCodeBlock,
    InsertThematicBreak,
} from '@mdxeditor/editor';
import '@mdxeditor/editor/style.css';

interface MDXEditorWrapperProps {
    content: string;
    onChange?: (content: string) => void;
}

const MDXEditorWrapper: React.FC<MDXEditorWrapperProps> = ({ content, onChange }) => {
    return (
        <MDXEditor
            markdown={content}
            onChange={onChange}
            plugins={[
                toolbarPlugin({
                    toolbarContents: () => (
                        <>
                            <UndoRedo />
                            <BoldItalicUnderlineToggles />
                            <BlockTypeSelect />
                            <ListsToggle />
                            <CreateLink />
                            <InsertCodeBlock />
                            <InsertThematicBreak />
                        </>
                    ),
                }),
                headingsPlugin(),
                listsPlugin(),
                quotePlugin(),
                thematicBreakPlugin(),
                codeBlockPlugin({ defaultCodeBlockLanguage: 'txt' }),
                codeMirrorPlugin({
                    codeBlockLanguages: {
                        js: 'JavaScript',
                        ts: 'TypeScript',
                        python: 'Python',
                        rust: 'Rust',
                        sql: 'SQL',
                        json: 'JSON',
                        xml: 'XML',
                        txt: 'Plain Text',
                    },
                }),
                linkPlugin(),
                linkDialogPlugin(),
            ]}
            contentEditableClassName="prose prose-sm max-w-none focus:outline-none min-h-[300px] p-3"
        />
    );
};

export default MDXEditorWrapper;