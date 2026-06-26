# OKF 知识树 + 文件上传 + Markdown 编辑器 + Skill 统一设计方案

## 一、整体架构：三层模型

```
┌─────────────────────────────────────────────────────────────────────┐
│                        前端展示层（React）                           │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────┐                  │
│  │ 知识树组件   │  │  Markdown    │  │ 文件上传   │                  │
│  │ (TreeNav)   │  │  编辑器组件   │  │ 组件      │                  │
│  └──────┬──────┘  └──────┬───────┘  └─────┬──────┘                  │
│         │                │                │                         │
├─────────┼────────────────┼────────────────┼─────────────────────────┤
│         │      Tauri/HTTP API（Rust 后端） │                         │
│  ┌──────┴──────┐  ┌──────┴───────┐  ┌─────┴──────┐                  │
│  │ 知识树服务   │  │ 知识条目服务  │  │ 上传服务   │                  │
│  │ knowledge_  │  │ knowledge_   │  │ (S3 +      │                  │
│  │ tree 命令   │  │ asset 命令   │  │ 大文件分片) │                  │
│  └──────┬──────┘  └──────┬───────┘  └─────┬──────┘                  │
├─────────┼────────────────┼────────────────┼─────────────────────────┤
│         │     数据库（PostgreSQL）          │                         │
│  ┌──────┴──────┐  ┌──────┴───────┐  ┌─────┴──────┐                  │
│  │ knowledge_  │  │ knowledge_   │  │ file_      │                  │
│  │ tree        │  │ asset        │  │ uploads    │                  │
│  │ (树形目录)  │  │ (OKF词条+文件)│  │ (大文件信息)│                  │
│  └─────────────┘  └──────────────┘  └────────────┘                  │
└─────────────────────────────────────────────────────────────────────┘
```

### 核心理念：Raw 素材层 + Wiki 词条层 双层架构

| 层级 | 表 | 说明 |
|---|---|---|
| **Raw 原始素材层** | `knowledge_asset` (okf_type=raw_source) | 上传的文件、导入的文档、原始素材 |
| **Wiki 标准化词条层** | `knowledge_asset` (okf_type=concept/fact/rule/...) | AI结构化后的知识词条，供 LLM/Agent 使用 |
| **树形导航层** | `knowledge_tree` | 纯层级目录结构，挂载 raw/wiki 节点 |

---

## 二、数据库表结构设计

### 2.1 knowledge_tree（树形导航表）— 精简版

```sql
CREATE TABLE IF NOT EXISTS {schema}.knowledge_tree (
    id BIGINT PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
    parent_id BIGINT REFERENCES {schema}.knowledge_tree(id),
    node_type VARCHAR(20) NOT NULL DEFAULT 'folder',
        -- folder / wiki_node / raw_file / skill
    title VARCHAR(255) NOT NULL,
    icon VARCHAR(50),
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_expanded BOOLEAN NOT NULL DEFAULT true,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.knowledge_tree IS '知识树节点表';
COMMENT ON COLUMN {schema}.knowledge_tree.node_type IS '节点类型：folder=文件夹 wiki_node=OKF词条 raw_file=原始文件 skill=Skill规则';
COMMENT ON COLUMN {schema}.knowledge_tree.parent_id IS '父节点ID（顶级节点为空）';
```

### 2.2 knowledge_asset（OKF 知识资产表）— 全新核心表

```sql
CREATE TABLE IF NOT EXISTS {schema}.knowledge_asset (
    id BIGINT PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
    tree_node_id BIGINT NOT NULL REFERENCES {schema}.knowledge_tree(id) ON DELETE CASCADE,

    -- 内容字段
    title VARCHAR(512) NOT NULL,
    content TEXT,                          -- Markdown 内容 / 词条正文
    content_html TEXT,                     -- 渲染后的 HTML（可选缓存）

    -- OKF 知识类型体系（机器可读的核心）
    okf_type VARCHAR(30) NOT NULL DEFAULT 'raw_source',
        -- raw_source / concept / fact / rule / param / process / case
    summary TEXT,                          -- AI 生成的摘要
    source VARCHAR(512),                   -- 知识溯源（URL / 文档名 / 引用）
    confidence FLOAT DEFAULT 1.0,          -- 可信度评分 0.0 ~ 1.0
    status VARCHAR(20) NOT NULL DEFAULT 'draft',
        -- draft / valid / outdated / banned

    -- 时效管理
    effective_at TIMESTAMP WITH TIME ZONE,  -- 生效时间
    expire_at TIMESTAMP WITH TIME ZONE,     -- 过期时间（NULL 表示长期有效）

    -- 知识关联
    relation_ids BIGINT[],                  -- 关联其他 knowledge_asset ID 列表
    tags TEXT[],                            -- 标签数组，用于快速过滤

    -- 文件上传专属字段
    file_url VARCHAR(1024),                 -- 对象存储地址
    file_name VARCHAR(512),                 -- 原始文件名
    file_size BIGINT,                       -- 文件大小（字节）
    file_mime VARCHAR(100),                 -- MIME 类型
    file_md5 VARCHAR(64),                   -- 文件 MD5（防重复上传）

    -- Markdown 编辑器元数据
    editor_mode VARCHAR(20) NOT NULL DEFAULT 'wysiwyg',
        -- wysiwyg / markdown / raw

    -- 基础字段
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.knowledge_asset IS 'OKF 知识资产表（承载文件、词条、知识元数据）';
COMMENT ON COLUMN {schema}.knowledge_asset.okf_type IS 'OKF知识类型：raw_source=原始素材 concept=概念 fact=事实 rule=规则 param=参数 process=流程 case=案例';
COMMENT ON COLUMN {schema}.knowledge_asset.status IS '状态：draft=草稿 valid=有效 outdated=过期 banned=禁用';
```

### 2.3 核心数据流：文件上传 → 知识树

```
用户上传 PDF/图片/文档
       │
       ▼
1. 文件存入 S3 对象存储
   └→ 返回 file_url
       │
       ▼
2. knowledge_tree 新建节点 (node_type = raw_file)
   └→ 得到 tree_node_id
       │
       ▼
3. knowledge_asset 新建记录 (okf_type = raw_source)
   └→ 绑定 tree_node_id、file_url、file_name、file_size、file_mime、file_md5
       │
       ▼
4. 用户/系统打开 Markdown 编辑器查看/编辑
       │
       ▼
5. AI 结构化解析（可选）
   └→ 从 raw_source 中提取内容
   └→ 在 knowledge_tree 新建 wiki_node
   └→ knowledge_asset 新建标准化词条 (okf_type = concept/fact/rule/...)
       │
       ▼
6. LLM/Agent 仅读取 wiki_node 标准化词条
   └→ raw_source 作为溯源备份
```

---

## 三、Markdown 编辑器组件设计

### 3.1 技术选型：MDXEditor

推荐使用 [MDXEditor](https://mdxeditor.dev/)（React + TypeScript，基于 Prosemirror，支持 WYSIWYG 和 Markdown 双模式）。

### 3.2 组件接口

```tsx
// MarkdownEditor.tsx
interface MarkdownEditorProps {
    // 内容
    content?: string;
    onChange?: (content: string) => void;

    // 元数据
    title: string;
    onTitleChange: (title: string) => void;

    // OKF 属性
    okfType: OkfType;
    onOkfTypeChange: (type: OkfType) => void;
    summary?: string;
    source?: string;
    status: 'draft' | 'valid' | 'outdated';

    // 文件上传
    fileUrl?: string;
    fileName?: string;
    onFileUpload?: (file: File) => Promise<string>; // 上传回调

    // 编辑器模式
    editorMode?: 'wysiwyg' | 'markdown' | 'raw';

    // 操作
    onSave?: () => void;
    saving?: boolean;
}

type OkfType = 'raw_source' | 'concept' | 'fact' | 'rule' | 'param' | 'process' | 'case';
```

### 3.3 编辑器布局

```
┌─────────────────────────────────────────────────┐
│ [← 返回知识树]   保存   撤销   重做   预览      │
├─────────────────────────────────────────────────┤
│ 标题： [________________________________]        │
│ 知识类型：[concept ▼]  状态：[valid ▼]          │
│ 摘要：[________________________________]        │
│ 来源：[________________________________]        │
├─────────────────────────────────────────────────┤
│  ┌───────────────────────────────────────────┐  │
│  │                                           │  │
│  │         Markdown 编辑区域                  │  │
│  │                                           │  │
│  │  (WYSIWYG / 源码 双模式切换)              │  │
│  │                                           │  │
│  └───────────────────────────────────────────┘  │
├─────────────────────────────────────────────────┤
│ 文件： [上传文件] sample.pdf (2.3 MB)            │
│ 标签： [资产] [合同] [供应商]                      │
│ 关联： [相关词条1] [相关词条2]                     │
└─────────────────────────────────────────────────┘
```

---

## 四、Skill 在知识树中的定位

### 4.1 Skill 即 OKF 规则节点

Skill（skill.md）本质上是一个 **OKF 的 rule/process 类型词条**，在知识树中以 `node_type = skill` 挂载。

```
知识树
├── 📁 资产管理办法 (folder)
│   ├── 📄 采购流程规范 (wiki_node, okf_type=process)
│   ├── 📄 报废条件判定 (wiki_node, okf_type=rule)
│   └── ⚡ 采购审批流程 (skill, 可执行的OKF规则)
├── 📁 合同模板 (folder)
│   └── 📄 标准采购合同 (wiki_node, okf_type=case)
└── 📁 上传文件 (folder)
    └── 📎 合同扫描件.pdf (raw_file, raw_source)
```

### 4.2 Skill 与知识树的联动

```
knowledge_tree 节点 (node_type = skill)
       │
       ▼
knowledge_asset (okf_type = rule / process)
       │
       ├── content = skill.md 的 Markdown 内容
       ├── source = 知识溯源
       ├── tags = ['skill', '可执行']
       │
       ▼
Skill Registry 注册
       │
       ▼
AI 对话 / Agent 自动调用
```

---

## 五、API 接口设计

### 5.1 知识树接口

```
GET    /api/knowledge/tree              → 获取完整知识树
POST   /api/knowledge/node              → 新增节点
PUT    /api/knowledge/node/{id}         → 更新节点
DELETE /api/knowledge/node/{id}         → 删除节点
PUT    /api/knowledge/node/{id}/move    → 移动节点
```

### 5.2 OKF 知识资产接口

```
GET    /api/knowledge/list              → 获取词条列表
GET    /api/knowledge/{id}              → 获取单条词条（含 Markdown 内容）
POST   /api/knowledge                   → 新增词条（含 Markdown 内容）
PUT    /api/knowledge/{id}              → 更新词条
DELETE /api/knowledge/{id}              → 删除词条
```

### 5.3 文件上传接口（复用现有大文件上传）

```
POST   /api/upload/init                 → 初始化上传（返回 Presigned URL）
POST   /api/upload/{id}/chunk           → 上报分片完成
GET    /api/upload/{id}/progress        → 查询上传进度
POST   /api/upload/{id}/complete        → 完成上传（合并分片）
DELETE /api/upload/{id}                 → 取消上传

# 上传完成后自动触发：
POST   /api/knowledge/attach-file       → 将上传的文件绑定到知识树节点
```

---

## 六、组件目录结构

```
src/
└── components/
    ├── KnowledgeTree/                   # 知识树组件（左侧导航）
    │   ├── index.tsx
    │   ├── TreeNode.tsx                # 树节点
    │   ├── TreeContextMenu.tsx         # 右键菜单（新建/删除/移动）
    │   └── types.ts
    │
    ├── MarkdownEditor/                  # Markdown 编辑器组件
    │   ├── index.tsx                   # 主组件
    │   ├── EditorToolbar.tsx           # 工具栏（保存/撤销/重做/模式切换）
    │   ├── MetaPanel.tsx               # 元数据面板（OKF类型/状态/标签）
    │   ├── FileAttachPanel.tsx         # 文件附件面板
    │   └── types.ts
    │
    ├── FileUploader/                   # 文件上传组件（复用现有）
    │   └── index.tsx                   # 拖拽/点击上传
    │
    └── pages/
        └── KnowledgePage.tsx           # 知识库页面（组合知识树 + 编辑器）
```

---

## 七、实施路线图

| 阶段 | 内容 | 预计工时 |
|---|---|---|
| **Phase 1** | 数据库表改造：修改 `knowledge_tree` + 新增 `knowledge_asset` | 2h |
| **Phase 2** | Rust 后端：模型定义 + 服务层 + Tauri 命令 | 4h |
| **Phase 3** | Markdown 编辑器组件开发（集成 MDXEditor） | 6h |
| **Phase 4** | 前端知识树组件改造：适配新表结构 | 3h |
| **Phase 5** | 文件上传 → 知识树绑定链路打通 | 3h |
| **Phase 6** | Skill 挂载到知识树 + AI 联想链路 | 3h |
| **Phase 7** | 测试 + 联调 | 3h |

---

## 八、与现有代码的兼容性

### 当前 asset_knowledge 表如何处理？

当前 `asset_knowledge` 表（`tenant_tables.sql` 第 311 行）是旧的通用知识表。迁移策略：

1. **初期**：`knowledge_asset` 和 `asset_knowledge` 两张表并存
2. **迁移期**：前端知识树页面切换到 `knowledge_asset`，旧 `asset_knowledge` 作只读
3. **最终**：数据迁移完成后废弃 `asset_knowledge`

### 当前 knowledge_tree 如何处理？

直接替换表结构（因为当前表只存在于开发环境，没有生产数据）：

```sql
DROP TABLE IF EXISTS {schema}.knowledge_tree CASCADE;
-- 然后重新创建新版本