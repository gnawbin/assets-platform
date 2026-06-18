'use client';

import React, { useEffect, useState, useCallback } from 'react';
import {
    listSkills,
    getSkill,
    executeSkill,
    registerCustomSkill,
    unregisterSkill,
    type SkillMeta,
    type SkillResult,
} from '@/services/skillService';
import { useAuthStore } from '@/store/authStore';

// ======================== 图标组件 ========================

const PlayIcon = () => (
    <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832l3.197-2.132a1 1 0 000-1.664z" />
        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
    </svg>
);

const SearchIcon = () => (
    <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
    </svg>
);

const CloseIcon = () => (
    <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
    </svg>
);

// ======================== Skill 卡片组件 ========================

interface SkillCardProps {
    skill: SkillMeta;
    onExecute: (skill: SkillMeta) => void;
    onDelete: (skillId: string) => void;
}

const SkillCard: React.FC<SkillCardProps> = ({ skill, onExecute, onDelete }) => {
    const isBuiltin = skill.skill_type === 'Builtin';

    return (
        <div className="bg-white rounded-lg border border-gray-200 p-4 hover:shadow-md transition-shadow">
            <div className="flex items-start justify-between">
                <div className="flex items-center gap-3">
                    <span className="text-2xl">{skill.icon}</span>
                    <div>
                        <h3 className="text-sm font-semibold text-gray-800">{skill.name}</h3>
                        <p className="text-xs text-gray-500 mt-0.5">{skill.description}</p>
                    </div>
                </div>
                <span className={`text-xs px-2 py-0.5 rounded-full ${isBuiltin
                    ? 'bg-blue-100 text-blue-600'
                    : 'bg-green-100 text-green-600'
                    }`}>
                    {isBuiltin ? '内置' : '自定义'}
                </span>
            </div>

            <div className="mt-3 flex items-center gap-3 text-xs text-gray-400">
                <span>v{skill.version}</span>
                <span>作者: {skill.author}</span>
            </div>

            <div className="mt-3 flex gap-2">
                <button
                    className="flex items-center gap-1 px-3 py-1.5 text-xs text-white bg-blue-600 hover:bg-blue-700 rounded-md transition-colors"
                    onClick={() => onExecute(skill)}
                >
                    <PlayIcon />
                    执行
                </button>
                {!isBuiltin && (
                    <button
                        className="px-3 py-1.5 text-xs text-red-600 hover:bg-red-50 rounded-md transition-colors"
                        onClick={() => onDelete(skill.id)}
                    >
                        移除
                    </button>
                )}
            </div>
        </div>
    );
};

// ======================== 主页面 ========================

export default function SkillsPage() {
    const [skills, setSkills] = useState<SkillMeta[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [searchQuery, setSearchQuery] = useState('');
    const { user } = useAuthStore();

    // 执行对话框
    const [showExecuteDialog, setShowExecuteDialog] = useState(false);
    const [executingSkill, setExecutingSkill] = useState<SkillMeta | null>(null);
    const [inputText, setInputText] = useState('');
    const [executing, setExecuting] = useState(false);
    const [result, setResult] = useState<SkillResult | null>(null);

    // 加载 Skill 列表
    const loadSkills = useCallback(async () => {
        try {
            setLoading(true);
            setError(null);
            const data = await listSkills();
            setSkills(data);
        } catch (err: unknown) {
            setError(err instanceof Error ? err.message : '加载 Skill 列表失败');
        } finally {
            setLoading(false);
        }
    }, []);

    useEffect(() => {
        loadSkills();
    }, [loadSkills]);

    // 过滤
    const filteredSkills = skills.filter(
        (s) =>
            s.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
            s.description.toLowerCase().includes(searchQuery.toLowerCase())
    );

    // 按类型分组
    const builtinSkills = filteredSkills.filter((s) => s.skill_type === 'Builtin');
    const customSkills = filteredSkills.filter((s) => s.skill_type === 'Custom');

    // 执行 Skill
    const handleExecute = (skill: SkillMeta) => {
        setExecutingSkill(skill);
        setInputText('');
        setResult(null);
        setShowExecuteDialog(true);
    };

    const handleConfirmExecute = async () => {
        if (!executingSkill || !user) return;
        setExecuting(true);
        setResult(null);
        try {
            const res = await executeSkill({
                skill_id: executingSkill.id,
                input_text: inputText,
                user_id: Number(user.id),
                tenant_id: Number(user.tenant_id ?? 1),
            });
            setResult(res);
        } catch (err: unknown) {
            setResult({
                output: err instanceof Error ? err.message : '执行失败',
                output_type: 'error',
                position: 'after_selection',
            });
        } finally {
            setExecuting(false);
        }
    };

    // 移除 Skill
    const handleDelete = async (skillId: string) => {
        if (!confirm('确定移除这个 Skill？')) return;
        try {
            await unregisterSkill(skillId);
            await loadSkills();
        } catch (err: unknown) {
            alert(err instanceof Error ? err.message : '移除失败');
        }
    };

    // ======================== 渲染 ========================

    return (
        <div className="h-full flex flex-col bg-gray-50">
            {/* 顶部栏 */}
            <div className="flex items-center justify-between px-6 py-3 bg-white border-b border-gray-200">
                <div>
                    <h1 className="text-base font-semibold text-gray-800">Zen Engine - Skill 管理</h1>
                    <p className="text-xs text-gray-500 mt-0.5">
                        管理和执行 AI 工作流 Skill（共 {skills.length} 个）
                    </p>
                </div>
                <div className="flex items-center gap-3">
                    <div className="relative">
                        <input
                            className="w-64 pl-8 pr-3 py-1.5 text-sm border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                            placeholder="搜索 Skill..."
                            value={searchQuery}
                            onChange={(e) => setSearchQuery(e.target.value)}
                        />
                        <span className="absolute left-2.5 top-1/2 -translate-y-1/2 text-gray-400">
                            <SearchIcon />
                        </span>
                    </div>
                </div>
            </div>

            {/* 内容区 */}
            <div className="flex-1 overflow-y-auto p-6">
                {loading ? (
                    <div className="flex items-center justify-center h-40 text-sm text-gray-400">
                        加载中...
                    </div>
                ) : error ? (
                    <div className="text-sm text-red-500 p-4 bg-red-50 rounded-lg">{error}</div>
                ) : filteredSkills.length === 0 ? (
                    <div className="flex items-center justify-center h-40 text-sm text-gray-400">
                        {searchQuery ? '未找到匹配的 Skill' : '暂无 Skill'}
                    </div>
                ) : (
                    <div className="space-y-6">
                        {/* 内置 Skill */}
                        {builtinSkills.length > 0 && (
                            <div>
                                <h2 className="text-sm font-semibold text-gray-600 mb-3 flex items-center gap-2">
                                    <span className="w-2 h-2 bg-blue-500 rounded-full"></span>
                                    内置 Skill ({builtinSkills.length})
                                </h2>
                                <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                                    {builtinSkills.map((skill) => (
                                        <SkillCard
                                            key={skill.id}
                                            skill={skill}
                                            onExecute={handleExecute}
                                            onDelete={handleDelete}
                                        />
                                    ))}
                                </div>
                            </div>
                        )}

                        {/* 自定义 Skill */}
                        {customSkills.length > 0 && (
                            <div>
                                <h2 className="text-sm font-semibold text-gray-600 mb-3 flex items-center gap-2">
                                    <span className="w-2 h-2 bg-green-500 rounded-full"></span>
                                    自定义 Skill ({customSkills.length})
                                </h2>
                                <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                                    {customSkills.map((skill) => (
                                        <SkillCard
                                            key={skill.id}
                                            skill={skill}
                                            onExecute={handleExecute}
                                            onDelete={handleDelete}
                                        />
                                    ))}
                                </div>
                            </div>
                        )}
                    </div>
                )}
            </div>

            {/* ======================== 执行对话框 ======================== */}
            {showExecuteDialog && executingSkill && (
                <div className="fixed inset-0 bg-black/30 flex items-center justify-center z-50">
                    <div className="bg-white rounded-lg shadow-xl w-[600px] p-6">
                        <div className="flex items-center justify-between mb-4">
                            <div className="flex items-center gap-2">
                                <span className="text-xl">{executingSkill.icon}</span>
                                <h3 className="text-base font-semibold text-gray-800">
                                    执行: {executingSkill.name}
                                </h3>
                            </div>
                            <button
                                className="p-1 text-gray-400 hover:text-gray-600"
                                onClick={() => setShowExecuteDialog(false)}
                            >
                                <CloseIcon />
                            </button>
                        </div>

                        <p className="text-xs text-gray-500 mb-4">{executingSkill.description}</p>

                        <div className="space-y-3">
                            <div>
                                <label className="block text-xs text-gray-500 mb-1">输入文本</label>
                                <textarea
                                    className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm h-28"
                                    value={inputText}
                                    onChange={(e) => setInputText(e.target.value)}
                                    placeholder="请输入要处理的文本..."
                                />
                            </div>
                        </div>

                        {/* 执行结果 */}
                        {result && (
                            <div className={`mt-4 p-3 rounded-md text-sm ${result.output_type === 'error'
                                ? 'bg-red-50 text-red-600'
                                : 'bg-blue-50 text-gray-700'
                                }`}>
                                <pre className="whitespace-pre-wrap font-sans text-sm">
                                    {result.output}
                                </pre>
                            </div>
                        )}

                        <div className="flex justify-end gap-2 mt-6">
                            <button
                                className="px-4 py-2 text-sm text-gray-600 hover:bg-gray-100 rounded-md"
                                onClick={() => setShowExecuteDialog(false)}
                            >
                                关闭
                            </button>
                            <button
                                className="flex items-center gap-1 px-4 py-2 text-sm text-white bg-blue-600 hover:bg-blue-700 rounded-md disabled:opacity-50"
                                onClick={handleConfirmExecute}
                                disabled={executing || !inputText.trim()}
                            >
                                {executing ? (
                                    <>
                                        <svg className="animate-spin h-4 w-4" viewBox="0 0 24 24">
                                            <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" />
                                            <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                                        </svg>
                                        执行中...
                                    </>
                                ) : (
                                    <>
                                        <PlayIcon />
                                        执行
                                    </>
                                )}
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
}
