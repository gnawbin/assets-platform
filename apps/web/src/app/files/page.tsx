/**
 * 文件管理页面
 *
 * 大文件上传与管理的主页面，支持：
 * - 文件上传（拖拽/点击）
 * - 文件列表展示（列表/网格视图）
 * - 文件搜索
 * - 分类筛选
 * - 存储统计
 * - 右键菜单（下载/复制链接/删除）
 */

'use client';

import React, { useState, useEffect, useCallback } from 'react';
import {
  Box,
  Text,
  Group,
  Stack,
  Paper,
  TextInput,
  ActionIcon,
  Tooltip,
  Badge,
  Progress,
  Menu,
  rem,
} from '@mantine/core';
import {
  IconSearch,
  IconLayoutGrid,
  IconLayoutList,
  IconDownload,
  IconLink,
  IconTrash,
  IconUpload,
  IconFolder,
  IconFile,
} from '@tabler/icons-react';
import { FileUploader } from '@/components/FileUploader';

// ======================== 类型定义 ========================

interface FileItem {
  id: string;
  name: string;
  size: number;
  type: string;
  url: string;
  uploadedAt: string;
  uploadedBy: string;
}

// ======================== 工具函数 ========================

function formatSize(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

function formatDate(dateStr: string): string {
  const d = new Date(dateStr);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

function getFileIcon(name: string): string {
  const ext = name.split('.').pop()?.toLowerCase() || '';
  const iconMap: Record<string, string> = {
    pdf: '📄',
    doc: '📝',
    docx: '📝',
    xls: '📊',
    xlsx: '📊',
    jpg: '🖼️',
    jpeg: '🖼️',
    png: '🖼️',
    gif: '🖼️',
    webp: '🖼️',
    zip: '📦',
    rar: '📦',
    '7z': '📦',
    tar: '📦',
    gz: '📦',
    mp4: '🎬',
    avi: '🎬',
    mov: '🎬',
    mp3: '🎵',
    wav: '🎵',
  };
  return iconMap[ext] || '📄';
}

// ======================== 图标组件 ========================

const UploadIcon = () => (
  <svg className="w-12 h-12 text-gray-300" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={1.5}
      d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12"
    />
  </svg>
);

// ======================== 主页面 ========================

export default function FilesPage() {
  const [files, setFiles] = useState<FileItem[]>([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [viewMode, setViewMode] = useState<'list' | 'grid'>('list');
  const [selectedCategory, setSelectedCategory] = useState('all');
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    file: FileItem;
  } | null>(null);

  // 分类
  const categories = [
    { id: 'all', label: '全部文件', icon: '📁' },
    { id: 'image', label: '图片', icon: '🖼️' },
    { id: 'document', label: '文档', icon: '📝' },
    { id: 'archive', label: '压缩包', icon: '📦' },
    { id: 'video', label: '视频', icon: '🎬' },
    { id: 'other', label: '其他', icon: '📄' },
  ];

  // 存储统计
  const totalStorage = 100 * 1024 * 1024 * 1024; // 100GB
  const usedStorage = files.reduce((sum, f) => sum + f.size, 0);
  const storagePct = Math.round((usedStorage / totalStorage) * 100);

  // 筛选文件
  const filteredFiles = files.filter((f) => {
    const matchSearch = f.name.toLowerCase().includes(searchQuery.toLowerCase());
    const ext = f.name.split('.').pop()?.toLowerCase() || '';
    const matchCategory =
      selectedCategory === 'all' ||
      (selectedCategory === 'image' &&
        ['jpg', 'jpeg', 'png', 'gif', 'bmp', 'webp'].includes(ext)) ||
      (selectedCategory === 'document' &&
        ['pdf', 'doc', 'docx', 'xls', 'xlsx', 'txt', 'md'].includes(ext)) ||
      (selectedCategory === 'archive' && ['zip', 'rar', '7z', 'tar', 'gz'].includes(ext)) ||
      (selectedCategory === 'video' && ['mp4', 'avi', 'mov', 'mkv', 'wmv'].includes(ext)) ||
      (selectedCategory === 'other' &&
        !['jpg', 'jpeg', 'png', 'gif', 'pdf', 'doc', 'docx', 'xls', 'xlsx', 'zip', 'rar', '7z', 'mp4', 'avi', 'mov'].includes(ext));
    return matchSearch && matchCategory;
  });

  // 右键菜单
  useEffect(() => {
    const handleClick = () => setContextMenu(null);
    document.addEventListener('click', handleClick);
    return () => document.removeEventListener('click', handleClick);
  }, []);

  const handleContextMenu = (e: React.MouseEvent, file: FileItem) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, file });
  };

  const handleUploadComplete = useCallback(
    (result: { fileUrl: string; originalName: string; fileSize: number }) => {
      setFiles((prev) => [
        {
          id: Math.random().toString(36).substr(2, 9),
          name: result.originalName,
          size: result.fileSize,
          type: result.originalName.split('.').pop() || '',
          url: result.fileUrl,
          uploadedAt: new Date().toISOString(),
          uploadedBy: '当前用户',
        },
        ...prev,
      ]);
    },
    []
  );

  const handleDelete = useCallback((fileId: string) => {
    setFiles((prev) => prev.filter((f) => f.id !== fileId));
    setContextMenu(null);
  }, []);

  const handleCopyLink = useCallback((url: string) => {
    navigator.clipboard.writeText(url).catch(() => {
      // 忽略复制失败
    });
    setContextMenu(null);
  }, []);

  const handleDownload = useCallback((url: string, name: string) => {
    const a = document.createElement('a');
    a.href = url;
    a.download = name;
    a.click();
    setContextMenu(null);
  }, []);

  return (
    <Box style={{ display: 'flex', height: 'calc(100vh - 60px)' }}>
      {/* ======================== 左侧面板 ======================== */}
      <Paper
        w={220}
        style={{ borderRight: '1px solid var(--mantine-color-gray-3)' }}
        bg="gray.0"
        p="md"
      >
        <Stack gap="md">
          {/* 上传按钮 */}
          <FileUploader
            accept=".pdf,.docx,.jpg,.png,.zip,.rar"
            maxSize={10 * 1024 * 1024 * 1024}
            multiple={true}
            concurrency={3}
            onUploadComplete={handleUploadComplete}
          />

          {/* 分类列表 */}
          <Box>
            <Text size="xs" c="dimmed" fw={700} tt="uppercase" mb="xs">
              分类
            </Text>
            <Stack gap={4}>
              {categories.map((cat) => (
                <Box
                  key={cat.id}
                  onClick={() => setSelectedCategory(cat.id)}
                  style={{
                    cursor: 'pointer',
                    padding: `${rem(8)} ${rem(12)}`,
                    borderRadius: rem(8),
                    backgroundColor:
                      selectedCategory === cat.id
                        ? 'var(--mantine-color-blue-light)'
                        : 'transparent',
                    transition: 'background-color 200ms ease',
                  }}
                >
                  <Group gap="sm">
                    <Text size="lg">{cat.icon}</Text>
                    <Text
                      size="sm"
                      fw={selectedCategory === cat.id ? 600 : 400}
                      c={selectedCategory === cat.id ? 'blue' : 'dark'}
                    >
                      {cat.label}
                    </Text>
                  </Group>
                </Box>
              ))}
            </Stack>
          </Box>

          {/* 存储统计 */}
          <Box style={{ marginTop: 'auto' }} pt="md">
            <Text size="xs" c="dimmed" fw={700} tt="uppercase" mb="xs">
              存储统计
            </Text>
            <Paper withBorder p="sm" radius="md">
              <Group justify="space-between" mb={4}>
                <Text size="sm" c="dimmed">
                  已用 {storagePct}%
                </Text>
                <Text size="xs" c="dimmed">
                  {formatSize(usedStorage)} / {formatSize(totalStorage)}
                </Text>
              </Group>
              <Progress
                value={storagePct}
                color={storagePct > 80 ? 'red' : storagePct > 60 ? 'yellow' : 'blue'}
                size="sm"
              />
            </Paper>
          </Box>
        </Stack>
      </Paper>

      {/* ======================== 右侧主区域 ======================== */}
      <Box style={{ flex: 1, display: 'flex', flexDirection: 'column' }}>
        {/* 工具栏 */}
        <Paper
          style={{ borderBottom: '1px solid var(--mantine-color-gray-3)' }}
          bg="white"
          px="lg"
          py="sm"
        >
          <Group justify="space-between">
            <Group gap="md">
              {/* 搜索框 */}
              <TextInput
                placeholder="搜索文件..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                leftSection={<IconSearch size={16} />}
                size="sm"
                w={280}
              />

              {/* 视图切换 */}
              <Paper withBorder style={{ overflow: 'hidden' }}>
                <Group gap={0}>
                  <ActionIcon
                    variant={viewMode === 'list' ? 'filled' : 'subtle'}
                    color={viewMode === 'list' ? 'blue' : 'gray'}
                    onClick={() => setViewMode('list')}
                    size="sm"
                    radius={0}
                  >
                    <IconLayoutList size={16} />
                  </ActionIcon>
                  <ActionIcon
                    variant={viewMode === 'grid' ? 'filled' : 'subtle'}
                    color={viewMode === 'grid' ? 'blue' : 'gray'}
                    onClick={() => setViewMode('grid')}
                    size="sm"
                    radius={0}
                  >
                    <IconLayoutGrid size={16} />
                  </ActionIcon>
                </Group>
              </Paper>
            </Group>

            <Text size="sm" c="dimmed">
              共 {filteredFiles.length} 个文件
            </Text>
          </Group>
        </Paper>

        {/* 文件列表 */}
        <Box style={{ flex: 1, overflow: 'auto' }} p="lg">
          {filteredFiles.length === 0 ? (
            <Box
              style={{
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                justifyContent: 'center',
                height: '100%',
              }}
              c="dimmed"
            >
              <UploadIcon />
              <Text size="lg" fw={500} mt="md">
                暂无文件
              </Text>
              <Text size="sm" mt={4}>
                点击左侧「上传文件」按钮开始上传
              </Text>
            </Box>
          ) : viewMode === 'list' ? (
            /* 列表视图 */
            <Stack gap={4}>
              {filteredFiles.map((file) => (
                <Paper
                  key={file.id}
                  onContextMenu={(e) => handleContextMenu(e, file)}
                  style={{ cursor: 'pointer' }}
                  styles={{
                    root: {
                      '&:hover': {
                        backgroundColor: 'var(--mantine-color-gray-0)',
                      },
                    },
                  }}
                  p="sm"
                  radius="md"
                >
                  <Group gap="md" wrap="nowrap">
                    <Text size="xl">{getFileIcon(file.name)}</Text>
                    <Box style={{ flex: 1, minWidth: 0 }}>
                      <Text size="sm" fw={500} truncate>
                        {file.name}
                      </Text>
                      <Text size="xs" c="dimmed">
                        {formatSize(file.size)} · {formatDate(file.uploadedAt)}
                      </Text>
                    </Box>
                    <Text size="xs" c="dimmed" style={{ whiteSpace: 'nowrap' }}>
                      {file.uploadedBy}
                    </Text>
                    <Group
                      gap={4}
                      style={{ opacity: 0 }}
                      className="file-actions"
                      onMouseEnter={(e) => {
                        (e.currentTarget as HTMLElement).style.opacity = '1';
                      }}
                      onMouseLeave={(e) => {
                        (e.currentTarget as HTMLElement).style.opacity = '0';
                      }}
                    >
                      <Tooltip label="下载">
                        <ActionIcon
                          variant="subtle"
                          color="blue"
                          size="sm"
                          onClick={() => handleDownload(file.url, file.name)}
                        >
                          <IconDownload size={16} />
                        </ActionIcon>
                      </Tooltip>
                      <Tooltip label="复制链接">
                        <ActionIcon
                          variant="subtle"
                          color="green"
                          size="sm"
                          onClick={() => handleCopyLink(file.url)}
                        >
                          <IconLink size={16} />
                        </ActionIcon>
                      </Tooltip>
                      <Tooltip label="删除">
                        <ActionIcon
                          variant="subtle"
                          color="red"
                          size="sm"
                          onClick={() => handleDelete(file.id)}
                        >
                          <IconTrash size={16} />
                        </ActionIcon>
                      </Tooltip>
                    </Group>
                  </Group>
                </Paper>
              ))}
            </Stack>
          ) : (
            /* 网格视图 */
            <Box
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fill, minmax(160px, 1fr))',
                gap: rem(16),
              }}
            >
              {filteredFiles.map((file) => (
                <Paper
                  key={file.id}
                  withBorder
                  onContextMenu={(e) => handleContextMenu(e, file)}
                  style={{ cursor: 'pointer' }}
                  styles={{
                    root: {
                      '&:hover': {
                        boxShadow: 'var(--mantine-shadow-md)',
                      },
                    },
                  }}
                  p="md"
                  radius="md"
                >
                  <Stack align="center" gap="xs">
                    <Text size="4rem">{getFileIcon(file.name)}</Text>
                    <Text size="sm" ta="center" truncate style={{ width: '100%' }}>
                      {file.name}
                    </Text>
                    <Text size="xs" c="dimmed">
                      {formatSize(file.size)}
                    </Text>
                    <Group gap={4}>
                      <Tooltip label="下载">
                        <ActionIcon
                          variant="subtle"
                          color="blue"
                          size="sm"
                          onClick={() => handleDownload(file.url, file.name)}
                        >
                          <IconDownload size={16} />
                        </ActionIcon>
                      </Tooltip>
                      <Tooltip label="删除">
                        <ActionIcon
                          variant="subtle"
                          color="red"
                          size="sm"
                          onClick={() => handleDelete(file.id)}
                        >
                          <IconTrash size={16} />
                        </ActionIcon>
                      </Tooltip>
                    </Group>
                  </Stack>
                </Paper>
              ))}
            </Box>
          )}
        </Box>
      </Box>

      {/* ======================== 右键菜单 ======================== */}
      {contextMenu && (
        <Paper
          withBorder
          shadow="lg"
          style={{
            position: 'fixed',
            left: contextMenu.x,
            top: contextMenu.y,
            zIndex: 1000,
            minWidth: 160,
          }}
          py={4}
        >
          <Menu.Item
            leftSection={<IconDownload size={16} />}
            onClick={() => handleDownload(contextMenu.file.url, contextMenu.file.name)}
          >
            下载
          </Menu.Item>
          <Menu.Item
            leftSection={<IconLink size={16} />}
            onClick={() => handleCopyLink(contextMenu.file.url)}
          >
            复制链接
          </Menu.Item>
          <Menu.Divider />
          <Menu.Item
            color="red"
            leftSection={<IconTrash size={16} />}
            onClick={() => handleDelete(contextMenu.file.id)}
          >
            删除
          </Menu.Item>
        </Paper>
      )}
    </Box>
  );
}
