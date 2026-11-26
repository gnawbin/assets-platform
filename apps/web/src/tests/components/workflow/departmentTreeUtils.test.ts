import {
    buildDepartmentTree,
    departmentMatches,
    getDepartmentDescendantIds,
    getDepartmentPath,
} from '@/components/workflow/departmentTreeUtils';
import type { Department } from '@/services/departmentService';

function dept(id: string, name: string, parentId: string | null = null): Department {
    return {
        id,
        department_name: name,
        parent_id: parentId,
        leader_id: null,
        description: null,
        created_by: null,
        created_at: null,
        updated_by: null,
        updated_at: null,
        deleted: null,
        tenant_id: '1',
    };
}

describe('departmentTreeUtils', () => {
    const departments = [
        dept('1', '技术部'),
        dept('2', '前端组', '1'),
        dept('3', '后端组', '1'),
        dept('4', '财务部'),
    ];

    it('构建父子部门树', () => {
        const tree = buildDepartmentTree(departments);
        expect(tree).toHaveLength(2);
        expect(tree[0].department_name).toBe('技术部');
        expect(tree[0].children.map((item) => item.department_name)).toEqual([
            '前端组',
            '后端组',
        ]);
    });

    it('计算自身及后代部门 ID', () => {
        expect(Array.from(getDepartmentDescendantIds(departments, '1')).sort()).toEqual([
            '1',
            '2',
            '3',
        ]);
    });

    it('生成部门全路径', () => {
        expect(getDepartmentPath(departments, '2')).toBe('技术部 / 前端组');
    });

    it('按部门名 / 路径匹配关键字', () => {
        expect(departmentMatches(departments, departments[1], '前端')).toBe(true);
        expect(departmentMatches(departments, departments[1], '技术部')).toBe(true);
        expect(departmentMatches(departments, departments[1], '财务')).toBe(false);
    });
});
