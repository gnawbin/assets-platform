# Rust原生多模态解析 + pgvector多模态RAG 设计文档（V1.7 整合版）

> **本文档为唯一权威版本**，由以下内容的最终整合：
> 1. V1.7 落地实现（pgvector 唯一权威主存储、进程内 moka 缓存）
> 2. V1.8 未来规划（Valkey 分布式缓存、LanceDB 热层、TEI 图像向量）
> 3. 既有文档《文档解析后向量化数据缓存方案.md》冲突消解（LanceDB 主存储结论作废，仅复用缓存设计思想）
>
> **明确排除**：❌ SQLite（全平台统一 PostgreSQL）；❌ SurrealDB（本文档存储体系完全不引入 SurrealDB）。
>
> 目标：可施工、可编译、可验收；文档与代码实现一一对应。

---

## 目录
1. [版本边界与未来规划总览](#1-版本边界与未来规划总览)
2. [核心约束与前提假设](#2-核心约束与前提假设)
3. [系统整体架构](#3-系统整体架构)
4. [数据模型与DDL设计](#4-数据模型与ddl设计)
5. [业务完整数据流](#5-业务完整数据流)
6. [关键模块详细设计](#6-关键模块详细设计)
7. [缓存体系设计（V1.7进程内 + V1.8分布式）](#7-缓存体系设计)
8. [V1.8未来规划（Valkey / LanceDB / TEI）](#8-v18未来规划)
9. [部署形态、配置体系](#9-部署形态配置体系)
10. [迁移、灰度、回滚方案](#10-迁移灰度回滚方案)
11. [POC验证清单（前置实验，不改动生产代码）](#11-poc验证清单)
12. [待决策清单](#12-待决策清单)
13. [放行验收标准](#13-放行验收标准)
14. [依赖、许可证、风险清单](#14-依赖许可证风险清单)
15. [附录：既有文档冲突处理说明](#15-附录既有文档冲突处理说明)

---

# 1. 版本边界与未来规划总览

## 1.1 版本路线图
| 版本 | 范围 | 存储体系 | 缓存体系 |
|------|------|----------|----------|
| **V1.7（当前落地）** | 解析、上传对接、增量ETL、向量检索、进程内缓存 | **pgvector 唯一权威主存储** | **进程内 moka 缓存** |
| **V1.8（未来规划）** | 分布式热层、图像向量、冷热分层 | pgvector 冷库 + **LanceDB 热层（可选）** | **Valkey 分布式缓存** |

> ⚠️ V1.8 是否上热层 / 分布式缓存，**必须依赖 POC-6 真实数据决策**，不得臆断。见 §11 与 §12 D7。

## 1.2 存储体系边界（强制铁律）
| 模块 | 存储内容 | 生命周期 | 是否允许丢失 |
|------|----------|----------|--------------|
| 会话记忆 | 对话消息、工具调用记录 | 分钟-小时 TTL | 可以丢失；**严禁存入向量库，不参与相似度检索** |
| 向量冷库（V1.7主存储） | 全量 chunk、向量、过滤元数据 | 跟随文档生命周期 | **绝对不允许丢失** |
| 向量热层（V1.8可选） | 高频访问向量副本 | 热度驱动 | 允许丢失，降级仅性能变差 |
| 业务库 PG | doc_meta、file_uploads、元数据 | 永久 | 不允许丢失 |

> 关键定义：**热数据 ≠ 短期数据**；热数据必须同时满足：访问频率高 + 可能变更 + 延迟敏感 + 需要长期保存。会话记忆是短期数据，不属于热向量数据。

## 1.3 现存系统三大核心问题（基线现状）
1. **向量检索完全未实现**：`RAGRetriever::retrieve()` 返回固定 `0.0 AS similarity`，按ID排序，不使用用户问题；`chunk_and_vectorize` 不生成向量，INSERT 不写 embedding 字段。
2. **多套向量维度混乱**：数据库 `document_chunk.embedding vector(1536)`；Python 解析侧 `EMBEDDING_DIM=512`；V1.6 文档写768、需求文档写1024，维度不一致直接运行报错。
3. **解析输出扁平化丢失结构信息**：`ParseResult` 只有扁平文本，无标题层级、页码、时间轴结构；无法生成稳定 `chunk_id`；上传链路与解析链路无契约定义；仓库并存两套冲突存储架构文档。

## 1.4 整改分类统计
| 类别 | 数量 | 是否阻断放行 |
|------|------|------------|
| 阻断级B（功能不可实现） | 9 | ✅是 |
| 数据模型M缺陷 | 12 | ✅是 |
| 流程正确性F缺陷 | 8 | ✅是 |
| 依赖构建D缺陷 | 11 | ✅是 |
| 集成冲突C缺陷 | 7 | ✅是 |
| 结构性缺失G（可TODO到V1.7.1/V1.8） | 10 | ❌否 |

---

# 2. 核心约束与前提假设

1. **多租户模型：schema-per-tenant**
   业务数据表全部存放在租户独立 schema；`runledger` 调度表存独立 `runledger` 公共 schema；禁止行级 `tenant_id TEXT` 做隔离，仅做审计冗余字段。

2. **存储选型：统一 PostgreSQL，排除 SQLite 与 SurrealDB**
   - 服务端 Docker 与 Tauri 桌面端 **统一使用 PostgreSQL**（桌面端本地跑便携 PostgreSQL 实例）。
   - **不使用 SQLite**：不维护两套 SQL 方言、两套 DDL、两套 SQL 执行逻辑。
   - **不使用 SurrealDB**：本文档存储体系完全基于 PostgreSQL + pgvector，不引入 SurrealDB。

3. **向量扩展版本约束**
   PostgreSQL ≥14；**pgvector 扩展 ≥0.5.0（支持HNSW），生产推荐 ≥0.7.0**；Rust crate `pgvector = "0.4"`（crate 版本与 pgvector 扩展版本为两套独立版本号）。

4. **推理栈约束**
   OCR 统一使用 `pdf_oxide/ocr-tract` 纯 Rust tract-onnx；**禁止 ORT 多版本并存**；`ort` 依赖全部移除。

5. **ETL 流水线固定顺序（不可变更）**
   `解析 → 结构化分块 → diff双键比对 → 仅变更块执行Embedding → 写入向量库 → 校验放行`
   > 必须 diff 之后再 embedding，实现省算力增量更新；禁止全量分块后全部调用 Embedding。

---

# 3. 系统整体架构

## 3.1 架构图
```
文件上传（S3 presign直传）
        ↓
file_uploads对象存储元数据
        ↓【complete完成，投机解析触发】
文档解析流水线（PDF/DOCX/图片/音视频）
        ↓输出ParseResult(含blocks结构化块)
ChunkAssembler生成chunk_id、logical_path
        ↓diff（对比存量rag_chunk，区分新增/修改/不变/删除）
        ↓仅新增、修改的chunk调用Embedding生成向量
        ↓写入pgvector(rag_chunk)生成staged待放行版本
        ↓【用户commit确认】一次性切换active_doc_version对外生效

检索链路：
用户query → 规范化
        ↓【V1.7进程内moka缓存查向量】
        ↓Embedding生成查询向量
        ↓pgvector HNSW向量检索（过采样*4）
        ↓业务层过滤min_similarity、max_tokens预算截断
        ↓返回结果；异步批量更新文档热度统计（禁止检索路径UPDATE）
        ↓【V1.8】进程缓存未命中 → Valkey分布式缓存 → pgvector

后台辅助任务：
1.解析消费worker；2.purge_staged垃圾回收；3.consistency_check一致性对账；4.index_maintenance索引维护
```

## 3.2 两大部署形态
| 能力 | 服务端 Docker | Tauri 桌面端 |
|------|--------------|--------------|
| runledger DAG调度 | ✅支持 | ❌禁用，使用 `SimpleLocalOrchestrator` |
| Patroni/PGBouncer只读副本 | ✅支持 | ❌不需要 |
| 独立cron对账进程 | ✅ | ❌仅启动轻量校验 |
| OCR/ASR并发上限 | CPU核数限制 | 固定1-2，考虑笔记本散热 |
| 数据库 | PostgreSQL | **本地便携 PostgreSQL（不支持SQLite）** |
| 缓存 | 进程内 moka（V1.7）→ Valkey（V1.8多实例） | 进程内 moka（V1.7），单实例无需分布式 |

## 3.3 三大组件职责边界
| 组件 | 职责 | 不负责 |
|------|------|--------|
| adk-graph | 交互式Agent、会话工具调用（用户可见秒级） | 后台大数据ETL任务、孤儿任务回收 |
| bpmninja | 业务审批流 | 文档解析向量加工流水线 |
| runledger（可选） | 后台批处理ETL，lease租约、DLQ、cron调度 | 用户交互、审批流程 |

> 若 runledger 带来双 sqlx 编译代价不可接受，则扶正 `SimpleLocalOrchestrator` 作为唯一实现。

---

# 4. 数据模型与DDL设计

> 全部 DDL 放置 `crates/database/src/sql/rag_migration.sql`；使用 `{schema}` 作为租户 schema 占位符；全部语句幂等 `IF NOT EXISTS`；新增迁移函数 `migrate_tenant_rag_tables` 挂数据库初始化流程。

## 4.1 doc_meta：文档主元数据（RAG体系根表）
```sql
CREATE TABLE IF NOT EXISTS {schema}.doc_meta (
    doc_id                  TEXT PRIMARY KEY,              -- sha256带长度前缀哈希，消除拼接歧义
    tenant_id               BIGINT,                        -- 仅审计，隔离靠schema
    source_system           VARCHAR(64)  NOT NULL,
    source_doc_id           VARCHAR(512) NOT NULL,
    source_uri              TEXT,                          -- s3://bucket/key，不存http下载链接
    source_rev              BIGINT,                        -- 对应file_uploads.version源版本
    source_etag             VARCHAR(255),                  -- S3对象ETag指纹
    source_file_name        VARCHAR(512),
    source_file_type        VARCHAR(32),
    content_global_hash     CHAR(64),                      -- 全文规范化哈希，不变跳过全量处理
    doc_version             BIGINT NOT NULL DEFAULT 1,
    active_doc_version      BIGINT,                        -- 当前对外生效版本；检索只读取该版本
    staged_doc_version      BIGINT,                        -- 解析完成待放行版本；检索禁止读取
    status                  VARCHAR(20) NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','processing','staged','active','failed','deleting','deleted','expired')),
    expire_time             TIMESTAMPTZ,
    permission_json         JSONB NOT NULL DEFAULT '{}'::jsonb,
    embedding_model_version VARCHAR(64),
    chunk_strategy_version  VARCHAR(32),
    access_count            BIGINT NOT NULL DEFAULT 0,     --文档级热度统计，异步批量更新
    last_accessed_at        TIMESTAMPTZ,                   --最近命中时间，用于冷热判断
    last_error              TEXT,
    created_by              BIGINT,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_by              BIGINT,
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted                 SMALLINT NOT NULL DEFAULT 0,
    CONSTRAINT uk_doc_meta_source UNIQUE (source_system, source_doc_id)
);
--索引省略，详见原始文档
```
> 关键语义：
> 1. `status` 只代表对外可见性，**不代表处理进度**；处理进度由 `workflow_binding` 管理。
> 2. `staged_doc_version`：投机解析产出，**检索SQL严禁读取该字段**；commit 放行时一次性提升为 `active_doc_version`。
> 3. `access_count / last_accessed_at` **禁止在检索热路径执行 UPDATE**，仅后台异步批量累加。

## 4.2 rag_chunk：分片与向量表（支持版本共存，实现可回滚）
```sql
CREATE TABLE IF NOT EXISTS {schema}.rag_chunk (
    id                      BIGSERIAL PRIMARY KEY,
    chunk_id                TEXT NOT NULL,                  --逻辑位置ID sha256(doc_id+logical_path+local_index)，内容变更不改变
    doc_id                  TEXT NOT NULL REFERENCES {schema}.doc_meta(doc_id) ON DELETE CASCADE,
    doc_version             BIGINT NOT NULL,
    logical_path            TEXT NOT NULL DEFAULT '',       --标题层级锚点如"1/2/0"
    local_index             INT  NOT NULL DEFAULT 0,        --块内序号
    chunk_index             INT  NOT NULL,                  --文档全局序号，仅用于展示排序
    source_type             VARCHAR(16) NOT NULL CHECK (source_type IN ('text','pdf','document','image','audio','video')),
    text                    TEXT,
    image_path              TEXT,
    page_no                 INT,                            --页码
    file_name               VARCHAR(512),                   --冗余字段，避免检索路径JOIN doc_meta
    start_ms                BIGINT,                         --音视频时间，毫秒；废弃float浮点
    end_ms                  BIGINT,
    content_hash            CHAR(64) NOT NULL,              --规范化文本sha256；用于变更检测
    token_count             INT,                            --token统计
    char_len                INT,
    text_vector             vector(768),                    --文本向量维度，由sys_system_config全局配置管控
    image_vector            vector(512),                    --V1.7默认置NULL；V1.8可选开启（TEI图像向量）
    embedding_model_version VARCHAR(64) NOT NULL,
    chunk_strategy_version  VARCHAR(32) NOT NULL,
    is_deleted              BOOLEAN NOT NULL DEFAULT false,
    metadata                JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uk_rag_chunk_pos UNIQUE (chunk_id, doc_version)   --【核心】逻辑位置+版本联合唯一，支持版本共存回滚
);

--部分HNSW索引：已删除、向量NULL的数据不进入索引，规避pgvector后过滤塌陷B-7
CREATE INDEX IF NOT EXISTS idx_rag_chunk_text_vec
    ON {schema}.rag_chunk USING hnsw (text_vector vector_cosine_ops)
    WITH (m = 16, ef_construction = 64)
    WHERE text_vector IS NOT NULL AND is_deleted = false;
```

## 4.3 配套表
1. **tombstone 墓碑**：拦截落后旧版本任务回写；`(doc_id,event_version)` 唯一约束+索引。
2. **workflow_binding**：记录 ETL 任务运行状态；`trace_id` 链路追踪字段；**部分唯一索引保证同一文档同一版本同一工作流只能一个 running 实例**，作为 runledger 降级兜底。
3. **diff_repair_task**：对账差异工单；联合唯一主键避免重复生成修复工单。

## 4.4 存量表 document_chunk 修复（兼容历史数据）
- 增加唯一约束 `uk_chunk_asset_idx(asset_id,chunk_index)`，防止重复分片。
- 增加 `embedding_model_version` 字段记录向量模型版本。
- 补齐缺失的 HNSW 向量索引。

## 4.5 检索 SQL（唯一检索入口）
> 关键点：`ORDER BY` 直接使用 `text_vector <=> $1`，不要包 `1-()`，保证命中 HNSW 索引；业务层做**过采样*4**，然后做相似度阈值过滤、max_tokens 预算截断，记录 degraded 降级标记。
```sql
WITH RECURSIVE sub_tree AS (
    SELECT id FROM {schema}.knowledge_tree WHERE id = $1::BIGINT AND deleted = 0
    UNION ALL
    SELECT kt.id FROM {schema}.knowledge_tree kt
    JOIN sub_tree st ON kt.parent_id = st.id WHERE kt.deleted = 0
)
SELECT
    c.chunk_id, c.text, c.chunk_index, c.page_no, c.file_name, c.start_ms, c.end_ms,
    m.doc_id, m.status,
    1 - (c.text_vector <=> $2::vector) AS similarity
FROM {schema}.rag_chunk c
JOIN {schema}.doc_meta  m ON m.doc_id = c.doc_id
WHERE c.is_deleted = false
  AND c.text_vector IS NOT NULL
  AND m.status = 'active'
  AND m.deleted = 0
  AND m.active_doc_version = c.doc_version
  AND ($1::BIGINT IS NULL OR m.tree_node_id IN (SELECT id FROM sub_tree))
ORDER BY c.text_vector <=> $2::vector
LIMIT $3 * 4; --过采样4倍，业务层再过滤
```

---

# 5. 业务完整数据流

## 5.1 上传-对象存储-解析-放行完整时序（投机解析模式）
> 配置开关 `speculative_parse_enabled=true`；超过 `speculative_parse_max_bytes(50MB)` 自动关闭投机解析，等待 commit 后再解析。
```
1.upload_init → 创建file_uploads记录 status=pending
2.upload_start下发S3 presigned URL；浏览器分片直传，服务器不接收文件字节
3.upload_complete：完成分片上传；head_object校验对象完整性
    → step-1：UPSERT doc_meta记录；status=pending，推入解析队列
4.worker消费队列：
    claim版本CAS（step0，**不修改status**，避免换版检索中断）
    → download_object【流式落盘，禁止全部读入内存，解决B-9 OOM】
    → DocParser解析输出ParseResult（新增blocks结构化块）
    → ChunkAssembler生成chunk_id、logical_path
    → diff双键比对：(chunk_id,content_hash)区分新增/修改/不变/删除
    → 仅新增、修改chunk调用Embedding
    → upsert写入rag_chunk，版本门控防止旧任务覆盖新版本F-2
    → step6e：写入staged_doc_version；新文档status=staged；换版文档status保持active
5.upload_commit 用户确认保存【事务内执行step6f放行】
    → 将staged_doc_version提升为active_doc_version；staged置NULL；status=active
    → 检索立刻可以访问新版本
```
### 三种终局
1. **放行成功**：commit 触发切换 active_doc_version，对外可见。
2. **解析失败**：新文档置 status=failed；**换版场景 status 保持 active，旧版本继续对外提供检索**。
3. **放弃保存**（用户关闭页面不commit）：定时任务 `purge_staged`（默认24h）回收 staged 版本、S3 对象、数据库记录。

> 禁止方案：解析放在 commit 之后；会导致用户点击保存需要等待 OCR 大文件，阻塞前端。

## 5.2 写入更新流程
1. claim 做 doc_version CAS 乐观锁；`workflow_binding` 做并发互斥兜底。
2. diff 比对 `(chunk_id, content_hash)`，不变 chunk 跳过 embedding。
3. rag_chunk upsert，WHERE 条件限制 `doc_version`，拒绝旧版本覆盖新数据。
4. 更新 staged_doc_version 等待 commit 放行。

## 5.3 删除流程
1. 开启数据库事务：更新 doc_meta 状态为 deleting；批量标记 rag_chunk.is_deleted=true。
2. 事务提交后，写入 tombstone 墓碑，用于拦截迟到任务；后续定时清理资源。

## 5.4 一致性校验对账流程
1. **源侧必须读取 PG 主库**；禁止直接读从库避免复制延迟产生假差异 F-5。
2. 记录快照水位，水位漂移过大标记本次对账结果不可靠，不生成工单。
3. LEFT JOIN 比对 `doc_meta` 与 `rag_chunk`，缺失生成 diff_repair_task 修复工单。
4. 抽样重算向量，校验向量内容一致性。

---

# 6. 关键模块详细设计

## 6.1 文档解析模块
### ParseResult 扩展（B-1）
新增 `blocks` 结构化块，向后兼容，老数据自动为空；Python 解析侧同步修改 Pydantic 模型，保证 local/remote 解析输出完全一致。
```rust
pub struct ParseResult {
    pub file_name: String,
    pub file_type: String,
    pub raw_text: String,
    pub images: Vec<String>,
    pub metadata: serde_json::Value,
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    pub blocks: Vec<Block>, //新增结构化块
}
pub struct Block {
    pub heading_path: String,
    pub local_index: u32,
    pub block_type: String,
    pub page_no: Option<i32>,
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    pub text: String,
    pub images: Vec<String>,
}
```
> `ChunkAssembler` 根据 blocks 计算 `chunk_id = sha256(doc_id + logical_path + local_index)`；blocks 为空降级使用 `chunk_index` 兜底，记录元数据 `id_source="fallback_index"`。

### 分块器二选一（D5待决策）
1. **方案①推荐**：`text-splitter 0.33` 替换自研 TextChunker；使用真实 tokenizer，修复 token 估算 bug、字节切片 panic 风险。
2. **方案②保留自研**：重写 `estimate_tokens`、修复按字节切片越界 panic。

> 分块策略变更更新 `chunk_strategy_version`，触发重解析。

## 6.2 Embedding模块
1. **全局统一维度管控**：向量维度读取系统配置表 `sys_system_config`；代码、DDL、Python 侧**禁止硬编码向量维度** B-2。
2. nomic-embed-text 必须携带前缀：文档 `search_document: `；query `search_query: `。
3. V1.7 默认砍掉图像向量 image_vector；图片内容通过 OCR 提取文本送入文本向量；如需图像向量 V1.8 引入独立 TEI 服务 B-8（见 §8.3）。
4. 使用 `futures::stream::buffer_unordered` 做并发限流；不引入 rxrust 依赖 D-7。

## 6.3 检索模块
1. 检索结果返回真实余弦相似度，修复原代码固定 `0.0` 问题 B-3。
2. 过采样*4 兜底 HNSW 后过滤召回塌陷；可选 pgvector≥0.8 开启 `hnsw.iterative_scan`。
3. `max_tokens` 预算：业务层累加 token_count，达到阈值截断结果，不能单纯依赖 LIMIT。
4. 热度统计**异步批量更新** doc_meta.access_count，检索路径禁止 UPDATE 数据库。

## 6.4 调度模块
两种调度实现：
1. `RunledgerOrchestrator`：分布式后台 DAG 任务；注意 runledger 依赖旧版 sqlx，会产生两套 sqlx 二进制，需要评估体积编译时间 D-5。
2. `SimpleLocalOrchestrator` 单机降级方案；V1.7 必须完整支持；关闭 runledger feature 可直接切换。
> 禁止 trait 签名中暴露 sqlx 类型，避免两套 sqlx 类型冲突。

---

# 7. 缓存体系设计

## 7.1 缓存分层总设计（V1.7 vs V1.8 边界）
| 层级 | V1.7实现范围 | V1.8（POC-6通过后才落地） | 数据来源 | 降级保障 |
|------|-------------|--------------------------|----------|----------|
| 进程内内存缓存（单实例 moka） | ✅完整实现 | 保留，作为上层热点 | PG 数据库回写回填 | 重启全部失效；降级直接查询 PG，功能无损，仅延迟上升 |
| Valkey 分布式缓存（集群多实例共享） | ❌不实现 | ✅实现 | PG 数据库回写回填 | Valkey 挂掉直接穿透到 PG，业务不故障 |
| pgvector（向量冷库/权威源） | ✅唯一权威存储 | ✅不变 | ETL 写入 | —— 所有缓存全部是加速器，**绝对不能作为数据源** |

> ⚠️核心铁律：
> 1. **所有缓存均可随时清空，系统必须正常工作，只是性能下降；缓存不承担正确性，PG 才是事实源。**
> 2. 缓存**不存储完整 chunk 大文本**，只缓存：查询向量、检索命中元组 `(chunk_id,score)`；需要正文时，拿 `chunk_id` 回查 PG。
> 3. 会话记忆独立使用 `sess:` 前缀；向量缓存统一 `rag:` 前缀；**严禁混用前缀，防止 FLUSHDB 误删向量缓存/会话**。

## 7.2 V1.7 进程内 moka 缓存详细设计

### 7.2.1 选型
- 使用 `moka` 作为进程 LRU 缓存库（V1.7 引入；**仍禁止 lancedb / valkey，V1.8 才引入**）。
- moka 优势：Segmented-LRU(SLRU)、支持 TTI 空闲过期、TTL 绝对过期、淘汰事件、统计指标、tokio 异步友好。

```toml
moka = { version = "0.12.16", features = ["future"] }
```

### 7.2.2 缓存 Key 设计（七元组约束，修复旧文档缺陷）
> 旧文档缺陷：缺少租户 schema、embedding 版本、分块策略版本、权限指纹，会出现跨租户泄露、换模型返回脏缓存。长字符串用 sha256 摘要缩短 key。

```
# 1、查询向量缓存【可跨租户共享！仅与query文本+embedding模型有关，不含业务租户数据】
rag:qvec:{embedding_model_version}:{sha256(normalized_query_text)}
value: Arc<Vec<f32>>

# 2、检索命中结果缓存【不可跨租户，强绑定租户、版本、权限】
rag:hits:{schema}:{embedding_model_version}:{chunk_strategy_version}:{top_k}:{perm_hash}:{sha256(normalized_query_text)}
value: Vec<CachedHit>  # 只存 (chunk_id, score)，不存完整文本
# perm_hash：sha256(排序后的可访问doc_id集合 + permission_level集合)，用户权限变更，hash自动变化，旧key自然失效

# 会话记忆完全隔离，前缀sess:
sess:session:{session_id}
```

#### 缓存 TTL/TTI 配置（`.env.example.toml [rag]`）
```toml
local_cache_enable = true
local_cache_max_entries = 2000
local_cache_ttl_sec = 60
local_cache_tti_sec = 300
```

### 7.2.3 moka 存储向量（Vec<f32>）的可行性
> **moka 原生可以存储 `Vec<f32>`**，value 约束 `V: Clone + Send + Sync + 'static`，`Vec<f32>` 完全满足，编译直接通过，无需序列化，内存直接存 Rust 对象。

**⚠️ 关键坑点：`get()` 每次命中会完整 Clone 整个 Vec<f32>。**
- 768 维 `Vec<f32>`：768 * 4 字节 = **3072 字节（~3KB）**，单次 clone 很小；1024/1536 维会变大，高 QPS 下产生内存与 CPU 压力。

**优化一：包一层 Arc（推荐）**，clone 仅拷贝指针：
```rust
use std::sync::Arc;
let cache: Cache<String, Arc<Vec<f32>>> = Cache::builder().max_capacity(1000).build_async();
let vec = Arc::new(vec![0.1_f32; 768]);
cache.insert("key".to_string(), vec.clone()).await;
let hit: Option<Arc<Vec<f32>>> = cache.get("key").await;
```

**优化二：使用 weigher 按字节权重淘汰**（不能只按条目数）：
```rust
let cache = Cache::builder()
    .max_capacity(5000)
    .weigher(|_key, value: &Arc<Vec<f32>>| -> u32 { (value.len() * 4) as u32 })
    .max_weighted_capacity(8 * 1024 * 1024)  // 总内存上限8MB
    .build_async();
```

### 7.2.4 核心 Rust 数据结构（V1.7）
```rust
use moka::future::Cache;
use std::sync::Arc;
use std::time::Duration;
use sha2::{Sha256, Digest};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedHit {
    pub chunk_id: String,
    pub score: f32,
}

#[derive(Clone)]
pub struct LocalVectorCache {
    /// 查询向量缓存 key:rag:qvec:* ；Arc包裹避免clone向量
    qvec_cache: Cache<String, Arc<Vec<f32>>>,
    /// 检索命中结果缓存 key:rag:hits:*，结构体小，不需要Arc
    hits_cache: Cache<String, Vec<CachedHit>>,
}

impl LocalVectorCache {
    pub fn new(cfg: &crate::config::RagConfig) -> Self {
        let qvec_cache = Cache::builder()
            .max_capacity(cfg.local_cache_max_entries)
            .max_weighted_capacity(8*1024*1024) //总内存上限8MB
            .weigher(|_k, v:&Arc<Vec<f32>>| (v.len() * std::mem::size_of::<f32>()) as u32)
            .time_to_live(Duration::from_secs(cfg.local_cache_ttl_sec))
            .time_to_idle(Duration::from_secs(cfg.local_cache_tti_sec))
            .eviction_listener(|k,_v,cause|{ tracing::debug!(key=%k, ?cause, "local qvec cache evict item"); })
            .build_async();

        let hits_cache = Cache::builder()
            .max_capacity(cfg.local_cache_max_entries / 2)
            .time_to_live(Duration::from_secs(cfg.local_cache_ttl_sec))
            .time_to_idle(Duration::from_secs(cfg.local_cache_tti_sec))
            .eviction_listener(|k,_v,cause|{ tracing::debug!(key=%k, ?cause, "local hits cache evict item"); })
            .build_async();

        Self { qvec_cache, hits_cache }
    }

    fn sha256_digest(s: &str) -> String { hex::encode(Sha256::digest(s.as_bytes())) }

    pub async fn get_query_vector(&self, model_ver:&str, query:&str) -> Option<Arc<Vec<f32>>> {
        let key = format!("rag:qvec:{}:{}", model_ver, Self::sha256_digest(query));
        self.qvec_cache.get(&key).await
    }
    pub async fn put_query_vector(&self, model_ver:&str, query:&str, vec:Arc<Vec<f32>>) {
        let key = format!("rag:qvec:{}:{}", model_ver, Self::sha256_digest(query));
        self.qvec_cache.insert(key, vec).await;
    }

    pub async fn get_hits(&self, schema:&str, emb_ver:&str, chunk_strat_ver:&str, top_k:usize, perm_hash:&str, query:&str) -> Option<Vec<CachedHit>> {
        let key = format!("rag:hits:{}:{}:{}:{}:{}:{}", schema, emb_ver, chunk_strat_ver, top_k, perm_hash, Self::sha256_digest(query));
        self.hits_cache.get(&key).await
    }
    pub async fn put_hits(&self, schema:&str, emb_ver:&str, chunk_strat_ver:&str, top_k:usize, perm_hash:&str, query:&str, hits:Vec<CachedHit>) {
        let key = format!("rag:hits:{}:{}:{}:{}:{}:{}", schema, emb_ver, chunk_strat_ver, top_k, perm_hash, Self::sha256_digest(query));
        self.hits_cache.insert(key, hits).await;
    }

    pub async fn clear_all(&self) {
        self.qvec_cache.invalidate_all().await;
        self.hits_cache.invalidate_all().await;
    }
}
```

### 7.2.5 查询完整执行链路（V1.7，加入缓存逻辑）
```
用户原始query文本
    ↓
1. 规范化query文本（去除多余空白）
    ↓
2. 【读本地缓存】生成key尝试读取缓存查询向量
    ├─✅命中 → 直接复用向量
    └─❌未命中 → 调用Embedding模型生成query向量 → put写入本地缓存
    ↓
3. 生成perm_hash（基于当前用户可访问资源集合）
4. 【读本地缓存】读取hits检索结果缓存
    ├─✅命中：拿到Vec<CachedHit>（chunk_id+score）
    │       → 使用chunk_id回表查询PG rag_chunk获取完整业务数据
    │       → 做业务过滤min_similarity、max_tokens预算截断，返回上层
    └─❌未命中：访问pgvector执行向量检索
            → 将(chunk_id,score)写入本地hits缓存
            → 业务过滤、token截断返回上层
```
> 重点：**即使缓存命中，依然要回 PG 拿 chunk 正文**；杜绝缓存存储大文本带来的数据不一致风险。

### 7.2.6 缓存失效策略（V1.7 进程内）
> V1.7 是单机进程缓存；进程缓存**无法做到精准单 key 删除（多实例无解）**，所以不做 doc_id 粒度主动失效；依靠 TTL+TTI 过期 + perm_hash 变更 + embedding_version 变更自动隔离脏缓存。

| 变更场景 | 缓存处理策略（V1.7） | 说明 |
|---|---|---|
| 文档内容更新 / 版本升级 | 不主动删除；依靠 TTL 自动过期；key 携带版本信息，变化自动生成新 key | V1.8 引入 Valkey 后实现按 schema 前缀批量删除 |
| 用户权限变更 | perm_hash 变化生成全新 hits key，旧 key 自然过期 | 不需遍历删除该用户缓存 |
| Embedding 模型升级 | 修改 `embedding_model_version`，旧 rag:* key 全部失效；可调用 `clear_all()` | 建议重启或 clear_all |
| 分块策略版本升级 | 修改 `chunk_strategy_version`，旧缓存自动失效 | 同上 |
| 开关关闭缓存 | `local_cache_enable=false`，所有读写直接跳过缓存层 | 用于压测、故障排查 |

---

# 8. V1.8未来规划

> 本节全部为**规划**，V1.7 不编码。**触发条件**：POC-6 验证 pgvector 延迟偏高、访问符合二八流量偏斜，才启动开发。任一规划落地前必须复核 POC 数据，禁止直接照搬旧文档版本号。

## 8.1 Valkey 分布式缓存（V1.8）

### 8.1.1 定位
解决多实例部署场景进程缓存不共享导致的命中率分裂问题；提供多实例共享的二级热点缓存。

### 8.1.2 Key 规范
- **完全复用 V1.7 key 规范**（`rag:qvec:*`、`rag:hits:*`）。
- V1.7 进程缓存代码作为上层抽象，底层可切换 moka/valkey 实现，**面向 trait 编程**。

### 8.1.3 分层链路（V1.8）
```
用户Query
    ↓
1.进程内moka缓存（一级热点，速度最快）
    ├命中 → 直接返回
    └未命中 → 查询Valkey分布式缓存（二级共享缓存）
        ├✅命中 → 回填进程缓存，返回
        └❌未命中 → 查询pgvector；结果同时回填Valkey + moka
```

### 8.1.4 失效增强
1. 支持按 `{schema}` 租户前缀批量失效（`SCAN + DEL`，运维接口；**禁止 FLUSHDB**，防止误删 `sess:*` 会话数据）。
2. **依然不缓存完整 chunk 文本，只缓存 chunk_id+score**。
3. Valkey 挂掉：全部降级穿透 PG，业务不雪崩。
4. 向量 `Arc<Vec<f32>>` 需序列化为二进制字节数组存入 Valkey（V1.7 moka 本地内存无需序列化）。

### 8.1.5 配置继承
TTL 配置继承 V1.7 原有配置；新增 `hot_tier_enabled`、`hot_tier_ttl_sec`、`demote_after_days`（见 §9）。

## 8.2 LanceDB 热层（V1.8，可选）

### 8.2.1 定位与边界
- LanceDB 仅作为**热层**（高频访问向量副本），**绝不作主存储**。
- pgvector 永远是唯一权威冷库；LanceDB 允许全部丢失，可从 pgvector 重建。
- V1.7 架构**不引入 lancedb**；V1.8 依据 POC-6 决定。

### 8.2.2 何时启用（决策标准）
- POC-6 显示 pgvector P95 延迟偏高，且访问流量二八偏斜显著。
- 命中率稳定大于 60%，进程缓存收益显著，评估再加 LanceDB 热层。

### 8.2.3 设计要点（规划）
- 数据来源：从 pgvector 冷库按热度（access_count / last_accessed_at）回填热层。
- 冷热分层：热层存高频 chunk 向量副本；冷数据保留在 pgvector。
- 一致性：LanceDB 热层无权威性，变更后异步重建；不阻塞主流程。
- 依赖：V1.8 使用 crates.io 最新版 lancedb，**禁止直接复制旧文档版本号**。

## 8.3 TEI 图像向量服务（V1.8，可选）
> V1.7 默认砍掉 image_vector，图片内容通过 OCR 提取文本送入文本向量。若业务确需图像语义向量：
- 引入独立 TEI（Text Embeddings Inference）服务提供 CLIP 等图像向量。
- 决定项见 D3（§12）：①本地 ONNX CLIP；②独立 TEI 服务。建议 ②独立 TEI 服务。

---

# 9. 部署形态、配置体系

## 9.1 配置落点约定 C-4
- 配置统一放在 `.env.example.toml`；通过环境变量读取；**不自行解析原始 toml**。
- PG 数据库配置复用 `PG_HOST/PG_PORT` 等环境变量，**不使用数据库 URL 字符串**。
- 所有新增配置项示例写入 `.env.example.toml`。

### [rag] 核心配置
```toml
# 投机解析
speculative_parse_enabled = true
speculative_parse_max_bytes = 52428800
purge_staged_after_hours = 24

# 进程内本地缓存（V1.7，仅进程内存，无分布式）
local_cache_enable = true
local_cache_max_entries = 2000
local_cache_ttl_sec = 60
local_cache_tti_sec = 300

# 热层开关：V1.7固定false；V1.8根据POC-6开启
hot_tier_enabled = false
hot_tier_ttl_sec = 86400
demote_after_days = 7

# 会话记忆（与向量完全独立）
session_memory_ttl_sec = 1800

# 索引维护
index_rebuild_growth_ratio = 1.5

# 分块参数
chunk_size_tokens = 512
chunk_overlap_tokens = 64
parse_timeout_sec = 1200
```

### [db] Tauri 桌面端 PostgreSQL 本地实例配置
```toml
## Tauri桌面端 — PostgreSQL本地实例配置（无SQLite分支）
[db]
pg_host = "127.0.0.1"
pg_port = 5432
pg_database = "asset_platform_desktop"
pg_username = "xxx"
pg_password = "xxx"
```

## 9.2 Cargo.toml 关键依赖约束
1. `pgvector = { version = "0.4", features = ["sqlx"] }`；sqlx 约束 `>=0.8,<0.10`。
2. V1.7 允许 moka（进程缓存）；**禁止引入 lancedb / valkey / redis**；V1.8 依据决策引入 lancedb + valkey。
3. 删除 `ort`、`datafusion`、`rxrust` 无效依赖。
4. opentelemetry 版本与 lock 文件对齐 `0.32`，禁止引入旧版本产生两套 otel。
5. `ffmpeg-sidecar` 标记为 optional feature `media`，默认关闭，减少编译体积。

---

# 10. 迁移、灰度回滚方案

## 10.1 向量维度迁移三步法（例如1536迁移768）
不停机在线迁移：
1. 新增目标维度列；业务层双写新旧两列。
2. 后台分批回填向量。
3. 短事务内 DROP 旧列，RENAME 新列为正式列；重建 HNSW 索引。

## 10.2 回滚策略
1. 功能开关 `RAG_USE_VECTOR=false` 直接切回旧 document_chunk 检索链路，无需删表。
2. rag_chunk 支持版本共存；文档回滚直接修改 `doc_meta.active_doc_version = 旧版本号`。
3. DDL 全部使用 `IF NOT EXISTS`；不会破坏已有数据。

---

# 11. POC验证清单

> 必须完成 POC 再进入正式开发，消除设计假设风险。合计约 2 人日。全部 POC 只针对 PostgreSQL（无 SQLite 实验）。
1. **POC-1 pgvector-rust+sqlx 兼容性**：验证编译、向量编解码；确认不会产生多份 sqlx。
2. **POC-2 tract-onnx OCR 性能**：测试扫描 PDF 单页耗时，确定超时阈值 G-3。
3. **POC-3 runledger 双 sqlx 代价测量**：确认二进制体积、编译时间增量，用于 D4 决策。
4. **POC-4 离线构建验证**：测试 parser asr 特性在离线 CI 是否可以编译，评估 sherpa-rs 风险。
5. **POC-5 S3 大文件流式下载**：1.5GB 对象，峰值 RSS 内存增量 <100MB，验证修复 B-9 是否生效。
6. **POC-6 pgvector 性能与访问偏斜统计**：100 万 chunk 测试 P50/P95 检索延迟；统计访问二八偏斜，**决定 V1.8 是否需要热层缓存（Valkey / LanceDB）D7**。

---

# 12. 待决策清单

| 编号 | 决策点 | 选项 | 建议 |
|------|--------|------|------|
| D2 | 表策略：rag_chunk 与 document_chunk | A改造旧表 / B双表并存+统一视图 / C全量替换 | **B：灰度可回滚，V1.7DDL按B编写** |
| D3 | 是否保留 image_vector 图像向量 | ①砍掉，OCR+VLM文本；②本地ONNX CLIP；③独立TEI服务 | ①V1.7砍掉；V1.8如需图像向量选③TEI |
| D4 | runledger 是否转正 | ①双实现保留；②只保留 SimpleLocalOrchestrator | 先执行 POC-3，看双 sqlx 代价 |
| D5 | 分块器 | ①text-splitter替换自研；②修复原有自研 | ①推荐，彻底解决 token 估算 bug |
| D6 | 桌面端存储 | **①桌面端也使用 PostgreSQL**；②SQLite（废弃） | **选定①：统一 PostgreSQL，放弃 SQLite** |
| D7 | 检索热层 / 分布式缓存 | A V1.7不做，V1.8依据POC-6结果；B V1.7直接引入；C引入LanceDB热层 | **A：V1.7只上进程内 moka，V1.8 依据 POC-6 实测再上 Valkey / LanceDB，避免双写一致性风险** |

---

# 13. 放行验收标准

分为文档完整性、编译、数据正确性、可观测降级、上传解析链路验收、存储分层边界验收、缓存/旧文档冲突验收。

> 核心关键点摘录：
1. **编译**：无重复 major 版本依赖；`cargo tree -d` 无非法重复包。
2. **数据**：向量检索返回真实 similarity；HNSW 索引生效不走全表扫描；ETL 幂等，重复执行不产生重复 chunk。
3. **上传链路**：complete 未 commit，检索不到 staged 文档；换版解析进行中旧版本依然可检索；大文件内存不暴涨。
4. **边界**：会话记忆与向量库完全隔离，清空会话缓存不影响知识库检索。
5. **存储选型**：仓库中不存在 SurrealDB 相关代码/依赖；全平台仅 PostgreSQL。

### 缓存验收（V1.7）
- [ ] 开关 `local_cache_enable=false`，缓存全部跳过，全链路正常运行。
- [ ] 缓存命中场景，只读取 chunk_id，回 PG 拿完整 chunk 数据；缓存内部不存储完整文本。
- [ ] key 包含 `schema`、`embedding_model_version`、`chunk_strategy_version`；不同租户相同 query 生成不同 `rag:hits:*` key。
- [ ] perm_hash 变更（用户权限变化），旧缓存 key 不再命中。
- [ ] metrics 指标正常输出，可统计命中率。
- [ ] 重启进程缓存全部丢失，业务功能正常，性能下降但结果正确。
- [ ] `rag:` 与 `sess:` 两套前缀完全隔离。
- [ ] V1.7 Cargo.toml **只有 moka，没有 lancedb、valkey、redis**。
- [ ] moka 存储 `Arc<Vec<f32>>` 无重复 clone 向量问题；weigher 按字节限制内存上限。

### 缓存验收（V1.8 规划，暂不实现）
- [ ] moka 和 valkey 实现同一 trait，可配置切换底层实现。
- [ ] Valkey 宕机，自动降级穿透 pgvector，业务无报错。
- [ ] 支持按 schema 前缀批量清理向量缓存，不影响会话记忆。
- [ ] LanceDB 仅作热层，pgvector 保持唯一权威源；LanceDB 丢失可重建。

---

# 14. 依赖、许可证、风险清单

## 14.1 许可证
- `symphonia` MPL-2.0，需要在 NOTICE 文件声明。
- ffmpeg LGPL-2.1；media feature 默认关闭。
- CI 执行 `cargo deny check licenses` 校验许可证。

## 14.2 主要风险
1. HNSW 索引碎片化：高频增量写入，定期执行 `REINDEX CONCURRENTLY`。
2. ETL 消息丢失：不依赖 MQ 至少一次语义；依靠 doc_meta.status=pending 作为任务队列。
3. Embedding 模型升级：新旧向量不在同一语义空间，必须全量重建向量。
4. 解析崩溃风险：解析子进程隔离为 V1.8 目标；Windows 无 Landlock 沙箱。
5. 临时文件、S3 孤儿对象：RAII 临时目录；定时 purge_staged 回收孤儿资源。
6. **缓存一致性**：V1.7 进程缓存不做 doc_id 精准删除（多实例无解）；依赖 TTL/TTI + 版本 key 自动隔离。V1.8 引入 Valkey 后按 schema 前缀批量清理。
7. **热层双写一致性**：LanceDB 热层若启用，必须接受其非权威性；写入路径不做同步删除，由后台管理任务按需清理。

---

# 15. 附录：既有文档冲突处理说明

> 同仓已有文档《文档解析后向量化数据缓存方案.md》

## 冲突根源
旧文档：**LanceDB 为主存储，pgvector 仅归档兜底**；本文档：**pgvector 作为唯一权威主存储，LanceDB 仅作 V1.8 可选热层**。

## 处理策略
1. 旧文档文件保留；**文档头部增加取代声明：主存储 LanceDB 结论已被本文档取代**。
2. **可以复用内容**：Valkey 选型；缓存 key 分类思想；命中率预估基线；缓存工程纪律。
3. **必须修改**：写入顺序；缓存 key（增加 schema、perm_hash、doc_version、embedding_model_version 七元组）；一致性判断逻辑。
4. **必须否决**：LanceDB 做主存储；错误的 bytea 直接 cast 为 vector；把解析数据存入 asset_knowledge 表；只依靠 permission_level 过滤权限。
5. V1.7 禁止引入 `lancedb / valkey / redis`；V1.8 如需热层，重新评估 POC-6，使用 crates.io 最新版本，**禁止直接复制旧文档的版本号**。

---
