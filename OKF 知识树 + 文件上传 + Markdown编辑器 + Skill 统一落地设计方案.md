# OKF 知识树 + 文件上传 + Markdown 编辑器 + Skill 统一落地设计方案

> 基于现有代码库的零破坏性改造方案
> 适用版本：当前 Git commit 41903909

---

## 一、改造原则：不破坏现有代码

| 原则 | 说明 |
|---|---|
| ✅ 不删除现有表 | `asset_knowledge` 表保持原样，新业务使用 `knowledge_asset` |
| ✅ 不修改现有 Rust struct | 只新增 `KnowledgeAsset` struct，不改 `KnowledgeTree`/`AssetKnowledge` |
| ✅ 不修改现有 Service 函数 | 新增 `knowledge_asset_service.rs`，不改 `knowledge_service.rs` |
| ✅ 不修改现有 Command | 新增知识资产命令，不改现有知识树命令 |
| ✅ 前端增量开发 | 新的 `MarkdownEditor`/`KnowledgeAssetPage` 是独立组件 |

---

## 二、现有代码库核心结构（需了解的背景）

```
tenant_tables.sql:
  knowledge_tree    ← 树形目录结构（现有）
  asset_knowledge   ← 旧知识条目表（保留不动）
  file_uploads      ← 大文件上传记录表（现有，可复用）

models.rs:
  KnowledgeTree     ← 树节点 struct（现有，不改）
  KnowledgeTreeNode ← 带子节点的树（现有，不改）
  AssetKnowledge    ← 旧知识条目 struct（现有，不改）

knowledge_service.rs:
  get_knowledge_tree()      ← 现有，不改
  insert_knowledge_node()   ← 现有，不改
  ...                       ← 其他现有函数不改

knowledge_commands.rs:
  get_knowledge_tree         ← 现有，不改
  insert_knowledge_node      ← 现有，不改
  insert_knowledge           ← 现有，不改
  ...
```

---

## 三、数据库变更（最小化方案）

### 3.1 knowledge_tree 表结构不变，只扩展 node_type 取值

现有 SQL（`tenant_tables.sql` 第 371 行）**不做任何 DDL 改动**。只扩展 `node_type` 字段的语义：

```
现有取值：folder / document / link
扩展取值：folder / document / link / raw_file / wiki_node / skill
```

这样知识树无需改表结构即可挂载新类型节点。

### 3.2 新增 knowledge_asset 表（全新表，不碰旧表）

```sql
-- 全新知识资产表，与旧 asset_knowledge 完全独立
CREATE TABLE IF NOT EXISTS {schema}.knowledge_asset (
    id BIGINT PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
    tree_node_id BIGINT NOT NULL REFERENCES {schema}.knowledge_tree(id) ON DELETE CASCADE,

    -- 内容字段
    title VARCHAR(512) NOT NULL,
    content TEXT,
    content_html TEXT,

    -- OKF 知识类型
    okf_type VARCHAR(30) NOT NULL DEFAULT 'raw_source',
        -- raw_source / concept / fact / rule / param / process / case
    summary TEXT,
    source VARCHAR(512),
    confidence FLOAT DEFAULT 1.0,
    status VARCHAR(20) NOT NULL DEFAULT 'draft',
        -- draft / valid / outdated / banned

    -- 时效管理
    effective_at TIMESTAMP WITH TIME ZONE,
    expire_at TIMESTAMP WITH TIME ZONE,

    -- 知识关联
    relation_ids BIGINT[],
    tags TEXT[],

    -- 文件字段
    file_url VARCHAR(1024),
    file_name VARCHAR(512),
    file_size BIGINT,
    file_mime VARCHAR(100),
    file_md5 VARCHAR(64),

    -- 编辑器模式
    editor_mode VARCHAR(20) NOT NULL DEFAULT 'wysiwyg',

    -- 基础字段
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);
```

---

## 四、Rust 后端变更（增量，不影响现有代码）

### 4.1 新增 struct：`KnowledgeAsset`（models.rs 追加，不改现有 struct）

```rust
/// OKF 知识资产（全新表，不替代旧 asset_knowledge）
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct KnowledgeAsset {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    #[serde(serialize_with = "i64_to_string")]
    pub tree_node_id: i64,
    pub title: String,
    pub content: Option<String>,
    pub content_html: Option<String>,
    pub okf_type: String,
    pub summary: Option<String>,
    pub source: Option<String>,
    pub confidence: Option<f32>,
    pub status: String,
    pub effective_at: Option<DateTime<Utc>>,
    pub expire_at: Option<DateTime<Utc>>,
    pub relation_ids: Option<Vec<i64>>,
    pub tags: Option<Vec<String>>,
    pub file_url: Option<String>,
    pub file_name: Option<String>,
    pub file_size: Option<i64>,
    pub file_mime: Option<String>,
    pub file_md5: Option<String>,
    pub editor_mode: String,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}
```

### 4.2 新增 Service 文件：`knowledge_asset_service.rs`

```rust
//! 知识资产 Service（全新，不影响现有 knowledge_service.rs）

pub async fn get_knowledge_asset(tree_node_id: i64) -> Result<KnowledgeAsset, String>
pub async fn list_knowledge_assets(okf_type: Option<&str>, tags: Option<Vec<String>>) -> Result<Vec<KnowledgeAsset>, String>
pub async fn create_knowledge_asset(asset: &KnowledgeAsset) -> Result<KnowledgeAsset, String>
pub async fn update_knowledge_asset(id: i64, asset: &KnowledgeAsset) -> Result<KnowledgeAsset, String>
pub async fn delete_knowledge_asset(id: i64) -> Result<(), String>
pub async fn attach_file_to_asset(asset_id: i64, file_url: &str, file_name: &str, file_size: i64, file_mime: &str, file_md5: &str) -> Result<(), String>
```

### 4.3 新增 Command 文件：`knowledge_asset_commands.rs`

```rust
#[tauri::command]
pub async fn get_knowledge_asset(tree_node_id: String) -> Result<KnowledgeAsset, String>

#[tauri::command]  
pub async fn create_knowledge_asset(
    treeNodeId: String,
    title: String,
    okfType: String,
    content: Option<String>,
    summary: Option<String>,
    source: Option<String>,
    tags: Option<Vec<String>>,
    createdBy: Option<String>,
) -> Result<KnowledgeAsset, String>

#[tauri::command]
pub async fn update_knowledge_asset(
    id: String,
    title: Option<String>,
    content: Option<String>,
    okfType: Option<String>,
    summary: Option<String>,
    status: Option<String>,
    tags: Option<Vec<String>>,
    updatedBy: Option<String>,
) -> Result<KnowledgeAsset, String>

#[tauri::command]
pub async fn delete_knowledge_asset(id: String) -> Result<(), String>

#[tauri::command]
pub async fn attach_file_to_knowledge(
    assetId: String,
    fileUrl: String,
    fileName: String,
    fileSize: i64,
    fileMime: String,
    fileMd5: String,
) -> Result<(), String>
```

---

## 五、文件上传 → 知识树绑定链路

复用现有大文件分片上传能力（`s3.rs` + `upload.rs`），新增一个绑定接口：

```
用户上传文件
       │
       ▼
[现有] POST /api/upload/init          → 获取 Presigned URL
[现有] PUT  <Presigned URL>            → 前端直传到 S3
[现有] POST /api/upload/{id}/complete  → 完成上传
       │
       ▼
[新增] POST /api/knowledge/attach-file → 在 knowledge_tree 创建 raw_file 节点
                                        → 在 knowledge_asset 写入记录
                                        → 返回 tree_node_id
```

**attach-file 请求体**：
```json
{
    "parentNodeId": "树节点的父ID",
    "fileUrl": "https://s3-bucket/xxx.pdf",
    "fileName": "合同扫描件.pdf",
    "fileSize": 2356789,
    "fileMime": "application/pdf",
    "fileMd5": "d41d8cd98f00b204e9800998ecf8427e"
}
```

---

## 六、Markdown 编辑器组件

### 6.1 技术选型：MDXEditor

```bash
npm install @mdxeditor/editor @mdxeditor/react
```

### 6.2 组件 Props

```typescript
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
    onFileUpload?: (file: File) => Promise<string>;

    // 编辑器模式
    editorMode?: 'wysiwyg' | 'markdown' | 'raw';

    // 操作
    onSave?: () => void;
    saving?: boolean;
}

type OkfType = 'raw_source' | 'concept' | 'fact' | 'rule' | 'param' | 'process' | 'case';
```

### 6.3 组件布局

```
┌─────────────────────────────────────────────────┐
│ [← 返回]   保存   撤销   重做   预览   [源码]    │
├─────────────────────────────────────────────────┤
│ 标题： [________________________________]        │
│ 类型：[concept ▼]  状态：[valid ▼]               │
│ 摘要：[________________________________]        │
│ 来源：[________________________________]        │
├─────────────────────────────────────────────────┤
│  ┌───────────────────────────────────────────┐  │
│  │          Markdown 编辑区域                  │  │
│  │  （WYSIWYG / Markdown 源码 双模式）         │  │
│  │                                           │  │
│  └───────────────────────────────────────────┘  │
├─────────────────────────────────────────────────┤
│ 文件： [上传] sample.pdf | 2.3 MB               │
│ 标签： [资产] [合同]                              │
│ 关联： [词条A] [词条B]                            │
└─────────────────────────────────────────────────┘
```

---

## 七、Skill 在知识树中的定位

### 7.1 Skill = 可执行的 OKF 规则节点

- 知识树 `node_type = skill`
- 关联 `knowledge_asset` 的 `okf_type = rule` 或 `process`
- 内容 = `skill.md` 的 Markdown 格式
- 标签含 `['skill', '可执行']`，Skill Registry 自动扫描并注册

### 7.2 知识树中的示例

```
知识树
├── 📁 资产管理办法 (folder)
│   ├── 📄 采购流程 (wiki_node, okf_type=process)
│   ├── 📄 报废条件 (wiki_node, okf_type=rule)
│   └── ⚡ 采购审批流程 (skill, okf_type=rule, 可执行)
├── 📁 合同模板 (folder)
│   └── 📄 标准合同 (wiki_node, okf_type=case)
└── 📁 上传文件 (folder)
    └── 📎 合同扫描件.pdf (raw_file, raw_source)
```

---

## 八、前端组件目录（增量，不改造现有组件）

```
src/
└── components/
    ├── KnowledgeAsset/               # [新增] 知识资产相关组件
    │   ├── KnowledgeAssetPage.tsx    # 知识资产详情页（树+编辑器联动）
    │   └── AssetFileUploader.tsx     # 文件上传→绑定知识树
    │
    ├── MarkdownEditor/               # [新增] Markdown 编辑器组件
    │   ├── index.tsx                 # 主组件
    │   ├── EditorToolbar.tsx         # 工具栏
    │   ├── MetaPanel.tsx             # OKF 元数据面板
    │   ├── FileAttachPanel.tsx       # 文件附件面板
    │   └── types.ts                  # 类型定义
    │
    ├── KnowledgeTree/                # [改造] 现有知识树组件
    │   ├── index.tsx                 # 改造：适配新 node_type
    │   ├── TreeNode.tsx              # 改造：区分图标
    │   └── types.ts
    │
    └── pages/
        └── KnowledgePage.tsx         # [新增] 知识库主页面
```

---

## 九、实施路线图（非破坏性，可分阶段上线）

| 阶段 | 内容 | 改动文件 | 影响范围 |
|---|---|---|---|
| **Phase 1** | 新增 `knowledge_asset` DDL | `tenant_tables.sql` 追加 | ✅ 无影响，只加新表 |
| **Phase 2** | 新增 Rust struct + Service + Command | `models.rs` 追加 + 新文件 | ✅ 无影响，纯增量 |
| **Phase 3** | 前端 Markdown 编辑器组件 | 新组件 `MarkdownEditor/` | ✅ 无影响，新组件 |
| **Phase 4** | 文件上传 → 知识树绑定 | `attach-file` 命令 | ✅ 无影响，新接口 |
| **Phase 5** | 知识树组件改造（可选） | `KnowledgeTree/index.tsx` | ⚠️ 展示优化 |
| **Phase 6** | Skill 挂载到知识树 | Skill Registry 适配 | ✅ 无影响 |
| **Phase 7** | 测试联调 | — | — |

---

## 十、关键设计决策总结

| 决策 | 原因 |
|---|---|
| 不修改现有表，只新增 `knowledge_asset` | 不破坏现有 `asset_knowledge` 和 `knowledge_tree` 功能 |
| 使用 `GENERATED ALWAYS AS IDENTITY` | 比 `next_id()` snowflake 更简洁，与现有风格不同但是新表的独立选择 |
| 存量数据不动 | 旧 `asset_knowledge` 数据保留，通过 `tree_node_id` 挂载到知识树 |
| 文件上传复用现有 S3 分片能力 | `s3.rs` + `upload.rs` 已实现完整分片上传，新增一个绑定接口即可 |
| MDXEditor 选型 | React + TS + Prosemirror，支持 WYSIWYG/Markdown 双模式 |