/**
 * 大文件分片上传组件
 *
 * 基于 Mantine UI 组件库，支持：
 * - 拖拽上传（@mantine/dropzone）
 * - 点击选择文件
 * - 进度条显示
 * - 暂停/继续/取消操作
 * - 上传速度显示
 * - 文件类型/大小校验
 * - 断点续传
 * - 批量上传
 * - 内联模式（inline）：不弹 Modal，直接以内联面板渲染
 * - 单文件模式（singleFile）：限制只上传一个文件
 */

'use client';

import React, { useState, useCallback, useRef, useEffect } from 'react';
import {
  Modal,
  Button,
  Text,
  Progress,
  Group,
  Stack,
  Paper,
  ActionIcon,
  Tooltip,
  Box,
  rem,
  FileButton,
} from '@mantine/core';
import { Dropzone, type FileRejection } from '@mantine/dropzone';
import { useDisclosure } from '@mantine/hooks';
import { notifications } from '@mantine/notifications';
import {
  IconUpload,
  IconX,
  IconPlayerPause,
  IconPlayerPlay,
  IconTrash,
  IconRefresh,
  IconFile,
  IconFileTypePdf,
  IconFileTypeDoc,
  IconFileTypeXls,
  IconFileTypeZip,
  IconPhoto,
  IconVideo,
  IconMusic,
  IconPaperclip,
  IconExternalLink,
} from '@tabler/icons-react';
import { useChunkedUpload } from '@/hooks/useChunkedUpload';
import type { UploadStatus } from '@/hooks/useChunkedUpload';

// ======================== 类型定义 ========================

export interface FileUploaderProps {
  /** 接受的文件类型，如 ".pdf,.docx,.jpg" */
  accept?: string;
  /** 最大文件大小（字节），默认 10GB */
  maxSize?: number;
  /** 是否多文件，默认 false */
  multiple?: boolean;
  /** 并发上传数，默认 3 */
  concurrency?: number;

  /** ========== 内联模式（适用于 MarkdownEditor 附件面板） ========== */
  /** 是否为内联模式（无 Modal，直接以内联面板渲染），默认 false */
  inline?: boolean;
  /** 单文件模式（inline 模式下）：只允许一个文件，选新文件替换旧文件，默认 true（inline 模式下） */
  singleFile?: boolean;
  /** 已有文件的 URL（inline + singleFile 下显示已有文件） */
  fileUrl?: string;
  /** 已有文件的名称 */
  fileName?: string;
  /** 已有文件的大小 */
  fileSize?: number;

  /** ========== 回调 ========== */
  /** 上传完成回调 */
  onUploadComplete?: (result: {
    fileUrl: string;
    originalName: string;
    fileSize: number;
  }) => void;
  /** 上传错误回调 */
  onUploadError?: (error: string) => void;
  /** 业务上下文类型（commit 时使用），如 'knowledge' */
  contextType?: string;
  /** 业务实体 ID（commit 时使用） */
  contextId?: string;
  /** 已有 fileGroupId（替换文件时传入，自动 version+1） */
  fileGroupId?: string;
  /** 变更原因 */
  changeReason?: string;
}

interface UploadTask {
  id: string;
  file: File;
  progress: number;
  status: UploadStatus;
  speed: number;
  error: string | null;
  fileUrl?: string;
}

// ======================== 工具函数 ========================

function formatSize(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec === 0) return '0 B/s';
  const k = 1024;
  const sizes = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
  const i = Math.floor(Math.log(bytesPerSec) / Math.log(k));
  return parseFloat((bytesPerSec / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

function getFileExt(filename: string): string {
  return filename.split('.').pop()?.toLowerCase() || '';
}

function getFileIcon(filename: string): React.ReactNode {
  const ext = getFileExt(filename);
  const iconMap: Record<string, React.ReactNode> = {
    pdf: <IconFileTypePdf size={24} color="red" />,
    doc: <IconFileTypeDoc size={24} color="blue" />,
    docx: <IconFileTypeDoc size={24} color="blue" />,
    xls: <IconFileTypeXls size={24} color="green" />,
    xlsx: <IconFileTypeXls size={24} color="green" />,
    jpg: <IconPhoto size={24} color="purple" />,
    jpeg: <IconPhoto size={24} color="purple" />,
    png: <IconPhoto size={24} color="purple" />,
    gif: <IconPhoto size={24} color="purple" />,
    webp: <IconPhoto size={24} color="purple" />,
    zip: <IconFileTypeZip size={24} color="yellow" />,
    rar: <IconFileTypeZip size={24} color="yellow" />,
    '7z': <IconFileTypeZip size={24} color="yellow" />,
    tar: <IconFileTypeZip size={24} color="yellow" />,
    gz: <IconFileTypeZip size={24} color="yellow" />,
    mp4: <IconVideo size={24} color="orange" />,
    avi: <IconVideo size={24} color="orange" />,
    mov: <IconVideo size={24} color="orange" />,
    mp3: <IconMusic size={24} color="teal" />,
    wav: <IconMusic size={24} color="teal" />,
  };
  return iconMap[ext] || <IconFile size={24} color="gray" />;
}

function getSmallFileIcon(filename: string): React.ReactNode {
  const ext = getFileExt(filename);
  const iconMap: Record<string, React.ReactNode> = {
    pdf: <IconFileTypePdf size={20} color="red" />,
    doc: <IconFileTypeDoc size={20} color="blue" />,
    docx: <IconFileTypeDoc size={20} color="blue" />,
    xls: <IconFileTypeXls size={20} color="green" />,
    xlsx: <IconFileTypeXls size={20} color="green" />,
    jpg: <IconPhoto size={20} color="purple" />,
    jpeg: <IconPhoto size={20} color="purple" />,
    png: <IconPhoto size={20} color="purple" />,
    gif: <IconPhoto size={20} color="purple" />,
    webp: <IconPhoto size={20} color="purple" />,
    zip: <IconFileTypeZip size={20} color="yellow" />,
    rar: <IconFileTypeZip size={20} color="yellow" />,
    '7z': <IconFileTypeZip size={20} color="yellow" />,
  };
  return iconMap[ext] || <IconFile size={20} color="gray" />;
}

// ======================== 上传任务项组件 ========================

interface UploadTaskItemProps {
  task: UploadTask;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
}

function UploadTaskItem({ task, onPause, onResume, onCancel, onRetry }: UploadTaskItemProps) {
  const progressColor =
    task.status === 'paused' ? 'yellow' : task.status === 'error' ? 'red' : 'blue';

  return (
    <Paper withBorder p="sm" radius="md">
      <Group gap="sm" wrap="nowrap" align="flex-start">
        {/* 文件图标 */}
        {getFileIcon(task.file.name)}

        {/* 文件信息 */}
        <Box style={{ flex: 1, minWidth: 0 }}>
          <Group gap="xs" justify="space-between" wrap="nowrap">
            <Text size="sm" fw={500} truncate style={{ flex: 1 }}>
              {task.file.name}
            </Text>
            <Text size="xs" c="dimmed" style={{ whiteSpace: 'nowrap' }}>
              {formatSize(task.file.size)}
            </Text>
          </Group>

          {/* 进度条 */}
          {(task.status === 'uploading' || task.status === 'paused') && (
            <Box mt={4}>
              <Group gap="xs" mb={2}>
                <Progress
                  value={task.progress}
                  color={progressColor}
                  size="sm"
                  style={{ flex: 1 }}
                  animated={task.status === 'uploading'}
                />
                <Text size="xs" c="dimmed" style={{ minWidth: 35, textAlign: 'right' }}>
                  {task.progress}%
                </Text>
              </Group>
              {task.status === 'uploading' && (
                <Text size="xs" c="dimmed">
                  {formatSpeed(task.speed)}
                </Text>
              )}
            </Box>
          )}

          {/* 状态信息 */}
          {task.status === 'completed' && (
            <Text size="xs" c="green" mt={2}>
              上传完成
            </Text>
          )}
          {task.status === 'error' && task.error && (
            <Text size="xs" c="red" mt={2} lineClamp={2}>
              {task.error}
            </Text>
          )}
          {task.status === 'idle' && (
            <Text size="xs" c="dimmed" mt={2}>
              等待上传
            </Text>
          )}
        </Box>

        {/* 操作按钮 */}
        <Group gap={4} wrap="nowrap">
          {task.status === 'uploading' && (
            <>
              <Tooltip label="暂停">
                <ActionIcon
                  variant="subtle"
                  color="yellow"
                  onClick={() => onPause(task.id)}
                  size="sm"
                >
                  <IconPlayerPause size={16} />
                </ActionIcon>
              </Tooltip>
              <Tooltip label="取消">
                <ActionIcon
                  variant="subtle"
                  color="red"
                  onClick={() => onCancel(task.id)}
                  size="sm"
                >
                  <IconTrash size={16} />
                </ActionIcon>
              </Tooltip>
            </>
          )}
          {task.status === 'paused' && (
            <>
              <Tooltip label="继续">
                <ActionIcon
                  variant="subtle"
                  color="blue"
                  onClick={() => onResume(task.id)}
                  size="sm"
                >
                  <IconPlayerPlay size={16} />
                </ActionIcon>
              </Tooltip>
              <Tooltip label="取消">
                <ActionIcon
                  variant="subtle"
                  color="red"
                  onClick={() => onCancel(task.id)}
                  size="sm"
                >
                  <IconTrash size={16} />
                </ActionIcon>
              </Tooltip>
            </>
          )}
          {task.status === 'completed' && (
            <Text size="xs" c="green" fw={500}>
              完成
            </Text>
          )}
          {task.status === 'error' && (
            <>
              <Tooltip label="重试">
                <ActionIcon
                  variant="subtle"
                  color="orange"
                  onClick={() => onRetry(task.id)}
                  size="sm"
                >
                  <IconRefresh size={16} />
                </ActionIcon>
              </Tooltip>
              <Tooltip label="移除">
                <ActionIcon
                  variant="subtle"
                  color="red"
                  onClick={() => onCancel(task.id)}
                  size="sm"
                >
                  <IconTrash size={16} />
                </ActionIcon>
              </Tooltip>
            </>
          )}
        </Group>
      </Group>
    </Paper>
  );
}

// ======================== 单个文件上传控制器 ========================

interface UploadControllerProps {
  task: UploadTask;
  concurrency: number;
  onProgress: (id: string, pct: number) => void;
  onComplete: (id: string, fileUrl: string) => void;
  onError: (id: string, err: string) => void;
  onStatusChange: (id: string, status: UploadStatus) => void;
  onSpeedChange: (id: string, speed: number) => void;
  onControllerReady: (id: string, controller: UploadControllerHandle) => void;
}

export interface UploadControllerHandle {
  pause: () => void;
  resume: () => void;
  cancel: () => void;
  retry: () => void;
}

function UploadController({
  task,
  concurrency,
  onProgress,
  onComplete,
  onError,
  onStatusChange,
  onSpeedChange,
  onControllerReady,
}: UploadControllerProps) {
  const upload = useChunkedUpload({
    concurrency,
    autoResume: true,
    storageKey: `chunked_upload_${task.id}`,
    onProgress: (pct) => onProgress(task.id, pct),
    onComplete: (result) => onComplete(task.id, result.fileUrl),
    onError: (err) => onError(task.id, err),
  });

  // 同步状态到父组件
  useEffect(() => {
    onStatusChange(task.id, upload.status);
  }, [upload.status, task.id, onStatusChange]);

  useEffect(() => {
    onSpeedChange(task.id, upload.speed);
  }, [upload.speed, task.id, onSpeedChange]);

  // 暴露控制器给父组件
  useEffect(() => {
    onControllerReady(task.id, {
      pause: upload.pause,
      resume: upload.resume,
      cancel: upload.cancel,
      retry: upload.retry,
    });
  }, [task.id, upload.pause, upload.resume, upload.cancel, upload.retry, onControllerReady]);

  // 自动开始上传
  useEffect(() => {
    if (task.status === 'idle') {
      upload.start(task.file);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return null;
}

// ======================== 内联单文件模式（替代 FileAttachPanel 功能） ========================

interface InlineFileAttachProps {
  fileUrl?: string;
  fileName?: string;
  fileSize?: number;
  maxSize: number;
  accept: string;
  concurrency: number;
  onUploadComplete?: (result: {
    fileUrl: string;
    originalName: string;
    fileSize: number;
  }) => void;
  onUploadError?: (error: string) => void;
}

function InlineFileAttach({
  fileUrl: externalFileUrl,
  fileName: externalFileName,
  fileSize: externalFileSize,
  maxSize,
  accept,
  concurrency,
  onUploadComplete,
  onUploadError,
}: InlineFileAttachProps) {
  const [displayFileName, setDisplayFileName] = useState<string | undefined>(externalFileName);
  const [displayFileUrl, setDisplayFileUrl] = useState<string | undefined>(externalFileUrl);
  const [displayFileSize, setDisplayFileSize] = useState<number | undefined>(externalFileSize);
  const [uploadState, setUploadState] = useState<UploadStatus>('idle');
  const [progress, setProgress] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [speed, setSpeed] = useState(0);
  const [uploadStageText, setUploadStageText] = useState<string>('');
  const fileRef = useRef<File | null>(null);

  // 已有的外部文件信息同步
  useEffect(() => {
    if (externalFileName && uploadState === 'idle') {
      setDisplayFileName(externalFileName);
      setDisplayFileUrl(externalFileUrl);
      setDisplayFileSize(externalFileSize);
    }
  }, [externalFileName, externalFileUrl, externalFileSize, uploadState]);

  const upload = useChunkedUpload({
    concurrency,
    autoResume: true,
    storageKey: 'inline_file_attach_upload',
    onProgress: (pct) => {
      setProgress(pct);
    },
    onComplete: (result) => {
      setUploadState('completed');
      setProgress(100);
      setDisplayFileName(fileRef.current?.name || result.fileUrl.split('/').pop() || '');
      setDisplayFileUrl(result.fileUrl);
      setDisplayFileSize(fileRef.current?.size || 0);
      onUploadComplete?.({
        fileUrl: result.fileUrl,
        originalName: fileRef.current?.name || '',
        fileSize: fileRef.current?.size || 0,
      });
    },
    onError: (err) => {
      setUploadState('error');
      setError(err);
      onUploadError?.(err);
    },
  });

  // 同步 useChunkedUpload 的状态
  useEffect(() => {
    if (upload.status !== 'idle') {
      setUploadState(upload.status);
    }
    if (upload.status === 'uploading') {
      setProgress(upload.progress);
      // 根据进度推断当前阶段
      if (upload.progress === 0) {
        setUploadStageText('初始化上传...');
      } else if (upload.progress < 100) {
        setUploadStageText(`上传分片中 (${upload.progress}%)...`);
      } else if (upload.progress === 100) {
        setUploadStageText('完成合并中...');
      }
    }
    if (upload.status === 'paused') {
      setUploadStageText('已暂停');
    }
    if (upload.status === 'completed') {
      setUploadStageText('上传完成');
    }
    if (upload.status === 'error' && upload.error) {
      setUploadStageText('上传失败');
      setError(upload.error);
    }
    if (upload.status === 'idle') {
      setUploadStageText('');
    }
  }, [upload.status, upload.progress, upload.error]);

  const handleFileSelect = (file: File | null) => {
    if (!file) return;

    // 文件大小校验
    if (file.size > maxSize) {
      const msg = `文件 ${file.name} 超过大小限制 ${formatSize(maxSize)}`;
      notifications.show({ title: '文件过大', message: msg, color: 'red' });
      onUploadError?.(msg);
      return;
    }

    // 重置状态
    setProgress(0);
    setError(null);
    setUploadState('uploading');
    fileRef.current = file;
    setDisplayFileName(file.name);
    setDisplayFileSize(file.size);
    setDisplayFileUrl(undefined);

    upload.start(file);
  };

  // 进度条颜色
  const progressColor =
    uploadState === 'error' ? 'red' : uploadState === 'paused' ? 'yellow' : 'blue';

  const formatSpeedLocal = (bytesPerSec: number): string => {
    if (bytesPerSec === 0) return '';
    const k = 1024;
    const sizes = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
    const i = Math.floor(Math.log(bytesPerSec) / Math.log(k));
    return parseFloat((bytesPerSec / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
  };

  const formatFileSizeLocal = (bytes: number): string => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  return (
    <Paper p="sm" withBorder>
      <Stack gap="xs">
        <Group justify="space-between">
          <Group gap="xs" style={{ flex: 1, minWidth: 0 }}>
            {displayFileName && uploadState !== 'uploading' && uploadState !== 'paused' ? (
              <>
                {getSmallFileIcon(displayFileName)}
                <Text size="sm" truncate style={{ maxWidth: 200 }}>
                  {displayFileName}
                </Text>
                {displayFileSize && displayFileSize > 0 && (
                  <Text size="xs" c="dimmed">
                    ({formatFileSizeLocal(displayFileSize)})
                  </Text>
                )}
                {displayFileUrl && (
                  <ActionIcon
                    variant="subtle"
                    component="a"
                    href={displayFileUrl}
                    target="_blank"
                    size="sm"
                  >
                    <IconExternalLink size={14} />
                  </ActionIcon>
                )}
              </>
            ) : uploadState === 'completed' ? (
              <Group gap="xs">
                <IconFile size={20} color="gray" />
                <Text size="sm" c="green" fw={500}>
                  上传完成
                </Text>
              </Group>
            ) : uploadState === 'idle' || uploadState === 'error' ? (
              <>
                <IconPaperclip size={16} />
                <Text size="sm" c="dimmed">
                  {uploadState === 'error' ? '上传失败' : '暂无附件'}
                </Text>
              </>
            ) : (
              // uploading / paused
              <>
                {getSmallFileIcon(upload.fileName || '文件')}
                <Text size="sm" truncate style={{ maxWidth: 200 }}>
                  {upload.fileName || '上传中...'}
                </Text>
              </>
            )}
          </Group>

          {/* 操作按钮 */}
          <Group gap="xs" wrap="nowrap">
            {uploadState === 'uploading' && (
              <>
                <Tooltip label="暂停">
                  <ActionIcon
                    variant="subtle"
                    color="yellow"
                    onClick={upload.pause}
                    size="sm"
                  >
                    <IconPlayerPause size={14} />
                  </ActionIcon>
                </Tooltip>
                <Tooltip label="取消">
                  <ActionIcon
                    variant="subtle"
                    color="red"
                    onClick={() => {
                      upload.cancel();
                      setUploadState('idle');
                      setProgress(0);
                      setError(null);
                    }}
                    size="sm"
                  >
                    <IconX size={14} />
                  </ActionIcon>
                </Tooltip>
              </>
            )}
            {uploadState === 'paused' && (
              <>
                <Tooltip label="继续">
                  <ActionIcon
                    variant="subtle"
                    color="blue"
                    onClick={upload.resume}
                    size="sm"
                  >
                    <IconPlayerPlay size={14} />
                  </ActionIcon>
                </Tooltip>
                <Tooltip label="取消">
                  <ActionIcon
                    variant="subtle"
                    color="red"
                    onClick={() => {
                      upload.cancel();
                      setUploadState('idle');
                      setProgress(0);
                      setError(null);
                    }}
                    size="sm"
                  >
                    <IconX size={14} />
                  </ActionIcon>
                </Tooltip>
              </>
            )}
            {uploadState === 'error' && (
              <>
                <Tooltip label="重试">
                  <ActionIcon
                    variant="subtle"
                    color="orange"
                    onClick={upload.retry}
                    size="sm"
                  >
                    <IconRefresh size={14} />
                  </ActionIcon>
                </Tooltip>
                <Tooltip label="清除">
                  <ActionIcon
                    variant="subtle"
                    color="red"
                    onClick={() => {
                      upload.cancel();
                      setUploadState('idle');
                      setProgress(0);
                      setError(null);
                      setDisplayFileName(externalFileName);
                      setDisplayFileUrl(externalFileUrl);
                      setDisplayFileSize(externalFileSize);
                    }}
                    size="sm"
                  >
                    <IconX size={14} />
                  </ActionIcon>
                </Tooltip>
              </>
            )}
            {(uploadState === 'idle' || (uploadState === 'completed' && !displayFileName)) && (
              <FileButton onChange={handleFileSelect} accept={accept}>
                {(props) => (
                  <Button
                    {...props}
                    size="xs"
                    variant="light"
                    leftSection={<IconUpload size={14} />}
                  >
                    上传文件
                  </Button>
                )}
              </FileButton>
            )}
            {(uploadState === 'completed' || (displayFileName && uploadState === 'idle')) && (
              <Button
                size="xs"
                variant="light"
                color="green"
                leftSection={<IconUpload size={14} />}
                onClick={() => {
                  upload.cancel();
                  setUploadState('idle');
                  setProgress(0);
                  setError(null);
                }}
              >
                重新上传
              </Button>
            )}
          </Group>
        </Group>

        {/* 阶段说明文字 */}
        {uploadStageText && (uploadState === 'uploading' || uploadState === 'paused') && (
          <Text size="xs" c="dimmed">
            {uploadStageText}
          </Text>
        )}

        {/* 进度条 */}
        {(uploadState === 'uploading' || uploadState === 'paused') && (
          <Box>
            <Group gap="xs" mb={2}>
              <Progress
                value={progress}
                color={progressColor}
                size="sm"
                style={{ flex: 1 }}
                animated={uploadState === 'uploading'}
              />
              <Text
                size="xs"
                c="dimmed"
                style={{ minWidth: 35, textAlign: 'right' }}
              >
                {progress}%
              </Text>
            </Group>
            {uploadState === 'uploading' && speed > 0 && (
              <Text size="xs" c="dimmed">
                {formatSpeedLocal(speed)}
              </Text>
            )}
          </Box>
        )}

        {/* 错误信息 */}
        {uploadState === 'error' && error && (
          <Text size="xs" c="red">
            {error}
          </Text>
        )}
      </Stack>
    </Paper>
  );
}

// ======================== 主组件 ========================

export function FileUploader({
  accept = '.pdf,.docx,.jpg,.png,.zip,.rar',
  maxSize = 10 * 1024 * 1024 * 1024, // 10GB
  multiple = true,
  concurrency = 3,
  inline = false,
  singleFile = true,
  fileUrl,
  fileName,
  fileSize,
  onUploadComplete,
  onUploadError,
  contextType,
  contextId,
  fileGroupId,
  changeReason,
}: FileUploaderProps) {
  const [opened, { open, close }] = useDisclosure(false);
  const [tasks, setTasks] = useState<UploadTask[]>([]);
  const controllersRef = useRef<Map<string, UploadControllerHandle>>(new Map());

  // ========== 内联模式：直接渲染 InlineFileAttach，不弹 Modal ==========
  if (inline && singleFile) {
    return (
      <InlineFileAttach
        fileUrl={fileUrl}
        fileName={fileName}
        fileSize={fileSize}
        maxSize={maxSize}
        accept={accept}
        concurrency={concurrency}
        onUploadComplete={onUploadComplete}
        onUploadError={onUploadError}
      />
    );
  }

  // ======================== 文件校验 ========================

  const validateFile = useCallback(
    (file: File): string | null => {
      if (file.size > maxSize) {
        return `文件 ${file.name} 超过大小限制 ${formatSize(maxSize)}`;
      }
      if (accept) {
        const ext = '.' + getFileExt(file.name);
        const allowed = accept.split(',').map((s) => s.trim().toLowerCase());
        if (!allowed.includes(ext.toLowerCase()) && !allowed.includes('.*')) {
          return `文件 ${file.name} 类型不支持，支持格式: ${accept}`;
        }
      }
      return null;
    },
    [accept, maxSize]
  );

  // ======================== 添加文件 ========================

  const addFiles = useCallback(
    (files: File[]) => {
      const newTasks: UploadTask[] = [];

      for (const file of files) {
        const error = validateFile(file);
        newTasks.push({
          id: Math.random().toString(36).substr(2, 9),
          file,
          progress: 0,
          status: error ? ('error' as UploadStatus) : ('idle' as UploadStatus),
          speed: 0,
          error: error || null,
        });
      }

      setTasks((prev) => [...prev, ...newTasks]);
      open();
    },
    [validateFile, open]
  );

  // ======================== 回调处理 ========================

  const handleProgress = useCallback((id: string, pct: number) => {
    setTasks((prev) => prev.map((t) => (t.id === id ? { ...t, progress: pct } : t)));
  }, []);

  const handleComplete = useCallback(
    (id: string, fileUrl: string) => {
      const task = tasks.find((t) => t.id === id);
      setTasks((prev) =>
        prev.map((t) =>
          t.id === id ? { ...t, progress: 100, status: 'completed', fileUrl } : t
        )
      );
      if (task) {
        onUploadComplete?.({
          fileUrl,
          originalName: task.file.name,
          fileSize: task.file.size,
        });
      }
    },
    [tasks, onUploadComplete]
  );

  const handleError = useCallback(
    (id: string, err: string) => {
      setTasks((prev) =>
        prev.map((t) => (t.id === id ? { ...t, status: 'error', error: err } : t))
      );
      onUploadError?.(err);
    },
    [onUploadError]
  );

  const handleStatusChange = useCallback((id: string, status: UploadStatus) => {
    setTasks((prev) => prev.map((t) => (t.id === id ? { ...t, status } : t)));
  }, []);

  const handleSpeedChange = useCallback((id: string, speed: number) => {
    setTasks((prev) => prev.map((t) => (t.id === id ? { ...t, speed } : t)));
  }, []);

  const handleControllerReady = useCallback(
    (id: string, controller: UploadControllerHandle) => {
      controllersRef.current.set(id, controller);
    },
    []
  );

  // ======================== 操作处理 ========================

  const pauseUpload = useCallback((taskId: string) => {
    const controller = controllersRef.current.get(taskId);
    controller?.pause();
  }, []);

  const resumeUpload = useCallback((taskId: string) => {
    const controller = controllersRef.current.get(taskId);
    controller?.resume();
  }, []);

  const cancelUpload = useCallback((taskId: string) => {
    const controller = controllersRef.current.get(taskId);
    controller?.cancel();
    controllersRef.current.delete(taskId);
    setTasks((prev) => prev.filter((t) => t.id !== taskId));
  }, []);

  const retryUpload = useCallback(
    (taskId: string) => {
      const controller = controllersRef.current.get(taskId);
      controller?.retry();
    },
    []
  );

  // ======================== Dropzone 回调 ========================

  const handleDrop = useCallback(
    (files: File[]) => {
      addFiles(files);
    },
    [addFiles]
  );

  const handleReject = useCallback((rejections: FileRejection[]) => {
    for (const rejection of rejections) {
      const message = rejection.errors.map((e) => e.message).join(', ');
      notifications.show({
        title: '文件被拒绝',
        message: `${rejection.file.name}: ${message}`,
        color: 'red',
      });
    }
  }, []);

  // ======================== 统计 ========================

  const completedCount = tasks.filter((t) => t.status === 'completed').length;

  // ======================== 渲染 ========================

  return (
    <>
      {/* 上传按钮 */}
      <Button
        onClick={open}
        leftSection={<IconUpload size={16} />}
        variant="filled"
        color="blue"
        size="sm"
      >
        上传文件
      </Button>

      {/* 上传弹窗 */}
      <Modal
        opened={opened}
        onClose={close}
        title="上传文件"
        size={640}
      >
        <Stack gap="md">
          {/* 拖拽区域 */}
          <Dropzone
            onDrop={handleDrop}
            onReject={handleReject}
            accept={accept.split(',').map((ext) => ext.trim())}
            maxSize={maxSize}
            multiple={multiple}
            styles={{
              root: {
                border: '2px dashed',
                borderRadius: rem(12),
                padding: rem(32),
                cursor: 'pointer',
              },
            }}
          >
            <Stack gap="xs" align="center">
              <Dropzone.Accept>
                <IconUpload size={48} stroke={1.5} color="blue" />
              </Dropzone.Accept>
              <Dropzone.Reject>
                <IconX size={48} stroke={1.5} color="red" />
              </Dropzone.Reject>
              <Dropzone.Idle>
                <IconUpload size={48} stroke={1.5} color="gray" />
              </Dropzone.Idle>

              <Text size="md" fw={500} ta="center">
                拖拽文件到此处，或点击选择
              </Text>
              <Text size="xs" c="dimmed" ta="center">
                支持 {accept} 格式，单个文件最大 {formatSize(maxSize)}
              </Text>
            </Stack>
          </Dropzone>

          {/* 文件列表 */}
          <Stack gap="sm" style={{ maxHeight: 400, overflowY: 'auto' }}>
            {tasks.length === 0 ? (
              <Text c="dimmed" size="sm" ta="center" py="xl">
                暂无文件，请拖拽或选择文件上传
              </Text>
            ) : (
              tasks.map((task) => (
                <React.Fragment key={task.id}>
                  {/* 每个上传任务对应一个 UploadController 逻辑组件 */}
                  {/* 注意：UploadController 必须在所有状态下保持渲染，否则 unmount 会中断上传 */}
                  <UploadController
                    task={task}
                    concurrency={concurrency}
                    onProgress={handleProgress}
                    onComplete={handleComplete}
                    onError={handleError}
                    onStatusChange={handleStatusChange}
                    onSpeedChange={handleSpeedChange}
                    onControllerReady={handleControllerReady}
                  />
                  <UploadTaskItem
                    task={task}
                    onPause={pauseUpload}
                    onResume={resumeUpload}
                    onCancel={cancelUpload}
                    onRetry={retryUpload}
                  />
                </React.Fragment>
              ))
            )}
          </Stack>

          {/* 底部统计 */}
          <Group justify="space-between">
            <Text size="sm" c="dimmed">
              {completedCount} / {tasks.length} 完成
            </Text>
            <Button variant="subtle" size="sm" onClick={close}>
              关闭
            </Button>
          </Group>
        </Stack>
      </Modal>
    </>
  );
}