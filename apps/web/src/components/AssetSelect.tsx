'use client';

import React, { useEffect, useState, useMemo } from 'react';
import {
    Paper,
    Stack,
    Group,
    Text,
    TextInput,
    Select,
    Checkbox,
    Radio,
    Loader,
    Alert,
    ScrollArea,
    Badge,
    Divider,
    Button,
    Modal,
} from '@mantine/core';
import { IconAlertCircle, IconSearch, IconSelector } from '@tabler/icons-react';
import { getCategories, type Category } from '@/services/categoryService';
import { getHardwareAssets, type HardwareAssetView } from '@/services/hardwareService';
import { getIntangibleAssets, type IntangibleAssetView } from '@/services/softwareService';

// ======================== 联合资产类型 ========================

/** 统一资产视图，兼容固定资产和无形资产 */
export interface AssetItem {
    id: string;
    asset_no: string;
    asset_name: string;
    asset_type: 'fixed' | 'intangible';
    category_id: string;
    category_name: string;
    status: number;
    manufacturer: string | null;
    model: string | null;
    /** 固定资产的序列号 */
    sn: string | null;
}

// ======================== Props ========================

export interface AssetSelectProps {
    /** 模式: single=单选框, multiple=多选框 */
    mode: 'single' | 'multiple';
    /** 当前选中的资产ID (single模式) */
    value?: string | null;
    /** 当前选中的资产ID列表 (multiple模式) */
    values?: string[];
    /** 选择变化回调 (single模式) */
    onChange?: (id: string | null) => void;
    /** 选择变化回调 (multiple模式) */
    onChangeMultiple?: (ids: string[]) => void;
    /** 资产类型筛选: 'fixed' | 'intangible' | 'all' */
    assetType?: 'fixed' | 'intangible' | 'all';
    /** 是否禁用 */
    disabled?: boolean;
    /** 组件标题 */
    label?: string;
}

// ======================== 组件 ========================

const AssetSelect: React.FC<AssetSelectProps> = ({
    mode,
    value,
    values = [],
    onChange,
    onChangeMultiple,
    assetType: initialAssetType = 'all',
    disabled = false,
    label = '选择资产',
}) => {
    const [assets, setAssets] = useState<AssetItem[]>([]);
    const [categories, setCategories] = useState<Category[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [searchText, setSearchText] = useState('');
    const [typeFilter, setTypeFilter] = useState<string>(initialAssetType);
    const [opened, setOpened] = useState(false);

    // 加载数据
    useEffect(() => {
        const loadData = async () => {
            setLoading(true);
            setError(null);
            try {
                const [categoriesData, hardwareData, intangibleData] = await Promise.all([
                    getCategories(),
                    getHardwareAssets(),
                    getIntangibleAssets(),
                ]);
                setCategories(categoriesData);

                // 合并资产
                const merged: AssetItem[] = [
                    ...hardwareData.map((h) => ({
                        id: h.id,
                        asset_no: h.asset_no,
                        asset_name: h.asset_name,
                        asset_type: 'fixed' as const,
                        category_id: h.category_id,
                        category_name: getCategoryName(categoriesData, h.category_id),
                        status: h.status,
                        manufacturer: h.manufacturer,
                        model: h.model,
                        sn: h.sn,
                    })),
                    ...intangibleData.map((i) => ({
                        id: String(i.id),
                        asset_no: i.asset_no,
                        asset_name: i.asset_name,
                        asset_type: 'intangible' as const,
                        category_id: String(i.category_id),
                        category_name: getCategoryName(categoriesData, String(i.category_id)),
                        status: i.status,
                        manufacturer: i.manufacturer,
                        model: i.model,
                        sn: null,
                    })),
                ];

                setAssets(merged);
            } catch (err) {
                console.error('加载资产数据失败:', err);
                setError(typeof err === 'string' ? err : '加载资产数据失败');
            } finally {
                setLoading(false);
            }
        };
        loadData();
    }, []);

    // 获取分类名称
    function getCategoryName(cats: Category[], catId: string): string {
        const cat = cats.find((c) => c.id === catId);
        return cat ? cat.category_name : `分类#${catId}`;
    }

    // 根据类型筛选和搜索文本过滤
    const filteredAssets = useMemo(() => {
        let list = assets;

        // 按资产类型筛选
        if (typeFilter !== 'all') {
            list = list.filter((a) => a.asset_type === typeFilter);
        }

        // 按搜索文本筛选
        if (searchText.trim()) {
            const s = searchText.trim().toLowerCase();
            list = list.filter(
                (a) =>
                    a.asset_no.toLowerCase().includes(s) ||
                    a.asset_name.toLowerCase().includes(s) ||
                    (a.sn && a.sn.toLowerCase().includes(s)) ||
                    (a.manufacturer && a.manufacturer.toLowerCase().includes(s)) ||
                    (a.model && a.model.toLowerCase().includes(s))
            );
        }

        return list;
    }, [assets, typeFilter, searchText]);

    // 获取状态标签
    const getStatusBadge = (status: number) => {
        const map: Record<number, { label: string; color: string }> = {
            0: { label: '在库', color: 'blue' },
            1: { label: '使用中', color: 'green' },
            2: { label: '维修中', color: 'orange' },
            3: { label: '已报废', color: 'red' },
            4: { label: '已领用', color: 'teal' },
        };
        return map[status] || { label: '未知', color: 'gray' };
    };

    // 处理单选
    const handleSingleChange = (id: string) => {
        if (onChange) {
            onChange(value === id ? null : id);
        }
    };

    // 处理多选
    const handleMultipleChange = (id: string, checked: boolean) => {
        if (!onChangeMultiple) return;
        if (checked) {
            onChangeMultiple([...values, id]);
        } else {
            onChangeMultiple(values.filter((v) => v !== id));
        }
    };

    // 类型筛选选项
    const typeFilterOptions = [
        { value: 'all', label: '全部资产' },
        { value: 'fixed', label: '固定资产' },
        { value: 'intangible', label: '无形资产' },
    ];

    // 获取已选资产的显示文本
    const getSelectedLabel = (): string => {
        if (mode === 'single') {
            if (!value) return label;
            const asset = assets.find((a) => a.id === value);
            return asset ? `${asset.asset_no} - ${asset.asset_name}` : `${label} (已选)`;
        } else {
            if (values.length === 0) return label;
            return `${label} (已选 ${values.length} 项)`;
        }
    };

    // 弹窗内容
    const modalBody = loading ? (
        <Group justify="center" py="xl">
            <Loader />
            <Text c="dimmed">加载资产数据...</Text>
        </Group>
    ) : error ? (
        <Alert icon={<IconAlertCircle size={16} />} title="加载失败" color="red">
            {error}
        </Alert>
    ) : (
        <Stack gap="sm">
            {/* 筛选栏 */}
            <Group gap="sm" wrap="nowrap">
                <Select
                    data={typeFilterOptions}
                    value={typeFilter}
                    onChange={(val) => setTypeFilter(val || 'all')}
                    style={{ minWidth: 140 }}
                    size="sm"
                    disabled={disabled}
                />
                <TextInput
                    placeholder="搜索资产编号、名称、序列号..."
                    leftSection={<IconSearch size={14} />}
                    value={searchText}
                    onChange={(e) => setSearchText(e.target.value)}
                    style={{ flex: 1 }}
                    size="sm"
                    disabled={disabled}
                />
            </Group>

            {/* 资产列表 */}
            <ScrollArea h={400} type="auto">
                {filteredAssets.length === 0 ? (
                    <Text ta="center" c="dimmed" py="xl">
                        {searchText ? '未找到匹配的资产' : '暂无资产数据'}
                    </Text>
                ) : mode === 'single' ? (
                    /* ===== 单选模式 ===== */
                    <Radio.Group
                        value={value ?? ''}
                        onChange={(val) => {
                            if (onChange) onChange(val || null);
                        }}
                        disabled={disabled}
                    >
                        <Stack gap={4}>
                            {filteredAssets.map((asset) => {
                                const statusInfo = getStatusBadge(asset.status);
                                return (
                                    <Radio
                                        key={asset.id}
                                        value={asset.id}
                                        label={
                                            <Group gap="xs" wrap="nowrap">
                                                <Text size="sm" fw={500}>
                                                    {asset.asset_no}
                                                </Text>
                                                <Text size="sm" lineClamp={1}>
                                                    {asset.asset_name}
                                                </Text>
                                                <Badge
                                                    size="xs"
                                                    variant="light"
                                                    color={
                                                        asset.asset_type === 'fixed'
                                                            ? 'violet'
                                                            : 'cyan'
                                                    }
                                                >
                                                    {asset.asset_type === 'fixed'
                                                        ? '硬件'
                                                        : '软件'}
                                                </Badge>
                                                <Badge
                                                    size="xs"
                                                    variant="light"
                                                    color={statusInfo.color}
                                                >
                                                    {statusInfo.label}
                                                </Badge>
                                                {asset.sn && (
                                                    <Text size="xs" c="dimmed">
                                                        SN: {asset.sn}
                                                    </Text>
                                                )}
                                                <Text size="xs" c="dimmed">
                                                    {asset.category_name}
                                                </Text>
                                            </Group>
                                        }
                                    />
                                );
                            })}
                        </Stack>
                    </Radio.Group>
                ) : (
                    /* ===== 多选模式 ===== */
                    <Stack gap={4}>
                        {filteredAssets.map((asset) => {
                            const statusInfo = getStatusBadge(asset.status);
                            const checked = values.includes(asset.id);
                            return (
                                <Checkbox
                                    key={asset.id}
                                    checked={checked}
                                    onChange={(e) =>
                                        handleMultipleChange(
                                            asset.id,
                                            e.currentTarget.checked
                                        )
                                    }
                                    disabled={disabled}
                                    label={
                                        <Group gap="xs" wrap="nowrap">
                                            <Text size="sm" fw={500}>
                                                {asset.asset_no}
                                            </Text>
                                            <Text size="sm" lineClamp={1}>
                                                {asset.asset_name}
                                            </Text>
                                            <Badge
                                                size="xs"
                                                variant="light"
                                                color={
                                                    asset.asset_type === 'fixed'
                                                        ? 'violet'
                                                        : 'cyan'
                                                }
                                            >
                                                {asset.asset_type === 'fixed'
                                                    ? '硬件'
                                                    : '软件'}
                                            </Badge>
                                            <Badge
                                                size="xs"
                                                variant="light"
                                                color={statusInfo.color}
                                            >
                                                {statusInfo.label}
                                            </Badge>
                                            {asset.sn && (
                                                <Text size="xs" c="dimmed">
                                                    SN: {asset.sn}
                                                </Text>
                                            )}
                                            <Text size="xs" c="dimmed">
                                                {asset.category_name}
                                            </Text>
                                        </Group>
                                    }
                                />
                            );
                        })}
                    </Stack>
                )}
            </ScrollArea>

            <Divider />

            {/* 底部统计 */}
            <Group justify="space-between">
                <Text size="xs" c="dimmed">
                    共 {filteredAssets.length} 项资产
                    {typeFilter !== 'all' &&
                        ` (${typeFilter === 'fixed' ? '固定资产' : '无形资产'})`}
                </Text>
                {mode === 'multiple' && (
                    <Text size="xs" fw={500}>
                        已选: {values.length} 项
                    </Text>
                )}
            </Group>
        </Stack>
    );

    return (
        <>
            {/* 触发按钮 */}
            <Button
                variant="default"
                fullWidth
                rightSection={<IconSelector size={16} />}
                onClick={() => setOpened(true)}
                disabled={disabled}
                styles={{
                    root: {
                        justifyContent: 'space-between',
                        fontWeight: 400,
                        color: value || values.length > 0 ? undefined : '#868e96',
                    },
                }}
            >
                {getSelectedLabel()}
            </Button>

            {/* 选择弹窗 */}
            <Modal
                opened={opened}
                onClose={() => setOpened(false)}
                title={label}
                size="xl"
            >
                {modalBody}
            </Modal>
        </>
    );
};

export default AssetSelect;
