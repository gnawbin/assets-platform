/** OKF 知识类型 */
export type OkfType = 'raw_source' | 'concept' | 'fact' | 'rule' | 'param' | 'process' | 'case';

/** 编辑器模式 */
export type EditorMode = 'wysiwyg' | 'markdown' | 'raw';

/** Markdown 编辑器 Props */
export interface MarkdownEditorProps {
    // 内容
    content?: string;
    onChange?: (content: string) => void;

    // 标题（必填）
    title: string;
    onTitleChange: (title: string) => void;

    // OKF 属性
    okfType: OkfType;
    onOkfTypeChange: (type: OkfType) => void;
    summary?: string;
    onSummaryChange?: (summary: string) => void;
    source?: string;
    onSourceChange?: (source: string) => void;
    status: 'draft' | 'valid' | 'outdated';
    onStatusChange?: (status: 'draft' | 'valid' | 'outdated') => void;

    // 文件上传
    fileUrl?: string;
    fileName?: string;
    fileSize?: number;
    onFileUpload?: (file: File) => Promise<string>;

    // 编辑器模式
    editorMode?: EditorMode;
    onEditorModeChange?: (mode: EditorMode) => void;

    // 标签
    tags?: string[];
    onTagsChange?: (tags: string[]) => void;

    // 操作
    onSave?: () => void;
    saving?: boolean;
}

/** OKF 类型选项 */
export const OKF_TYPE_OPTIONS: { value: OkfType; label: string }[] = [
    { value: 'raw_source', label: '原始素材' },
    { value: 'concept', label: '概念' },
    { value: 'fact', label: '事实' },
    { value: 'rule', label: '规则' },
    { value: 'param', label: '参数' },
    { value: 'process', label: '流程' },
    { value: 'case', label: '案例' },
];

/** 状态选项 */
export const STATUS_OPTIONS: { value: 'draft' | 'valid' | 'outdated'; label: string }[] = [
    { value: 'draft', label: '草稿' },
    { value: 'valid', label: '有效' },
    { value: 'outdated', label: '过期' },
];

/** 编辑器模式选项 */
export const EDITOR_MODE_OPTIONS: { value: EditorMode; label: string }[] = [
    { value: 'wysiwyg', label: '可视化' },
    { value: 'markdown', label: 'Markdown' },
    { value: 'raw', label: '纯文本' },
];