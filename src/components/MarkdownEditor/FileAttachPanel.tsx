'use client';
import React from 'react';
import { FileUploader } from '@/components/FileUploader';

/**
 * 上传状态
 */
export type AttachUploadStatus =
    | 'idle'
    | 'uploading'
    | 'paused'
    | 'completed'
    | 'error';

export interface FileAttachPanelProps {
    fileUrl?: string;
    fileName?: string;
    fileSize?: number;

    /** 上传完成回调（父组件可在此拿到 fileUrl 绑定到业务实体） */
    onUploadComplete?: (result: { fileUrl: string; fileName: string; fileSize: number }) => void;
    /** 上传错误回调 */
    onUploadError?: (err: string) => void;
}

/**
 * 文件附件面板
 *
 * 轻量适配层，内部使用 <FileUploader> 的 inline + singleFile 模式
 * 实现 S3/RustFS 分片上传、断点续传、批量上传等完整能力
 */
const FileAttachPanel: React.FC<FileAttachPanelProps> = ({
    fileUrl,
    fileName,
    fileSize,
    onUploadComplete,
    onUploadError,
}) => {
    return (
        <FileUploader
            inline
            singleFile
            fileUrl={fileUrl}
            fileName={fileName}
            fileSize={fileSize}
            onUploadComplete={onUploadComplete ? (result) => {
                onUploadComplete({
                    fileUrl: result.fileUrl,
                    fileName: result.originalName,
                    fileSize: result.fileSize,
                });
            } : undefined}
            onUploadError={onUploadError}
        />
    );
};

export default FileAttachPanel;