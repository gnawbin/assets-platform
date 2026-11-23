import type { Department } from '@/services/departmentService';

export interface DepartmentTreeNode extends Department {
    children: DepartmentTreeNode[];
}

/** 把扁平部门列表构造成树。 */
export function buildDepartmentTree(departments: Department[]): DepartmentTreeNode[] {
    const map = new Map<string, DepartmentTreeNode>();
    const roots: DepartmentTreeNode[] = [];

    departments.forEach((dept) => {
        map.set(String(dept.id), { ...dept, id: String(dept.id), children: [] });
    });

    departments.forEach((dept) => {
        const node = map.get(String(dept.id));
        if (!node) return;
        const parentId = dept.parent_id ? String(dept.parent_id) : null;
        if (parentId && parentId !== '0' && map.has(parentId)) {
            map.get(parentId)!.children.push(node);
        } else {
            roots.push(node);
        }
    });

    return roots;
}

/** 获取部门自身 + 所有子部门 ID。 */
export function getDepartmentDescendantIds(
    departments: Department[],
    id: string
): Set<string> {
    const result = new Set<string>([id]);
    const queue = [id];
    while (queue.length > 0) {
        const current = queue.shift()!;
        departments
            .filter((dept) => String(dept.parent_id ?? '') === current)
            .forEach((child) => {
                const childId = String(child.id);
                if (!result.has(childId)) {
                    result.add(childId);
                    queue.push(childId);
                }
            });
    }
    return result;
}

/** 部门全路径名称。 */
export function getDepartmentPath(departments: Department[], id: string): string {
    const parts: string[] = [];
    const visited = new Set<string>();
    let current = departments.find((dept) => String(dept.id) === id);
    while (current && !visited.has(String(current.id))) {
        visited.add(String(current.id));
        parts.unshift(current.department_name);
        const parentId = current.parent_id ? String(current.parent_id) : null;
        current =
            parentId && parentId !== '0'
                ? departments.find((dept) => String(dept.id) === parentId)
                : undefined;
    }
    return parts.join(' / ');
}

/** 树节点是否命中关键字（部门名或路径）。 */
export function departmentMatches(
    departments: Department[],
    dept: Department,
    keyword: string
): boolean {
    const text = keyword.trim().toLowerCase();
    if (!text) return true;
    return (
        dept.department_name.toLowerCase().includes(text) ||
        getDepartmentPath(departments, String(dept.id)).toLowerCase().includes(text)
    );
}
