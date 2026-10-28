# Rust 原生多模态解析 + pgvector 多模态 RAG 设计文档 —— V1.7 整改清单

> **被评审对象**：`docs/Rust原生多模态解析 + pgvector多模态RAG系统设计文档（V1.6｜runledger DAG调度 · 评审整改版）.docx`（下文简称 **V1.6**，引用格式为 `§章节号`）
> **核对基线**：本仓库当前工作区代码（`apps/backend/src-tauri` 各 crate、`apps/doc-parser`、`crates/database/src/sql/*.sql`），以及 `crates.io` 实际元数据（2026-10-28 拉取）
> **输出目标**：把 V1.6 变成**可施工、可编译、可验收**的 V1.7。每条都是「原文 → 改为 → 理由 → 验收」，不写"建议加强"这类空话。
> **本清单只做文档整改，不含生产代码改动**；文中 DDL / Rust 片段是**设计交付物**，落地按 §8 的迁移步骤执行。

---

## 0. 结论速览

### 0.1 分类统计

| 类别 | 前缀 | 条数 | 是否阻断放行 |
|---|---|:--:|:--:|
| 阻断级（功能不可实现 / 数据直接错） | `B-` | 9 | **是** |
| 数据模型缺陷（DDL 不得入库） | `M-` | 12 | **是** |
| 流程正确性缺陷 | `F-` | 8 | **是** |
| 依赖与构建（`cargo build` 必须绿） | `D-` | 11 | **是** |
| 与现有代码的集成冲突 | `C-` | 7 | **是** |
| 结构性缺失（可带 TODO 进 V1.7.1） | `G-` | 10 | 否 |
| **合计** | | **57** | |

> **第二轮 / 第三轮 / 第四轮追加（10 条）**：`B-9`、`C-7`、`M-10`、`M-11`、`M-12`、`F-7`、`F-8`、`G-8`、`G-9`、`G-10` —— 来自三个此前完全空白的缺口：
> ① **上传 → 对象存储 → 解析的衔接**（`B-9 / M-10 / F-7 / F-8 / G-8`，交付物 **§7.7** + **POC-5**）：V1.6 **一次也没有提到上传链路**，而从解析器签名反推（`DocParser::parse_file(file_path: &str)`）可知，没有上传链路就没有解析的输入；
> ② **检索侧的存储分层**（`M-11 / G-9`，交付物 **§7.8** + **POC-6**）：V1.6 把「缓存」当成一个东西，导致**会话记忆 / 向量库冷热层 / 业务库**三者的边界从头到尾没有被定义过；
> ③ **缓存层落地路线**（`C-7` / `M-12` / `G-10`，交付物 **§7.9** + **§11.8**）：本仓库早已存在 `docs/文档解析后向量化数据缓存方案.md`，它把 **LanceDB 定为主存储、pgvector 定为归档**，与 §7.8「pgvector 唯一权威」正好相反；两份文档并存的结果不是"选一个"，而是被两个人各实现一半 —— 双主存储。

### 0.2 三条必须先看懂的结论

1. **V1.6 的"复用现有 RAG 链路"是不成立的假设。** 现有 `RAGRetriever::retrieve()` 里写的是 `0.0 AS similarity` + `ORDER BY dc.id`，**完全没有用 `params.question`**（`crates/service/src/rag_service.rs:98-121`）；`chunk_and_vectorize()` **从不调用任何 Embedding 接口**，`INSERT` 列清单里根本没有 `embedding`（`rag_service.rs:176-181`）。也就是说：本仓库目前**没有向量检索**，V1.6 §5.3 的"多路召回 / Rerank / 混合召回"全部是从零新建。
2. **维度对不上，而且是四套。** 数据库 `document_chunk.embedding vector(1536)`（`tenant_tables.sql:868`）、Python 侧车 `EMBEDDING_DIM=512`（`apps/doc-parser/config.py:32`）、V1.6 `text_vector vector(768)`（§5.4）、技术需求文档 `1024`。Python 侧把 512 维写进 1536 维列会直接 `expected 1536 dimensions, not 512`。这条链路现在是**坏的**，不是"待优化"。
3. **`chunk_id = sha256(doc_id + logical_path + local_index)` 在当前 `ParseResult` 上没有数据来源。** `ParseResult` 只有 `file_name / file_type / raw_text / images / metadata` 五个扁平字段（`crates/parser/src/types.rs:12-25`），`raw_text` 是一整段字符串，下游 `TextChunker` 按 `\n\n` 切（`rag_service.rs:38`）。**结构信息在切 chunk 之前就被抹平了**，`logical_path` 无从生成。

### 0.3 阅读约定

- 「原文」= 对 V1.6 的**逐字引用**（含章节号），方便在 docx 里 Ctrl+F 定位。
- 「改为」= 可直接粘贴进 V1.7 的替换文本 / DDL / 代码。
- 「理由」= 为什么必须改（引用代码行号或外部事实，不用"更规范"这类词）。
- 「验收」= 怎么证明改对了（命令 / SQL / 检查点）。
- 行号引用以本清单生成时的仓库内容为准；若代码已变更，以 `grep` / `Select-String` 结果为准。

---

## 1. 阻断级 P0（9 条）

### B-1｜`chunk_id` 方案 B 不可实现：`logical_path` 没有数据来源

- **原文**（§8.2）：`chunk_id = sha256(doc_id + logical_path + local_index)`；「`logical_path`：文档标题层级、段落编号、章节锚点等结构稳定标识」；「`logical_path` 维护规则：基于文档标题层级、段落编号等稳定结构生成」。
- **问题**：解析层输出契约是扁平的。`ParseResult`（`crates/parser/src/types.rs:12-25`）只有 `file_name / file_type / raw_text / images / metadata` 五个字段，`raw_text` 是单一字符串；`pdf.rs` 输出整段文本，`office.rs` 把表格拼成 `" | "` 并加 `【工作表: x】` 前缀。切分发生在下游 `TextChunker`（`rag_service.rs:29-71`），只按 `separator = "\n\n"` 顺序累积。**到切分这一步，标题层级信息已经不存在了。** 方案 B 唯一能拿到的 "logical_path" 只有 chunk 序号本身 —— 而这正是 V1.6 §8.2 自己否定过的「`doc_id`+`chunk_index` 的偏移问题」。
- **改为**：
  1. `ParseResult` **增量**增加结构块字段（向后兼容，见 C-1）：
     ```rust
     /// 结构化块（V1.7 新增；旧客户端反序列化时默认为空 → 退化为现有扁平行为）
     #[serde(default, skip_serializing_if = "Vec::is_empty")]
     pub blocks: Vec<Block>,
     ```
     ```rust
     /// 解析层输出的最小结构单元
     #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
     pub struct Block {
         /// 标题路径锚点，如 "1/2/0"；无标题结构时为空串
         #[serde(default)]
         pub heading_path: String,
         /// 该块在 heading_path 内的序号（方案 B 的 local_index）
         pub local_index: u32,
         /// 块类型：paragraph / table / heading / ocr_line / asr_segment / frame
         pub block_type: String,
         /// 页码（PDF/图片）；非分页格式为 None
         #[serde(default)]
         pub page_no: Option<i32>,
         /// 音视频时间轴（秒）
         #[serde(default)]
         pub start_sec: Option<f64>,
         #[serde(default)]
         pub end_sec: Option<f64>,
         /// 块文本
         pub text: String,
         /// 块内引用的图片/帧路径
         #[serde(default, skip_serializing_if = "Vec::is_empty")]
         pub images: Vec<String>,
     }
     ```
  2. **`chunk_id` 生成下沉到解析层之后的 `ChunkAssembler`**（建议新增 `crates/service/src/chunk_assembler.rs`），由它从 `blocks` 生成 `logical_path` 并计算 `chunk_id`；`rag_service` 只负责写库。
  3. 方案 B 的兜底：`blocks` 为空（老数据 / remote 侧车未升级 / 纯文本粘贴）时，**允许** `logical_path = ""`、`local_index = chunk_index`，并在 `metadata.id_source = "fallback_index"` 里记录，便于后续重算。
- **理由**：稳定 ID 的前提是"结构信息在切分之前就存在"。把 ID 生成放在 `TextChunker` 之后，等于要求下游猜结构；放在解析层（唯一知道 `H1/H2/页码/时间轴` 的地方）才可能成立。不补 `blocks`，§8.2 只能是纸面方案。
- **验收**：对一个 3 级标题的 docx 解析后，`blocks` 的 `heading_path` 序列形如 `1` → `1/0` → `1/1` → `1/1/0`；**在文档中间插入一个段落**后重跑，只有该段落所属 `heading_path` 下的 `chunk_id` 变化，其余 `chunk_id` 全部不变。这是方案 B 的唯一定义性检验。

---

### B-2｜四套向量维度并存，现有链路必然写入失败

- **原文**（§5.4 / §13.2）：`text_vector vector(768)`、`image_vector vector(512)`；§10 `text_vector_dim = 768`、`image_vector_dim = 512`。
- **现状核对**：

  | 位置 | 维度 | 证据 |
  |---|:--:|---|
  | 数据库 `document_chunk.embedding` | **1536** | `crates/database/src/sql/tenant_tables.sql:868` `embedding vector(1536)` |
  | Python 侧车 | **512** | `apps/doc-parser/config.py:31-32`（`bge-small-zh-v1.5` / `EMBEDDING_DIM=512`） |
  | V1.6 文本向量 | **768** | §5.4（`nomic-embed-text` v1.5 = 768） |
  | V1.6 图像向量 | **512** | §5.4（CLIP ViT-B/32 = 512） |
  | 技术需求文档 | **1024** | `docs/资产管理系统（固定资产+无形资产）技术需求文档.md:478` `bge-large-zh-v1.5` |

- **问题**：Python 侧车把 512 维写进 `vector(1536)` 列 → PostgreSQL 直接报 `expected 1536 dimensions, not 512`。这不只是"文档不一致"，是**当前代码路径就已经跑不通**。另外 `nomic-embed-text` 要求 `search_document:` / `search_query:` 前缀（V1.6 全文未提），不加前缀召回率会明显下降。
- **改为**：
  1. **单一事实源**：维度只允许从 `sys_system_config`（该表已存在，`knowledge_module_migration.sql` 里注释就写着"平台全局参数：RustFS地址/**向量维度**/文件白名单等"）读取 `vector_dim_text` / `vector_dim_image`；Rust、Python 侧车、DDL 三处**一律不允许自带默认值**。
  2. V1.7 锁定：`vector_dim_text = 768`（`nomic-embed-text` v1.5），`vector_dim_image = 512`（CLIP ViT-B/32，**仅当 B-8 决定保留图像向量时**）。
  3. `apps/doc-parser/config.py` 的 `EMBEDDING_DIM` 默认值改为从后端配置下发；`EMBEDDING_MODEL` 与后端 `embed_text_model` 必须同源，禁止两边各写一份。
  4. **必须**做前缀处理：`embed_document(t) = "search_document: " + t`，`embed_query(q) = "search_query: " + q`；前缀属于 `embedding_model_version` 的一部分（如 `nomic-embed-text-v1.5/prefixed`）。
  5. 启动期硬断言（挂在 `postgres.rs` 初始化链路里）：
     ```sql
     SELECT atttypmod FROM pg_attribute
      WHERE attrelid = '{schema}.rag_chunk'::regclass AND attname = 'text_vector';
     -- 期望值 = 768 + 4（pgvector 的 typmod 编码），不等则启动失败并打印实际值
     ```
- **理由**：维度不一致的表现是"要么直接报错，要么静默截断/全零向量"，后者更难查。跨语言三处各写一份默认值，漂移是必然事件而不是风险。
- **验收**：把 `sys_system_config.vector_dim_text` 改成 1024 后重启，初始化必须**明确失败**并提示"DDL 维度 768 ≠ 配置维度 1024，请执行迁移脚本"，而不是带着不一致继续跑。

---

### B-3｜"复用现有 RAG 链路、零改动"是错的：现有检索不是向量检索

- **原文**：`docs/知识库模块/文档解析与RAG记忆链路设计方案.md:463`「分片入库（**复用现有 RAG 链路，零改动**）」；V1.6 §5.3「多路召回；合并去重；强制过滤 …；Rerank 重排；Top-N 截断」。
- **现状核对**（`crates/service/src/rag_service.rs:94-156`）：

  ```sql
  SELECT dc.id, dc.chunk_text, dc.chunk_index,
         COALESCE(dc.title,'') AS title, COALESCE(dc.okf_type,'') AS okf_type,
         dc.asset_id, dc.token_count,
         0.0 AS similarity                       -- ← 相似度是常量 0
  FROM {schema}document_chunk dc
  WHERE dc.deleted = 0
    AND ($1::BIGINT IS NULL OR dc.tree_node_id IN (…递归子树…))
    AND ($2::VARCHAR IS NULL OR dc.okf_type = $2)
  ORDER BY dc.id                                -- ← 按主键排序，与问题无关
  LIMIT $3
  ```

  `params.question` **从未参与 SQL**；`min_similarity`、`max_tokens` **从未参与计算**。4 个调用方全部硬编码 `min_similarity: 0.0`：`conversation_service.rs:430`、`:484`、`:1095`、`crates/workflow/src/agent.rs:476`。
- **问题**：`/ask` 现在返回的是"该目录下按 id 排序的前 K 条 chunk"，与用户问题**零相关性**。V1.6 的 Rerank / Top-N / 混合召回全部建立在不存在的向量召回之上。
- **改为**：
  1. 把 §5.3 的"向量化 / 多路召回 / Rerank / Top-N"明确标注为 **V1.7 新建**，删除任何"复用 / 零改动"表述。
  2. 检索入口唯一化：`RAGRetriever::retrieve()` 内部改为「问题向量化 → 向量召回 → 过滤 →（可选）Rerank」，SQL 见 §7.3。
  3. 补齐被忽略的参数语义：`min_similarity` 作为 SQL 条件（`AND 1 - (embedding <=> $1) >= $5`）；`max_tokens` 作为**拦截式预算**（按 similarity 降序遍历累加 `token_count`，超预算截断），不能只靠 `LIMIT`。
  4. `ChunkResult.similarity` 从常量改为真实余弦相似度，前端"引用度"展示才有意义。
- **理由**：这不是"实现得不完美"，是"根本没实现"。把它当成既有能力去复用，会把 4 周的检索工作量隐藏成 0，直接导致排期失真。
- **验收**：用一个无关问题（如"今天天气"）对同一知识库检索，`similarity` 必须显著低于相关问题；相关问题的 Top-1 `asset_id` 必须是正确文档。**当前代码在这条验收上必然失败**（`similarity` 全 0）。

---

### B-4｜`chunk_and_vectorize` 从不生成向量，且 `RETURNING embedding` 必然运行时报错

- **现状核对**（`rag_service.rs:159-201`）：

  ```rust
  let sql = format!(
      "INSERT INTO {}document_chunk (asset_id, chunk_index, chunk_text, token_count, title, okf_type, tags, tree_node_id, created_at) \
       VALUES ($1,$2,$3,$4,$5,$6,$7,$8,NOW()) \
       RETURNING id, asset_id, chunk_index, chunk_text, token_count, embedding, title, okf_type, tags, tree_node_id, created_at, deleted",
      prefix
  );
  ```

  ① `INSERT` 列清单**没有 `embedding`** → 写入永远是 `NULL`；
  ② `RETURNING` 里带了 `embedding`，要解码进 `DocumentChunk.embedding: Option<Vec<f32>>`（`models.rs:856`）；
  ③ `apps/backend/src-tauri/Cargo.lock` 中**没有 `pgvector` crate** → sqlx 对 PostgreSQL 的 `vector` 类型没有 `Type` + `Decode` 实现 → 首次调用即 `unsupported type vector`。
- **改为**：
  1. 依赖 **必须**加 `pgvector`（不是可选、不是"以后再说"）：
     ```toml
     pgvector = { version = "0.4", features = ["sqlx"] }
     ```
     （`pgvector` 最新稳定版是 **0.4.2**；V1.6 写的 `0.7` 不存在。0.4.2 声明的是 `sqlx = { version = ">=0.8, <0.10", features = ["postgres"] }` —— 本工作区 `sqlx 0.9.0` 在范围内 ✅；`0.10` 被上界排除 —— 详见 D-1。）
  2. INSERT 前先批量算向量，写入时绑定 `pgvector::Vector`（类型安全），或退化为 `$n::vector` 文本绑定。
  3. 幂等：`(asset_id, chunk_index)` 建唯一约束 + `ON CONFLICT (asset_id, chunk_index) DO UPDATE`，避免重复点"重新向量化"产生重复分片（当前**没有任何唯一约束**）。
  4. `RETURNING` 去掉 `embedding` 列（用不上，还白传 768×4 字节/行）：`RETURNING id, asset_id, chunk_index, chunk_text, token_count, title, okf_type, tags, tree_node_id, created_at, deleted`。
- **理由**：这是**当前代码的既存 latent bug**，被 V1.6 的 `pgvector = "0.7"`（不存在的版本）掩盖了 —— 反正编译不过，就没人跑到运行时。
- **验收**：全新库执行一次"向量化"→ `SELECT count(*) FILTER (WHERE embedding IS NULL) FROM {schema}.document_chunk` 必须为 0；连续点两次"向量化"，行数不变。

---

### B-5｜`ort` + `ocr-tract` 双推理栈；`ort` 版本与 `pdf_oxide` 硬冲突

- **原文**（§13.1）：`ort = "2.0.0-rc.15"`。
- **现状核对**：`crates/parser/Cargo.toml:31-33` 已经选定纯 Rust 路线：

  ```toml
  render = ["pdf_oxide/rendering"]
  ocr    = ["pdf_oxide/ocr-tract", "dep:image"]
  ```

  而 `pdf_oxide` 的 `ocr` feature 把 ORT 钉死在 `ort = "=2.0.0-rc.13"`。若同时引入 `ort = "2.0.0-rc.15"`，Cargo 会解析出**两个 ort 版本**（`=` 精确版本无法统一），结果是 `onnxruntime` 符号重复 + 产物体积翻倍 + 运行时 DLL 加载冲突。
- **改为**：
  1. **从 §13.1 删除 `ort`**，一行都不要留。
  2. OCR 统一走 `pdf_oxide/ocr-tract`（内置 `tract-onnx`，纯 Rust，免随包分发 `onnxruntime.dll`）。
  3. 在 §5.1.2 增加一行硬约束：「**OCR 推理栈只允许一套**：`ocr-tract`（默认）。若确需 ORT 性能，改用 `pdf_oxide/ocr` 并接受其锁定的 `ort = 2.0.0-rc.13`；**两种 feature 互斥，同一构建中禁止并存**。」
- **理由**：Cargo 不会因为版本冲突报错，它会**同时编译两份**。这类问题在 CI 上是"绿"的，在用户机器上表现为 DLL 加载失败或内存翻倍。
- **验收**：`cargo tree -e features -p parser --features ocr | findstr /i ort` 无输出；`cargo tree -d` 中 `ort` / `onnxruntime` 不出现多于一个版本。

---

### B-6｜HNSW 索引实际不存在，且没写 pgvector 最低版本

- **原文**：§13.1「PostgreSQL 14+（pgvector扩展）」；§5.4 / §13.2 使用 `USING hnsw (...)`。
- **现状核对**：真实 DDL（`tenant_tables.sql:862-888`）里只有两条索引：

  ```sql
  CREATE INDEX IF NOT EXISTS idx_chunk_asset ON {schema}.document_chunk(asset_id, deleted);
  CREATE INDEX IF NOT EXISTS idx_chunk_tree  ON {schema}.document_chunk(tree_node_id, deleted);
  ```

  **没有任何 `USING hnsw`**。但同一段第 879 行的注释却写着「`1536维pgvector向量，HNSW索引加速`」——注释在骗人。对照 `docs/知识库模块/智能问答系统（RAG多轮对话）设计方案.md:335` 里是有 `CREATE INDEX idx_chunk_embedding ... USING hnsw (...)` 的，说明**落地时漏了这条索引**。此外 HNSW 是 **pgvector 0.5.0** 才引入的（0.4.x 只有 IVFFlat），V1.6 只约束 "PostgreSQL 14+" 不够。
- **改为**：
  1. 外部依赖一行改为：「PostgreSQL 14+，**pgvector ≥ 0.5.0（HNSW 最低要求），生产推荐 ≥ 0.7.0**」。
  2. 补齐索引（见 §7.2），并对老库补迁移：
     ```sql
     ALTER TABLE {schema}.document_chunk
       ADD COLUMN IF NOT EXISTS embedding_model_version VARCHAR(64) NOT NULL DEFAULT 'legacy-1536';
     CREATE INDEX IF NOT EXISTS idx_chunk_embedding ON {schema}.document_chunk
       USING hnsw (embedding vector_cosine_ops)
       WITH (m = 16, ef_construction = 64)
       WHERE embedding IS NOT NULL AND deleted = 0;
     ```
  3. 启动期版本断言：
     ```sql
     SELECT extversion FROM pg_extension WHERE extname = 'vector';  -- 必须 >= 0.5.0
     ```
     低于下限直接 `anyhow!` 中断启动，不要静默降级。
- **理由**：索引缺失时 `ORDER BY embedding <=> $1` 会退化为**全表顺序扫描**；10 万 chunk × 768 维 = 约 300MB 向量扫描，P95 从 20ms 级跳到秒级。而版本未约束会让"照文档部署"在第一句 `CREATE INDEX ... USING hnsw` 就失败。
- **验收**：`EXPLAIN (ANALYZE, BUFFERS) SELECT ... ORDER BY embedding <=> $1 LIMIT 8;` 执行计划里必须出现 `Index Scan using idx_chunk_embedding`，**不得**出现 `Seq Scan`。

---

### B-7｜`filter + HNSW` 的后过滤陷阱：状态过滤会让召回塌陷

- **原文**（§5.3）：「强制过滤 `is_deleted=false` 且 `status=active`」。
- **问题**：`status` 在 `doc_meta` 上，`rag_chunk` **没有 status 列** → 必须 JOIN `doc_meta`。HNSW 的执行语义是"先取 Top-K 再过滤"，即 `WHERE` 里的 JOIN 条件**已经晚于**索引扫描。如果命中集合里被下架的文档多，Top-8 过滤后可能只剩 0~1 条，而库里明明有大量有效数据。这是 pgvector 最经典的坑，表现为"召回不稳定、时好时坏"，极难归因。
- **改为**：
  1. `rag_chunk` 冗余 `is_deleted`（已有）并**保持与 `doc_meta.status` 同步**（同一事务内双写，见 F-3）；`status` 过滤改为在 `doc_meta` 上做**二次确认**，不再作为主过滤。
  2. 向量索引建成**部分索引**：`WHERE text_vector IS NOT NULL AND is_deleted = false`，让"已删除"的行根本不进索引。
  3. 过采样兜底：业务层取 `k * oversample`（`oversample` 默认 4，配置项 `rag.retrieve_oversample`），过滤后不足 `k` 条则最多重试 3 次、每次翻倍，并在 `metadata.degraded = true` 时记录日志。
  4. 若 pgvector ≥ 0.8：可用 `SET LOCAL hnsw.iterative_scan = relaxed_order;` 让服务端迭代补齐，业务层就无需过采样（二选一，配置开关 `rag.hnsw_iterative_scan`）。
- **理由**：`WHERE` 里的业务过滤和 ANN 索引是"两个世界"。不显式处理，检索质量会随"历史删除文档占比"漂移 —— 上线 3 个月后突然开始召回变差，而代码一行没改。
- **验收**：仓库里造 100 条 chunk、把其中 95 条的所属 doc 置为 `deleted`，再检索 `LIMIT 8`：结果条数仍应为 8（过采样生效），且 `degraded` 标记被正确设置。

---

### B-8｜Ollama 不提供 CLIP 图像向量，也不提供 Rerank

- **原文**（§10）：`embed_image_model = "clip"`、`rerank_model = "bge-reranker"`；§13.3「Ollama（本地Embedding/**Rerank**/LLM/VLM）」。
- **问题**：Ollama 的 embedding 端点（`/api/embed`、`/api/embeddings`）**只接受文本输入**，没有 CLIP 图像编码能力，也没有 rerank 端点。所以 §5.4 的 `image_vector vector(512)` 这一列**永远写不进值**，§7.2「同时 text_vector 与 image_vector」、§7.4「视频片段 image_vector」都是空转。
- **改为**：三选一，必须拍板并写进 §13.3：

  | 选项 | 做法 | 成本 | 适用 |
  |---|---|---|---|
  | **①（推荐）砍掉图像向量** | 图片走 OCR 文本 + VLM 语义描述 → 只写 `text_vector`；`image_vector` 列保留但可空 | 最低，复用现有 `ParseResult.images`→VLM 链路 | V1.7 |
  | ② 本地 ONNX 图像编码 | 引入 `fastembed-rs`（ONNX Runtime 或 tract）跑 CLIP ViT-B/32 | 中：多一个推理栈（与 B-5 冲突，需再评估） | V1.8 |
  | ③ 独立 TEI 服务 | `text-embeddings-inference` 部署 `/embed`，同时出文本与图像向量 | 高：多一个服务要运维 | 服务端多机场景 |

  Rerank 同理：`bge-reranker` 需经 TEI 或 `fastembed-rs`，**不能**写在 `[rag]` 段里假装是 Ollama 能力。若 V1.7 不做，就把 §5.3 的"Rerank 重排"降级为"可选，V1.8"，并把 `rerank_model` 配置项删掉（配置项存在但没人读，比不存在更糟）。
- **理由**：`image_vector` 是贯穿 §5.4 / §6.2 / §7.2 / §7.4 的骨架字段。它依赖一个**不存在的能力**，必须在 V1.7 拍死，否则 DDL 里那一列只是长期 NULL 的装饰。
- **验收**：`[rag]` 段里每个键都能在代码里找到一个读它的地方（`grep -r "embed_image_model" src/` 非空）；否则删键。

### B-9｜`S3Client::download_object` 全量读进内存，GB 级文件必然 OOM

- **原文**（§4.3 / §5.2）：解析前「从对象存储拉取原始文件到本地临时目录」——隐含前提是"下载这一步是安全的"。
- **问题**：`crates/storage/src/s3.rs:330-364` 的实际写法是
  ```rust
  let bytes = resp.body.collect().await...into_bytes();   // ← 整个对象读进 Vec<u8>
  std::fs::write(&local_path, &bytes)...;
  ```
  `collect()` 把**整个对象**读成内存字节数组。V1.6 的目标文件包含 GB 级 PDF / 视频，2GB 的文件就是 2GB 堆分配 —— 桌面端（Tauri，用户机器）必崩。另外 `create_dir_all(target_dir)` 排在下载**之后**（`:358`），顺序也应前置。
- **改为**：改成流式落盘，**签名不变**（`(bucket, key, target_dir) -> PathBuf`），调用方零改动：
  ```rust
  std::fs::create_dir_all(target_dir)?;                 // 前置
  let mut file = tokio::fs::File::create(&local_path).await?;
  let mut body = resp.body;                             // ByteStream
  while let Some(chunk) = body.try_next().await.map_err(...)? {
      tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await.map_err(...)?;
  }
  tokio::io::AsyncWriteExt::flush(&mut file).await?;
  ```
  并在 §4.3 明确写一句：「下载必须流式；**禁止** `body.collect()`；峰值内存不随文件大小增长」。
- **理由**：所有解析路径都**必须先下载**（`DocParser::parse_file` 只接受本地路径，见 F-7），所以这是整条链路的**第一块砖**。第一块砖是坏的，后面的 diff / embed / store 全部白做。
- **验收**：POC-5（§10.5）—— 下载一个 ≥1.5GB 对象，进程峰值 RSS 增量 < 100MB；`git grep -n "body.collect()" crates/storage/src/` 无命中。

---

## 2. 数据模型缺陷 P1（12 条）

> 本节所有问题都在 DDL 层可复现，**DDL 不入库**是放行门槛之一。修正后的完整 DDL 见 §7。

### M-1｜`doc_meta.tenant_id TEXT` 与现有"schema-per-tenant"隔离模型双轨

- **原文**（§8.1）：`tenant_id TEXT NOT NULL`。
- **现状核对**：本项目的多租户是 **PostgreSQL Schema 隔离**，不是行级 `tenant_id`：
  - `crates/database/src/postgres.rs:912-918` 启动时 `CREATE SCHEMA IF NOT EXISTS {schema}`；
  - `postgres.rs:759-761` 从 `public.sys_tenant.schema_name` 遍历所有租户；
  - `postgres.rs:722` 所有租户表 DDL 通过 `sql.replace("{schema}", schema)` 生成；
  - `rag_service.rs:96` 用 `database::schema_prefix()` 拼表名；
  - `sys_tenant.id` 是 `BIGINT`（雪花），**不是** TEXT。
- **问题**：`tenant_id TEXT` 会变成**第二套租户标识**（TEXT 与 BIGINT 两套 ID 并存、语义不同），且给了"跨租户 JOIN"一个看似合法的抓手 —— 而 schema 隔离下跨租户 JOIN 必须走 `多租户超级管理员跨Schema查询设计方案.md` 的白名单机制，不能靠 `WHERE tenant_id = ?`。
- **改为**：
  ```sql
  tenant_id BIGINT,          -- 冗余审计字段（与 public.sys_tenant.id 同源）；隔离由 schema 保证
  ```
  - 允许 NULL（历史数据 / 单体部署）；
  - 写入时由 service 层从当前会话上下文注入，**不参与任何 WHERE 过滤**（隔离靠 schema）；
  - 在 §5.5.4 之后补一节 **§5.5.6 多租户与 schema 隔离**：明确「业务表一律建在租户 schema 内，`runledger` 内部表建在**独立的 `runledger` schema**（不是各租户 schema），因为它没有租户概念」。
- **理由**：`TEXT` + `sys_tenant.id BIGINT` 不一致会在跨模块传参处直接编译/反序列化失败；而两套租户口径是**长期技术债**，晚改一天成本翻倍。

### M-2｜`doc_id = sha256(tenant_id + source_system + source_doc_id)` 无分隔符 → 拼接歧义

- **原文**（§8.2）：`doc_id = hex(sha256(tenant_id + source_system + source_doc_id))`。
- **问题**：字符串直接拼接存在**边界歧义**。`("ab","c","x")` 与 `("a","bc","x")` 拼接结果都是 `"abcx"`，哈希相同 → **两个不同文档得到同一个 `doc_id`**。这类碰撞概率不高但一旦发生就是"文档被静默覆盖"。
- **改为**：
  ```rust
  // 用长度前缀（或 \x1f 分隔符）消除歧义；推荐长度前缀，因为字段本身可能含分隔符
  let raw = format!("{}:{}:{}:{}", tenant_id.len(), tenant_id,
                                      source_system.len(), source_system,
                                      source_doc_id);
  // 三字段版本：len(a)|a|len(b)|b|c（最后一个字段不需要长度，因为它是结尾）
  let doc_id = hex::encode(Sha256::digest(raw.as_bytes()));
  ```
  §8.2 同步加一句：「**哈希输入必须带长度前缀或不可打印分隔符（`\x1f`），禁止裸拼接**」。同一规则适用于 `chunk_id`。
- **理由**：这是哈希 ID 方案的**标准缺陷**，成本 3 行代码，收益是消除一类静默数据覆盖。

### M-3｜`rag_chunk` 缺 `content_hash` 列，但 §8.6 把它当成幂等键的一半

- **原文**（§8.6）：「幂等维度：`(doc_id, doc_version)`、`(chunk_id, content_hash)`」；§8.3「Chunk 粒度哈希比对：区分未变更、修改、新增、删除四类」。
- **问题**：§13.2 的 `rag_chunk` DDL 里**没有 `content_hash` 列**，`diff_repair_task` 里却又有 `src_content_hash` / `tgt_content_hash`。也就是说：§8.3 的"四类区分"和 §8.6 的幂等键在**表结构上无处落地** —— chunk 级变更检测只能靠 `chunk_id` 是否变化，而方案 B 的 `chunk_id` 恰恰**故意不随内容变化**（这正是它的卖点）。结果：**正文改了但 `chunk_id` 没变 → 检测不到变更 → 不重新 embedding**。这是 V1.6 内部最严重的逻辑闭环缺口。
- **改为**：
  1. `rag_chunk` 增加：
     ```sql
     content_hash CHAR(64) NOT NULL,   -- sha256(规范化后的 chunk 文本)，用于变更检测
     ```
  2. 四类判定改写为**双键比对**：
     | 情况 | 判定 |
     |---|---|
     | 未变更 | `chunk_id` 相同 且 `content_hash` 相同 |
     | 修改 | `chunk_id` 相同 且 `content_hash` 不同 → 只重算这条向量 |
     | 新增 | `chunk_id` 不在旧集合 |
     | 删除 | `chunk_id` 在新集合中不存在 |
  3. §8.6 的幂等维度改写为「`(chunk_id, content_hash)` 作为 **embed 步骤的幂等键**」：`content_hash` 相同则跳过 embedding 调用（省 LLM 费用），不同则重算。
  4. 规范化函数要写死（避免"看起来一样但哈希不同"）：`normalize(t) = t.replace("\r\n","\n").trim_end()`，**不要**做全角/半角转换（会让"内容确实变了"被误判为没变）。
- **理由**：方案 B 的 `chunk_id` 稳定性和"内容变更检测"是**矛盾需求**，必须靠第二个哈希列调和。缺这一列，整个增量更新的 diff 步骤是空转。

### M-4｜`diff_repair_task` 缺唯一键，对账重跑会重复插工单

- **原文**（§8.1 / §13.2）：`diff_repair_task` 只有 `id BIGSERIAL PRIMARY KEY` + `idx_diff_repair_check_run`。
- **问题**：对账是**定时任务**，且 §5.5.5 支持"增量分段对账"。同一 `check_run_id` 内如果 handler 因为超时被 reaper 回收后重跑（§8.11 明确会发生），就会**重复插入同一差异工单**。运维看到的是"差异数翻倍"，进而误判数据健康状况。
- **改为**：
  ```sql
  CREATE TABLE IF NOT EXISTS {schema}.diff_repair_task (
      id BIGSERIAL PRIMARY KEY,
      check_run_id     TEXT NOT NULL,
      doc_id           TEXT NOT NULL,
      chunk_id         TEXT NOT NULL,
      src_content_hash CHAR(64),
      tgt_content_hash CHAR(64),
      diff_type VARCHAR(20) NOT NULL
          CHECK (diff_type IN ('src_only','tgt_only','hash_mismatch')),
      status VARCHAR(20) NOT NULL DEFAULT 'pending'
          CHECK (status IN ('pending','fixed','ignored')),
      note TEXT,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      resolved_at TIMESTAMPTZ,
      CONSTRAINT uk_diff_repair UNIQUE (check_run_id, doc_id, chunk_id, diff_type)
  );
  CREATE INDEX IF NOT EXISTS idx_diff_repair_check_run ON {schema}.diff_repair_task(check_run_id, status);
  ```
  插入用 `ON CONFLICT (check_run_id, doc_id, chunk_id, diff_type) DO NOTHING`。
- **理由**：幂等不是"代码写的时候小心点"，是"数据库不允许写重复"。定时任务 + 孤儿回收 + 分段执行三者叠加，重复执行是**常态**而非异常。

### M-5｜`tombstone` 缺查询索引与单调性约束，落后更新拦截不成立

- **原文**（§8.1 / §13.2）：`tombstone (id, doc_id, event_version, event_type, create_time)`，**无任何索引**。
- **问题**：§8.6 说「Tombstone 墓碑高版本优先」，即每次写入前都要查「该 `doc_id` 是否存在 `event_version` 高于本次的墓碑」。没有 `(doc_id, event_version)` 索引，这个查询就是全表扫描，而墓碑表是**只增不减**的 → 半年后每次写文档都要扫几十万行。
- **改为**：
  ```sql
  CREATE INDEX IF NOT EXISTS idx_tombstone_doc_ver
      ON {schema}.tombstone(doc_id, event_version DESC);
  -- 同一文档同一版本只允许一条墓碑事件，防止重放写入重复墓碑
  CREATE UNIQUE INDEX IF NOT EXISTS uk_tombstone_doc_ver
      ON {schema}.tombstone(doc_id, event_version);
  ```
  拦截查询固定为：
  ```sql
  SELECT 1 FROM {schema}.tombstone
   WHERE doc_id = $1 AND event_version >= $2 LIMIT 1;  -- 命中则拒绝写入
  ```
- **理由**：墓碑表的价值全在"查得快"。它只增不减，索引成本一次性、收益线性增长；没索引等于埋了个定时炸弹。

### M-6｜`workflow_binding` 缺 `(doc_id, doc_version)` 唯一约束，DB 层没有互斥兜底

- **原文**（§5.5.3 / §13.2）：`run_id TEXT NOT NULL UNIQUE`，`CREATE INDEX idx_workflow_binding_doc ON workflow_binding(doc_id)`。
- **问题**：§8.4 的互斥完全依赖 runledger 的 `active_workflow_key(doc_id)`。一旦降级到 `SimpleLocalOrchestrator`（§5.5.1 的 B 计划），**互斥能力直接消失**，而 `workflow_binding` 又没有唯一约束拦住重复提交 → 同一文档并发两次 `rag_etl`，两个 handler 同时 upsert 同一批 chunk，靠乐观锁才侥幸不坏（而乐观锁本身又有 F-2 的问题）。
- **改为**：
  ```sql
  CREATE TABLE IF NOT EXISTS {schema}.workflow_binding (
      id BIGSERIAL PRIMARY KEY,
      run_id        TEXT NOT NULL UNIQUE,
      doc_id        TEXT NOT NULL,
      doc_version   BIGINT NOT NULL,
      workflow_type VARCHAR(32) NOT NULL
          CHECK (workflow_type IN ('rag_etl','consistency_check')),
      status VARCHAR(20) NOT NULL DEFAULT 'running'
          CHECK (status IN ('running','succeeded','failed','cancelled')),
      started_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
      finished_at TIMESTAMPTZ,
      error_msg   TEXT
  );
  -- DB 层互斥兜底：同一文档同一版本同一工作流只允许一个活跃 run
  CREATE UNIQUE INDEX IF NOT EXISTS uk_wf_binding_active
      ON {schema}.workflow_binding(doc_id, doc_version, workflow_type)
      WHERE status = 'running';
  CREATE INDEX IF NOT EXISTS idx_wf_binding_doc ON {schema}.workflow_binding(doc_id, started_at DESC);
  ```
  `status` / `started_at` / `finished_at` 是**必需的**：`SimpleLocalOrchestrator` 没有 runledger 的 TUI 与状态表，业务侧必须自己存运行状态；否则任务面板（`docs/bpmninja-adk-graph 工作流引擎改造方案.md` 提到的可观测诉求）没有数据源。
- **理由**：`runledger` 是可选 feature（§5.5.1 明确）。**"可选"的组件不能是唯一的安全机制**。DB 唯一索引是唯一不随 feature 开关失效的互斥手段。

### M-7｜`rag_chunk` 缺 `token_count` / `char_len`，`max_tokens` 预算无依据

- **原文**（§5.4 / §13.2）：`rag_chunk` 字段为 `text TEXT`，没有 token 数。
- **问题**：`RetrieveParams.max_tokens` 存在（`models.rs:1132`），§5.3 也写「Top-N 截断」「PromptBuilder 差异化组装」，但**没有任何地方能知道一个 chunk 占多少 token**。现有 `document_chunk` 是有 `token_count` 列的（`tenant_tables.sql:867`），新表反而退化了。结果：Prompt 要么超模型上下文，要么保守地少放几条（浪费召回）。
- **改为**：
  ```sql
  token_count INT,          -- 与 embedding 同一 tokenizer 估算；NULL 表示未计算
  char_len    INT,          -- 字符数，用于无 tokenizer 时的兜底估算
  ```
  并明确估算口径（现有 `TextChunker::estimate_tokens` 的算法有问题，见 C-6）：ASCII 1 字符 ≈ 1/4 token，CJK 1 字符 ≈ 1 token，写入时一次算好落库，**查询期不再重算**。
- **理由**：token 预算必须落库，因为它与 embedding 模型绑定（同一文本在不同 tokenizer 下 token 数差 30%+）。查询期重算等于每次检索多一遍分词。

### M-8｜`start_sec` / `end_sec` 用 `FLOAT8`，与 Rust 结构体的 `f32` 不一致

- **原文**（§6.2）：`pub start_sec: Option<f32>`；§5.4 DDL：`start_sec FLOAT8`。
- **问题**：`f32` ↔ `FLOAT8(double)` 往返会导致**不等值**（`0.1f32 as f64 != 0.1f64`）。影响两处：① 播放器跳转精度（毫秒级可接受，但 `end_sec == start_sec` 这类判等会失效）；② 用 `(start_sec, end_sec)` 做去重/唯一键时永远匹配不上。
- **改为**：统一为 `f64` + `FLOAT8`（`pub start_sec: Option<f64>`），并在 §12.2 的"长音频时间戳漂移"风险条目里补一句：「时间戳统一 `f64` 秒；比较时按 `millis()` 取整，禁止浮点判等」。若嫌浮点麻烦，可改 `start_ms BIGINT / end_ms BIGINT`（**推荐**：整型 ms 天然可判等、可做唯一键）。
- **理由**：时间轴是音视频检索的**定位依据**，浮点判等失效会导致"点了播放器跳不到帧"这种难复现的 bug。整型毫秒是这类系统的标准做法。

### M-9｜缺 `page_no` / `file_name` / `source_uri`，§7.1「返回源文件、页码」无法实现

- **原文**（§5.3）：「LLM/VLM生成回答，返回**源文件、页码**、时间戳、帧路径」；§7.1「Query：文本向量检索，Prompt 携带**文件名页码**」。
- **问题**：`rag_chunk` 只有 `text / image_path / start_sec / end_sec / metadata`。页码只能塞进 `metadata`（JSONB），意味着：① 无法建索引做"按页码过滤"；② 无法在 SQL 里 `ORDER BY page_no` 保证引用顺序；③ 前端"跳转到第 12 页"要么解析 JSON，要么额外查 `doc_meta`。
- **改为**：
  ```sql
  page_no   INT,             -- PDF/图片页码（1-based）；非分页格式为 NULL
  file_name VARCHAR(512),    -- 冗余：避免每次 JOIN doc_meta
  source_uri TEXT,           -- 冗余：原始对象存储地址，前端直接可用
  ```
  `file_name` / `source_uri` 属于**刻意的反规范化**（与现有 `document_chunk.title` / `tree_node_id` 冗余同一套路，见 `tenant_tables.sql:881-883` 的注释意图），理由是不让检索热路径多一次 JOIN。
- **理由**：「返回源文件、页码」是 V1.6 自己承诺的产品能力。字段不存在，前端只能拿到一段无出处的文本 —— 用户无法验证回答，RAG 的可信度直接归零。

### M-10｜`doc_meta` 缺 `source_rev` / `source_etag`，无法回溯到 S3 上的具体对象版本

- **原文**（§5.4）：`doc_meta(source_system, source_doc_id, source_uri, doc_version, active_doc_version, ...)`。
- **问题**：`doc_version` 是**每次 claim 自增的内部版本**（F-6 方案①），而 `file_uploads.version` 是"人替换文件"的**源版本**（`upload.rs:154-171`，`MAX(version)+1`）。两者语义不同、递增时机也不同，**不能互相赋值**。现有 DDL 里没有任何一列记录"当前生效的 chunk 集合对应 S3 上哪一个版本的对象"，于是：① 回答不了"线上服务的是第几版文件"；② 回滚 `active_doc_version` 时不知道要回收哪个对象；③ F-5 的对账修复、B-2 的维度迁移都无法定位源。
- **改为**：`doc_meta` 增两列：
  ```sql
  source_rev    BIGINT,        -- 源版本：对应 file_uploads.version；NULL = 非上传来源
  source_etag   VARCHAR(255),  -- S3 ETag：对象内容指纹，用于判定"对象是否被替换过"
  ```
  `source_uri` 统一存 `s3://{bucket}/{object_key}`，**不存** `file_url`。理由：`file_url` 由 `public_base_url` 拼接（`upload.rs:581`），换域名/换端口即失效；`s3://` 永远可 `download_object` 复现。前端下载链接另走 `presign_get_object`（`s3.rs:300`）。
- **理由**：没有 `source_rev`，"版本共存 + 活跃版本指针"（F-6）在**上传场景**下没有源锚点 —— 版本号能自增，但说不出它对应哪个文件、哪次替换。
- **验收**：§7.7 的 `step -1` 能写出非空 `source_rev`；对任一 `status='active'` 的文档，`SELECT source_uri, source_rev FROM doc_meta` 能唯一定位到一个 `file_uploads` 行。

### M-11｜会话记忆 / 向量库冷热层 / 业务库三者没有边界定义，实现时必然被混用

- **原文**（§5.3）：多路召回里写「缓存命中直接返回」；§8.9 写「缓存（Redis / 内存）」；§12.3 风险表把「缓存穿透 / 雪崩」列为风险。全文**没有出现**「会话记忆」「Conversation History」「热库 / 冷库」任何一处。
- **问题**：把「缓存」当成一个东西，实现时最省事的做法就是三合一 —— 最近 N 轮对话写进同一套 Redis，并用「它访问频率高」当作放进热层的理由。这条推理里有三处错：
  1. **生命周期错**：会话记忆是**分钟~小时**（TTL 一过就该消失），向量库是**跟随文档生命周期**（同一 chunk 一年后还要能召回）；
  2. **访问方式错**：会话记忆按 `session_id` 顺序取，**不做相似度检索**；向量库的价值全在近似最近邻；
  3. **一致性要求错**：会话记忆允许最终一致（丢一轮上下文可接受），向量库**不允许**（少一个 chunk 就是答不出来）。

  更危险的是运维侧的连带伤害：会话缓存按前缀清理或 `FLUSHDB`，会把热向量一起清掉（见 §7.8 H 的红线）。
- **改为**：§5.4 与新增 **§7.8** 明确三个**独立**模块，各自独立 DDL / 独立 TTL / 独立清理策略：

  | 模块 | 存什么 | 生命周期 | 一致性要求 | 可否丢弃 |
  |---|---|---|---|---|
  | **会话记忆**（Session Memory / Conversation History） | 最近 N 轮 user/assistant 消息、工具调用记录 | 分钟~小时（TTL `session_memory_ttl_sec`） | 最终一致 | **可以**（丢了让用户重述） |
  | **向量库**（热层 / 冷层） | chunk 文本 + 向量 + 过滤元数据 | 跟随文档，与 `doc_meta` 同寿 | **强一致** | 热层可以；**冷层绝对不行** |
  | **业务库**（PostgreSQL） | `doc_meta` / `knowledge_asset` / `file_uploads` | 永久 | 事实源 | 不行 |

- **理由**：「热数据」在口语里是「访问多的数据」，在存储设计里是「**访问频率高 ∧ 数据可能频繁变化 ∧ 对延迟敏感 ∧ 仍需长期存在**」。少掉第四个条件，会话记忆就会被当成热向量数据，两套 TTL 与两套清理策略互相踩，最终表现为**偶发、不可复现的检索缺失**。
- **验收**：§11.7 第 1、2 条（配置项相互独立 + 清理任务不交叉）。

### M-12｜缓存 key 只有「查询 + top_k」，缺 `{schema}` 与 `{doc_version}`：照抄既有缓存方案就会跨租户串结果

- **原文**（既有设计文档 `docs/文档解析后向量化数据缓存方案.md:163`「Key 设计」）：
  ```text
  # 搜索结果缓存（TTL: 300 秒）
  SEARCH:{md5(query)}:{top_k} → JSON [{id, score, title, permission_level}]
  # 元数据缓存
  META:{knowledge_id} → JSON {title, permission_level, doc_source, knowledge_type}
  ```
- **问题**：这套 key 里没有**租户**、没有**文档版本**、没有**权限指纹**，四种串味全部会发生，而且**没有任何日志、没有任何异常**：
  | 缺哪个维度 | 触发条件 | 症状 |
  |---|---|---|
  | `{schema}`（租户） | 两个租户问了同一个问题（如「打印机驱动在哪」） | 后问的租户直接拿到前一个租户的 `id / title`，TTL 300 秒内不会自愈 —— 这是**跨租户数据泄露**，不是性能问题 |
  | `{doc_version}` | 文档被替换（F-6 / F-8 换版） | 命中旧 `chunk_id`，而旧 chunk 已按版本清理 → 前端拿到一批**指向不存在行**的引用（点开是空白） |
  | `{embedding_model_version}` | 蓝绿切换 / 换 embedding 模型（F-4） | 旧向量或旧答案继续命中，与 §8.2 的维度迁移冲突 |
  | 权限谓词 | 用户权限收回 / 文档从 `public` 改 `secret` | 已无权访问的结果继续被返回，直到 TTL 到期 |

  F-4 已经把「加工工艺」这一维补上（五元组），但没补租户与权限 —— 而**租户维度比模型维度更致命**：模型维度错了是答错，租户维度错了是把 A 公司的资料答给 B 公司。
- **改为**：在 F-4 五元组基础上扩为**七元组**，并把「不含租户数据的纯查询向量」单独拆出来：
  ```text
  # ① query 向量：只与 query 文本 + 模型有关，不含租户数据，可跨租户共享
  rag:qvec:{embedding_model_version}:{sha256(normalized_query)}
  # ② 检索结果：必须带 schema + 版本三元组 + 权限指纹
  rag:hits:{schema}:{embedding_model_version}:{chunk_strategy_version}:{doc_id}:{doc_version}:{top_k}:{perm_hash}
  # ③ 问答结果：把 doc_id/doc_version 换成 query 指纹（跨文档召回）
  rag:llm:{schema}:{embedding_model_version}:{chunk_strategy_version}:{sha256(query)}:{llm_model}:{perm_hash}
  ```
  - `{perm_hash}` = `sha256(排序后的可访问 doc_id 列表 ∪ permission_level 集合)`，**只参与 key、不落库**；用户权限一变 hash 就变，旧 key 自然失效，不需要逐条清缓存；
  - `{schema}` 用租户 schema 名（G-1），**不用** `tenant_id` 字符串（M-1 已说明双轨问题）；
  - 前缀统一 `rag:`；会话记忆用 `sess:` 前缀。两者**禁止**互相 `SCAN` / `KEYS` / `FLUSHDB`（§7.8 H 红线）。
- **理由**：缓存 key 是「正确性」的一部分，不是「优化」的一部分 —— 缺一个维度就是一次跨租户或跨版本的数据错误，而且**静默**。F-4 的教训本该一次学完。
- **验收**：§11.8 第 2、3 条（不同租户同 query → 不同 key；换版后立即检索 → 不返回旧 chunk）。

---

## 3. 流程正确性缺陷 P1（8 条）

### F-1｜两处流水线顺序互相矛盾：`parse→chunk→embed→diff→store` vs `parse→…→diff→embed→store`

- **原文**：§4「rag_etl（parse→**chunk→embed→diff**→store→check）」；§5.2 同样是 `chunk → embed → diff → store`；但 §8.4.2 写「3. chunk；4. **diff**（四类区分）；5. **embed（仅重算变更 chunk）**；6. store」。
- **问题**：两套顺序**在语义上不可能同时成立**。若 embed 在 diff 之前（§4/§5.2），则**每条 chunk 都算了向量**，§8.4.2 的"仅重算变更 chunk"就是空话，增量更新省不下 LLM 费用 —— 而 V1.6 §2 明确把"算力消耗极高"列为要解决的痛点。若 embed 在 diff 之后（§8.4.2），那 §4 的架构图与 §5.2 的流水线是错的。
- **改为**：**统一为 §8.4.2 的顺序**，并修正 §4 与 §5.2：
  ```
  parse → chunk → diff（与库内现有 chunk 按 (chunk_id, content_hash) 双键比对）
        → embed（仅对 新增/修改 的 chunk 调用 embedding）
        → store（upsert + 标记删除）
        → check（缓存失效 + 计数校验）
  ```
  同时在 §5.2 的 embed 一行补注：「embed 的输入是 diff 输出的 `changed_chunks`，不是全量 chunk；`content_hash` 未变的 chunk 直接复用库内已有向量（`store` 阶段不覆盖该列）」。
- **理由**：这是**成本模型**问题，不是措辞问题。顺序错了，增量更新退化为全量更新，V1.6 的核心卖点（省算力）消失。

### F-2｜乐观锁 `WHERE doc_version = expected` 在单表 upsert 上不成立

- **原文**（§8.6）：「数据库乐观锁 `WHERE doc_version = expected_version`」。
- **问题**：`rag_chunk` 的写入是 **upsert（INSERT ... ON CONFLICT）**，而 `doc_version` 只是 `rag_chunk` 的一个普通列，**不参与冲突键**。于是：
  ```sql
  INSERT INTO rag_chunk (..., doc_version) VALUES (..., 5)
  ON CONFLICT (chunk_id) DO UPDATE SET text = EXCLUDED.text, doc_version = EXCLUDED.doc_version
  -- 没有任何 doc_version 比较 → 版本 3 的旧任务可以覆盖版本 5 的新数据
  ```
  §8.6 声称的"防止旧任务回写脏数据"在 DDL 层**完全没有落地**。
- **改为**：冲突时加条件，让"旧版本无法覆盖新版本"由数据库保证：
  ```sql
  INSERT INTO {schema}.rag_chunk AS t (chunk_id, doc_id, doc_version, ...)
  VALUES ($1, $2, $3, ...)
  ON CONFLICT (chunk_id) DO UPDATE
     SET text = EXCLUDED.text,
         content_hash = EXCLUDED.content_hash,
         text_vector  = EXCLUDED.text_vector,
         updated_at   = now()
   WHERE t.doc_version <= EXCLUDED.doc_version;   -- ← 关键：只允许同版本或更高版本写入
  ```
  返回 `row_count = 0` 即表示"被更旧/更新的版本挡住"，handler 记录 `skip_stale_write` 指标并正常结束（**不重试**）。
- **理由**：分布式 + lease 租约 + reaper 回收的组合下，**旧任务晚到**是必然事件（不是异常）。乐观锁必须写在 SQL 里，写在注释里不算。

### F-3｜删除流程存在"元数据已删、chunk 未删"的可见窗口

- **原文**（§8.4.3）：「status=deleting 屏蔽检索；2. 软/硬删除；3. tombstone；4. 缓存失效；5. status=deleted；6. 删除专项校验」；§5.3「强制过滤 is_deleted=false 且 status=active」。
- **问题**：`doc_meta.status` 与 `rag_chunk.is_deleted` 是**两张表的两列**，V1.6 没有规定二者的写入事务边界。若先改 `doc_meta` 再批量改 `rag_chunk`，中间窗口里：检索按 `is_deleted=false` 命中、再 JOIN `doc_meta` 时发现 `status='deleting'` → 被过滤 → **召回变少**（B-7 的后过滤问题被放大）；若先改 `rag_chunk` 再改 `doc_meta`，中间窗口里：chunk 已不可见但文档仍显示"生效中" → 用户点开文档发现**检索不到自己的内容**。
- **改为**：明确"**单文档单事务**"边界，并在 §8.4.3 写死顺序：
  ```text
  BEGIN;
    UPDATE doc_meta SET status = 'deleting', updated_at = now()
     WHERE doc_id = $1 AND doc_version = $2 AND status = 'active';   -- 0 行则整体放弃
    UPDATE rag_chunk SET is_deleted = true, updated_at = now()
     WHERE doc_id = $1 AND is_deleted = false;
  COMMIT;
  -- 事务提交后再做：tombstone 落盘 → 缓存失效 → 若硬删除则异步物理清理
  ```
  - **tombstone 与缓存失效必须在事务外**（它们不可回滚：缓存删了没法"恢复"，墓碑是外部可见事件）；
  - 量化约束：单文档 chunk 数 > 5000 时，改为"先置 `doc_meta.status='deleting'`（检索侧立即屏蔽）+ 分批 `UPDATE rag_chunk`"，并接受短暂不一致（此时检索侧靠 `doc_meta` 屏蔽，不靠 `is_deleted`）；
  - 在 §5.3 检索侧补一句：**检索的最终一致判定以 `doc_meta.status = 'active'` 为准**，`is_deleted` 只用于索引剪枝（与 B-7 呼应）。
- **理由**：「屏蔽检索」和「删除数据」是两个动作，它们之间的窗口必须明确由谁负责。不写清楚，实现者会各写一套，然后出现"删了还能搜到"或"搜不到但文档还在"的两类工单。

### F-4｜缓存 key 缺 `embedding_model_version` / `chunk_strategy_version`

- **原文**（§8.9）：「key 绑定 `doc_id + doc_version`，精准清除」；§8.8：「分块策略迭代：`chunk_strategy_version` 标记，版本变更**强制完整重解析**」；「Embedding 模型升级 … 蓝绿双索引」。
- **问题**：蓝绿切换或分块策略升级时，`doc_version` **不会变**（文档内容没改）。于是：
  - query 向量缓存：旧模型的向量继续命中 → 用 768 维向量去查新的 1024 维索引 → 直接报错或全错；
  - 检索结果缓存 / LLM 问答缓存：旧分块方案的答案继续命中 → 用户看到的是"已经不该存在的引用"。
- **改为**：缓存 key 统一为五元组：
  ```text
  rag:qvec:{embedding_model_version}:{sha256(normalized_query)}
  rag:hits:{embedding_model_version}:{chunk_strategy_version}:{doc_id}:{doc_version}:{top_k}
  rag:llm:{embedding_model_version}:{chunk_strategy_version}:{doc_id}:{doc_version}:{llm_model}
  ```
  §8.8 的蓝绿流程补一步：「**切换流量前必须 flush 全部 `rag:*` 前缀缓存**（不是按 doc_version 精准清，而是按 `embedding_model_version` 前缀全清）」。版本变更也要触发 `chunk_strategy_version` 的全局缓存失效。
- **理由**：`doc_version` 只能表达"文档内容变了"，表达不了"加工工艺变了"。缓存的正确性依赖完整 key，缺维度就等于**跨版本串味**，而且错误是静默的。

### F-5｜`consistency_check` 走只读从库做全量对账 → 复制延迟造成"假差异"

- **原文**（§5.5.4）：「只读从库(Replica)｜对账 + 查询 + 报表｜`consistency_check` 全量对账、RAG 检索、对账 SQL」；「高可用：… 复制延迟监控，超阈值告警」。
- **问题**：对账的语义是"源数据 vs 向量库是否一致"。源侧若走**从库**，从库落后 30 秒时，刚入库的文档在从库上还不存在 → 对账得出 `tgt_only` 差异 → 写入 `diff_repair_task` → 修复任务去"修一个本来没坏的东西"，甚至可能按错误的源数据把新数据删掉。**只监控复制延迟并不能阻止误判**，因为对账是批量长事务，跑的过程中延迟会变。
- **改为**：
  1. 对账的**源侧必须读主库**（或至少：源侧读主库、目标侧读从库，且**两侧都记录快照水位**）；
  2. 每次对账开始时取一次水位，结束时再取一次，**水位变化超过阈值（默认 `pg_last_wal_replay_lsn` 差值对应的字节数）则本次结果标记 `unreliable = true` 并丢弃**，不写工单；
  3. 差异判定加"防抖"：`tgt_only` 且 `doc_meta.updated_at > check_started_at - 10min` 的条目**直接跳过**（最近改动的文档不给判定）；
  4. §5.5.4 的表格里，把"对账"从从库职责中**移除**，只保留"查询 + 报表"。
- **理由**：对账系统的第一原则是"**不要产生假告警**"。一个会误报的对账比没有对账更糟 —— 它训练运维忽略告警。

### F-6｜「先新版本校验通过再切换活跃版本」与「单表 upsert + 版本自增」不能同时成立

- **原文**（§8.11）：「双写回滚机制：先新版本校验通过再切换活跃版本」；§8.4.2：「6. store（upsert+标记删除+版本自增）」。
- **问题**：`rag_chunk` 是**单表 + 唯一 `chunk_id`**。upsert 会**原地覆盖**旧向量；一旦覆盖，**旧版本就没有了**，"校验不通过时切回旧版本"无数据可切。所谓"先校验后切换"要求新旧两版**同时存在**，而当前 DDL 做不到。
- **改为**：二选一（推荐 ①）：

  | 方案 | 做法 | 代价 |
  |---|---|---|
  | ①（推荐）**版本共存 + 活跃版本指针** | `rag_chunk` 唯一键改为 `(chunk_id, doc_version)`，不覆盖；`doc_meta.active_doc_version` 指向当前生效版本；检索固定 `JOIN doc_meta ON rag_chunk.doc_version = doc_meta.active_doc_version`；旧版本由 `check` 成功后异步清理（保留 1 个版本用于回滚） | 存储 ×2（短期），检索多一个 JOIN（可用部分索引缓解） |
  | ② 放弃双写回滚 | 删除 §8.11 的"双写回滚"，改为"写失败即标记 `doc_meta.status='failed'`，人工/重跑恢复" | 回滚靠重跑，RTO 变长 |

  若选 ①：`chunk_id` 的语义要澄清 —— 它标识"逻辑位置"，`(chunk_id, doc_version)` 标识"某个版本下的具体内容"。这样 `ON CONFLICT (chunk_id, doc_version)` 天然幂等，F-2 的乐观锁也变成"只写自己那一版"，更简单。
- **理由**：V1.6 同时承诺了"原地 upsert（省存储）"和"可回滚（要历史）"，这两者在单表上是**互斥**的。必须显式选一个，否则实现时会发现"回滚功能写不出来"，然后在联调阶段临时改表。

### F-7｜"上传"与"解析"之间没有任何衔接定义：谁触发、S3 何时可用、失败怎么办

- **原文**：V1.6 全文（§4、§5.2、§8 等）**一次也没有提到**上传链路、`file_uploads`、presigned 直传或对象存储生命周期 —— `grep -c "file_uploads"` 在 V1.6 正文中为 **0**。
- **问题**：解析的输入是文件，而文件从哪来、什么时候算"可用"，文档没写。核对本仓库现状：
  1. 解析器**只接受本地文件路径** —— `DocParser::parse_file(&self, file_path: &str, ...)`（`crates/service/src/doc_parser.rs:98-107`），remote 分支也是把 `{"file_path": ...}` POST 给侧车（`:146-150`）。所以"解析"必然以"文件已在本地磁盘"为前提。
  2. 上传是**六个状态**的状态机：`pending → uploading → completed → committed`，分支 `cancelled` / `failed`（`crates/storage/src/upload.rs:37`、`init:141-231`、`start_upload:240-320`、`complete:496-603`、`commit:610-653`、`abort:659-703`）。文档里没有任何一句把这些状态映射到 RAG 状态机。
  3. `complete` 之后 S3 对象才持久化（`complete_multipart_upload`，`upload.rs:574-578`），`commit` 之后才有业务上下文（`context_type/context_id`，`upload.rs:634-650`）。**在这两者之间没有任何钩子**，于是"什么时候可以开始解析"在实现时只能靠猜。
- **改为**：**先落 S3、再解析**，且把两个时刻分开（完整时序见新增 §7.7）：
  ```
  complete（S3 已持久化）= 解析的"准入条件"
  commit  （业务已绑定）= 解析的"放行条件"
  ```
  三条硬理由：
  1. 若"先解析后上传"，服务器必须经手全量字节，等于放弃 presigned 直传（`upload_routes.rs:133` / `s3.rs:203-234`），GB 级文件必然超时或占满内存；
  2. **可重建性**：S3 对象是唯一**不可重建**的资产。chunk 永远可以从 S3 重算（B-2 维度迁移、换 `embedding_model_version`、F-5 对账的修复手段全都是"重解析"）；反过来不成立；
  3. `rag_chunk.image_path` / `source_uri`（M-9）要求帧与源文件有**长期可寻址**的 key。若解析在前，帧在解析阶段还没有 S3 key 可分配，只能事后补传 → 两条上传路径、两套失败处理、两套清理逻辑。
- **理由**：这是整条链路的**第一步**。V1.6 缺这一步，等于从第二步开始画施工图。
- **验收**：§7.7 时序表每一行都能对应到 `upload.rs` 或 `doc_meta` 的真实字段/语句；V1.7 正文中 `grep -c "file_uploads"` ≥ 1。

### F-8｜`§7.6 step 0` 的 claim 会**屏蔽线上文档**，与 B-7 / F-3 自相矛盾

- **原文**（§7.6 step 0）：`UPDATE doc_meta SET doc_version = doc_version + 1, status = 'processing', ... WHERE doc_id = $1 AND doc_version = $4 AND status IN ('pending','active','failed') RETURNING doc_version;`
- **问题**：这段在**换版**（用户替换已生效的文件）场景下会造成召回中断 —— 一旦 claim 把 `status` 从 `'active'` 改成 `'processing'`，而检索的最终一致判定是以 `doc_meta.status = 'active'` 为准（B-7 的结论），那么**从用户点"替换"到新文件解析完成的整段时间里，旧版本也检索不到了**（GB 视频可能是几分钟）。这正是 B-7 描述的"召回时好时坏"，并且与 F-3 自己写的"用户点开文档发现检索不到自己的内容"是同一类错误的另一种触发方式。
  另外 `status IN ('pending','active','failed')` 这个白名单里没有 `'staged'`（§7.7 新增），投机解析下"同一文件重复解析"会静默 0 行、看不出原因。
- **改为**：claim **只做版本 CAS，不改 `status`**；互斥交给 `workflow_binding` 的部分唯一索引（M-6 的 `uk_wf_binding_active`）：
  ```sql
  UPDATE {schema}.doc_meta
     SET doc_version = doc_version + 1,
         embedding_model_version = $2,
         chunk_strategy_version  = $3,
         updated_at = NOW()
   WHERE doc_id = $1 AND doc_version = $4 AND deleted = 0
  RETURNING doc_version;                    -- 0 行 = 被别人抢先，直接放弃
  -- 紧接着 INSERT workflow_binding(doc_id, $N, 'rag_etl')；
  -- 命中 uk_wf_binding_active = 已有 run 在跑 → 放弃本次（不是错误，记指标）
  ```
  状态语义随之收敛为**"对外可见性"**，不再兼任"处理进度"：
  `pending`（已入队，不可见）/ `processing`（解析中，**仅新文档**）/ `staged`（解析完、待放行，不可见）/ `active`（生效）。
- **理由**：一个字段不能同时承担"进度"与"可见性"两种语义。一旦它既要表达"正在重算"又要表达"可以检索"，换版必然中断服务。
- **验收**：换版演练 —— 对一个 `status='active'` 的文档触发重解析，**在解析进行中**执行 §7.3 检索 SQL，必须仍能召回到旧版 chunk；解析完成后 `active_doc_version` 一次性切换。

---

## 4. 依赖与构建 P1（11 条）

> 本节全部经 `crates.io` 实际查询核实（2026-10-28）。**判据只有一个：`cargo build` 能不能绿。**
> 现有工作区基线（`apps/backend/src-tauri/Cargo.toml`）：`sqlx 0.9.0`（`postgres/runtime-tokio/tls-rustls/macros/chrono/rust_decimal`）、`tokio 1.53.2`、`tracing 0.1.44`、`reqwest` 未在根包、`futures 0.3.33`（`crates/api` 已在用）、`opentelemetry 0.32.0` + `opentelemetry_sdk 0.32.1` + `tracing-opentelemetry 0.33.0`（已在 `Cargo.lock` 中被间接引入）。

### D-1｜`pgvector = "0.7"` 不存在 —— 最高只有 0.4.2

- **原文**（§13.1）：`pgvector = "0.7"`。
- **核实**：`crates.io/api/v1/crates/pgvector` → `max_stable_version = 0.4.2`，`newest_version = 0.4.2`。**0.5 / 0.6 / 0.7 都不存在**（pgvector 的 Rust 客户端版本号与 PostgreSQL 扩展版本号是两套体系，V1.6 显然混了）。
- **改为**：

  ```toml
  # Cargo.toml（crates/database 或 crates/service，视 PgVectorStore 落点）
  pgvector = { version = "0.4", features = ["sqlx"] }
  ```

  - `0.4.2` 的 `sqlx` feature 声明的是 **`sqlx >= 0.8, < 0.10`**（`features = ["postgres"]`、`default-features = false`）。因此 **0.8.x ✅ / 0.9.x ✅ / 0.10.x ❌**（被上界排除）。本工作区 `sqlx 0.9.0` 在范围内 → **不需要**退回 sqlx 0.8。（核实话术来源：`docs.rs/crate/pgvector/0.4.2/source/Cargo.toml` 与 `crates.io/api/v1/crates/pgvector/0.4.2/dependencies`。）
  - ⚠️ **上界失效风险（必须写进 §13.1）**：将来工作区升到 sqlx 0.10 时**不会报「版本冲突」**——cargo 把 0.9 与 0.10 视为不兼容的两代并允许并存，pgvector 会**静默拉入它自己那份 `sqlx 0.9.x`**，`cargo tree -d` 里凭空多一个 sqlx（与 D-5 的 runledger 同款症状，但这次是隐式的，没有任何 Cargo.toml 里写着它）。真正报错的地方在**调用点**：`impl Encode<Postgres> for Vector` 里的 `Postgres` 来自 sqlx 0.9，业务侧 `PgPool` 来自 0.10 → 报 `the trait bound pgvector::Vector: Encode<Postgres> is not satisfied`，而报错里**两个 `sqlx::Postgres` 名字完全相同**，几乎不可能从字面看出是「存在两个 sqlx」。三种对策（择一）：① 等 pgvector 发布含 `<0.11` 的版本；② `[patch.crates-io]` 打本地补丁；③ 走下面的文本绑定降级路径。
  - 若编译期出现 `Type`/`Encode` 实现不匹配（sqlx 0.9 改了部分 trait bound），降级方案是**不启用 `sqlx` feature**，改为在 SQL 里用文本绑定：`$1::vector`（把 `Vec<f32>` 序列化成 `"[0.1,0.2,...]"`）。这条降级路径**必须写进 §5.4**，供实现时兜底，同时也是上面对策 ③ 的落点。
- **理由**：版本号写错 → `cargo add` 直接失败，整个 RAG 章节无法开工。而且这个错误会让人误以为"需要把 sqlx 降回 0.8"，进而在 workspace 里制造第二个 sqlx（见 D-5 的同款问题）。

### D-2｜`datafusion = "40"` 不存在，且 V1.6 没有成立的使用场景

- **原文**（§13.1）：`datafusion = { version = "40", optional = true }`；§5.5.5「数万文档级：启用DataFusion，挂载Parquet快照+PG向量表」。
- **核实**：`crates.io` → `datafusion max_stable_version = 55.2.0`。**40 这个版本不存在**（DataFusion 的版本号从 1 跳到 50+，中间没有 40）。
- **问题**：除了版本号，**场景也不成立**：
  1. §5.5.5 的示例 SQL 是 `src_document_snapshot FULL OUTER JOIN vector_chunk` —— 这是**标准 SQL JOIN**，PostgreSQL 原生就能做，不需要 DataFusion；
  2. 要用 DataFusion 就必须先把 PG 里的向量表导出成 Parquet 快照（§10 `[datafusion] parquet_snapshot_path`），而**导出本身就是全表扫描**，比直接在 PG 里 JOIN 更慢；
  3. 引入 DataFusion 会带来 `arrow` / `parquet` / `object_store` 一大坨依赖，编译时间与产物体积显著上升（对一个 Tauri 桌面应用是纯负担）。
- **改为**：**删除 `datafusion` 依赖与 §5.5.5 的 DataFusion 分支**，改为：
  ```text
  对账实现（V1.7）：
  · 主路径：PG 只读副本上执行单条 FULL OUTER JOIN（带 statement_timeout + 分段 doc_id 范围）
  · 规模上限：单次分段 ≤ 5000 doc；超出则按 doc_id 哈希分片多次执行
  · 若未来真的需要列式计算（>100 万 chunk），再引入 DataFusion，并把 §5.5.5 的 Parquet 快照方案重新评估
  ```
  §3.2 非目标里的「DataFusion 仅用于对账步骤内部计算」整条删除；§11 的「DataFusion 双编译路径 CI」验收项删除；§12.3 的两条 DataFusion 风险删除；§10 的 `[datafusion]` 段删除。
- **理由**：「为了一个可以用 SQL 解决的问题引入一个列式查询引擎」是典型的过度设计。而 V1.6 为它写的 6 处约束（feature 隔离、CI 双路径、UDF 限流、禁止单文档 diff 使用）全是**自造的复杂度**，删掉它们本身就是交付物。

### D-3｜`sqlx = "0.7"` + `jsonb` feature + `runtime-tokio-native-tls` 三重错误

- **原文**（§13.1）：
  ```toml
  sqlx = { version = "0.7", features = ["postgres","runtime-tokio-native-tls","jsonb"] }
  ```
- **核实**：三处都错。
  1. 工作区是 `sqlx 0.9.0`（`Cargo.toml:61`）。写 `0.7` 会在同一个 workspace 里拉出**第二个 sqlx**（0.7 + 0.9），类型系统上 `PgPool` 不互通 → 编译错误或被迫写适配层。
  2. `runtime-tokio-native-tls` 这个 feature 在 sqlx 0.8 已拆分，0.9 的写法是 `runtime-tokio` + `tls-native-tls`（工作区用的是 `tls-rustls`，与 `jsonwebtoken 11` 的 `aws_lc_rs` 路线一致）。
  3. **`jsonb` 不是 sqlx 的 feature**。sqlx 的 43 个 feature 里只有 `json`（`jsonb` 是 PostgreSQL 的类型名，不是 feature 名）。工作区目前**没有启用 `json`**，所以任何 `sqlx::types::Json<T>` 用法都会编译失败。
- **改为**：

  ```toml
  # 与工作区保持一致，不重复声明版本
  sqlx = { workspace = true }
  # 若确实需要 JSONB 映射（本设计需要：rag_chunk.metadata / doc_meta.permission_json）
  # 则在 apps/backend/src-tauri/Cargo.toml 的 sqlx features 里追加 "json"
  ```

  即：`features = ["postgres","runtime-tokio","tls-rustls","macros","chrono","rust_decimal","json"]`。
  > 注意：本仓库根 `Cargo.toml` **没有 `[workspace.dependencies]` 段**（各 crate 各自声明版本）。所以 `workspace = true` 目前**不可用**。两个选择：① 在根 `Cargo.toml` 新增 `[workspace.dependencies]` 统一收口（推荐，顺带解决未来版本漂移）；② 各 crate 复制同一份 `version = "0.9.0"` + features。**V1.7 应明确选 ①**。
- **理由**：版本号 + feature 名一起错，意味着这行配置从来没被编译验证过。而 `jsonb` 这个不存在的 feature 会让 `cargo build` 直接报 `feature 'jsonb' does not exist`，属于一眼可见的硬伤 —— 它出现在"评审整改版"里，说明上一轮评审没有做过编译验证。

### D-4｜删除 `ort`（与 `ocr-tract` 互斥）

- **原文**（§13.1）：`ort = "2.0.0-rc.15"`。
- **改为**：整行删除。理由与验收见 **B-5**（`pdf_oxide` 的 `ocr` feature 钉死 `ort = "=2.0.0-rc.13"`，并存会编译两份 ORT）。
- **补充**：`ort` 还涉及 `pdf_oxide` 的二进制分发问题（§12.1「onnxruntime多平台分发」风险条目）。现方案用 `tract-onnx`（纯 Rust）**已经绕过了这个风险**，所以 §12.1 该风险条目也要从"中"降级为"已规避（改用 tract）"，否则风险清单与实现方案自相矛盾。

### D-5｜`runledger` 锁 `=0.9.0` 既太旧，又钉死 `sqlx ^0.8.6` —— 这是确定成本，不是风险

- **原文**（§5.5.2）：「严格锁定小版本：`runledger-core = "=0.9.0"`（示例），禁止自动升级」；§13.1 三件套都是 `=0.9.0`。
- **核实**：`crates.io` → `runledger-core max_stable_version = 0.12.0`。0.9.0 落后 3 个 minor。更关键的是：`runledger-*` 的依赖是 **`sqlx ^0.8.6`**（0.9.0 与 0.12.0 **都是**），而工作区是 `sqlx 0.9.0` → **Cargo 必然同时编译 sqlx 0.8.x 与 0.9.0**。
- **这条必须写进 §13.1 并量化，V1.6 完全没提**：

  | 后果 | 具体表现 |
  |---|---|
  | 两套 `PgPool` 类型 | `sqlx_0_8::PgPool` ≠ `sqlx_0_9::PgPool`，runledger 的 pool 不能传给业务代码；`WorkflowOrchestrator` 的签名里**不能出现任何 sqlx 类型**（否则 trait 无法在两种 feature 下统一） |
  | 两份 `_sqlx_migrations` | runledger 执行自己的官方 migrations 时用的是它那版 sqlx，迁移表在它自己的 schema 里，**不能与业务 migrations 混用** |
  | 二进制体积 | sqlx + rustls + 两套 driver 编译两份，实测通常 +3~6MB（release） |
  | 编译时间 | workspace 冷编译时间上升（sqlx 是重依赖） |

- **改为**：
  1. 版本改为 **`=0.12.0`**（最新稳定，且 workflow 相关能力更全），三件套统一。
  2. 在 §5.5.1 的 `WorkflowOrchestrator` trait 上**加硬约束注释**：
     ```rust
     /// 严禁在 trait 签名中出现任何 sqlx 类型（PgPool / PgConnection / Row）：
     /// runledger 依赖 sqlx 0.8.x，业务依赖 sqlx 0.9.x，两者类型不互通。
     /// 所有跨边界数据必须是 String / u64 / 自定义 DTO。
     ```
  3. 在 §13.1 增加一段「**已知构建成本**」：明确"双 sqlx、双连接池、双迁移表"，并给出验证命令（见验收）。这不是"风险"，是**已确认的固定成本**，写进文档以免后续被当成 bug 排查。
  4. 若评估后发现体积/编译时间不可接受，则**优先选 `SimpleLocalOrchestrator` 作为唯一实现**（V1.6 已经把 B 计划定义好了，直接扶正即可，见 G-7）。
- **理由**：这是一个**可以提前算出**的成本，V1.6 却把它留成了"风险中/应对：锁版本+季度评估"—— 锁版本恰恰是成本来源。文档里不写，最后一定有人在 code review 时问"为什么有两个 sqlx"。
- **验收**：`cargo tree -d | findstr sqlx` 输出中 `sqlx v0.8.x` 与 `sqlx v0.9.0` 同时出现即证明该成本存在；把它贴进 §13.1 作为事实记录。

### D-6｜`sherpa-rs = "0.6.8"` 应降级为"候选，待 spike 验证"

- **原文**（§13.1）：`sherpa-rs = "0.6.8"`；§10 `asr_engine = "sherpa"`、`asr_model = "sensevoice-zh"`；§12.1「ASR/OCR精度、模型许可证｜中｜法务确认；降级 whisper-rs」。
- **核实**：
  - `crates.io` → `sherpa-rs max_stable_version = 0.6.8`（版本存在，可用）；
  - 它通过 `sherpa-rs-sys` **自带 C/C++ 库**（sherpa-onnx），**不使用 `ort`** → **不存在** ORT 冲突（此点此前被误判，现予更正）；
  - 但 `download-binaries` 会在 **build.rs 阶段联网下载预编译二进制**，在离线 CI / 内网构建环境会直接失败；
  - 该 crate 发布节奏较慢；
  - **代码侧尚未决策**：`crates/parser/tests/spike_asr.rs` 里 sherpa vs whisper-rs 仍是待定状态。
- **改为**：
  1. §13.1 的依赖表里把它标为 **`# 候选（Phase 4 前由 spike 决定，未 spike 前不进 Cargo.toml）`**，而不是无条件列出。
  2. §10 的 `asr_engine` / `asr_model` 保留为**配置项**，但不承诺默认值。
  3. 新增一条 POC（见 §10.4）：**离线构建验证** —— `cargo build --features asr --offline`，若失败说明该路径无法用于内网交付，需改为"随包分发模型 + 系统预装库"。
  4. §12.1 的"降级 whisper-rs"要写清触发条件与工作量（whisper-rs 同样带 C 库 + 独立模型文件，**不是零成本降级**）。
- **理由**：`download-binaries` 在**内网/离线**场景是硬阻断，而 §3.1 明确要求"支持离线模式"。这个矛盾必须在 spike 阶段撞出来，不能留到交付前。

### D-7｜删除 `rxrust`，用工作区已有的 `futures` 做并发限流

- **原文**（§13.1）：`rxrust = { version = "0.x", optional = true }`；§5.2「embed：… 可选 rx-rust 做 buffer、并发限流、内存瞬时重试」。
- **问题**：
  1. `0.x` 在 Cargo 里允许任意 minor 跳变（0.x 之间允许 breaking change），锁出来的版本随时会变；
  2. `rxrust` 是**同步** Observable 模型，在 async 上下文里做"并发 N 的流控"需要自己再搭一层，代码量比直接用 `futures` 还多；
  3. 工作区**已经有** `futures = "0.3.33"`（`crates/api/Cargo.toml:36`），`StreamExt::buffer_unordered` 就是现成的"并发 N"。
- **改为**：
  ```rust
  use futures::{stream, StreamExt};

  // 并发 embed_max_concurrency，结果顺序不保证（需要保序用 buffer_unordered 后按 index 回填）
  let vectors: Vec<anyhow::Result<Vec<f32>>> = stream::iter(chunks.iter().enumerate())
      .map(|(i, c)| async move { (i, embed_one(&http, &c.text).await) })
      .buffer_unordered(cfg.embed_max_concurrency)
      .collect()
      .await;
  ```
  删除 `rxrust` 依赖与 §13.3 的 "rx-rust" 选型段落；§5.2 的表述改为「embed 用 `futures::stream::buffer_unordered` 控并发，瞬时重试手写 3 次指数退避（**不引入新依赖**）」。
- **理由**：为了一个"buffer + 限流"引入响应式库是**负收益**：多一个依赖、多一套心智模型、还得写适配层。`futures` 已在依赖树里，零成本。

### D-8｜`text-splitter = "0.12"` 版本落后，且用途未定义

- **原文**（§13.1）：`text-splitter = "0.12"`。
- **核实**：`crates.io` → `text-splitter max_stable_version = 0.33.1`。`0.12` 是两年前的版本线。
- **问题**：更根本的是**用途没写**。V1.6 通篇没说 `text-splitter` 用来干什么 —— 是**替换**现有自研 `TextChunker`（`rag_service.rs:13-85`），还是只做语义边界辅助？若是替换，`chunk_size = 512 / chunk_overlap = 128` 的语义会从"自研估算的伪 token"变成"真 token"，切出来的 chunk 大小完全不同，且会影响 `chunk_strategy_version` 的迁移（存量数据必须全量重算）。
- **改为**：在 §5.2 明确二选一：

  | 选择 | 说明 | 影响 |
  |---|---|---|
  | **①（推荐）用 `text-splitter = "0.33"` 替换自研 `TextChunker`** | 开 `markdown` + `tokenizer` feature，按标题边界 + token 预算切分，`chunk_size` 用**真 tokenizer** 计算 | `chunk_strategy_version` `v1`→`v2`，存量需重算；换来 C-6 的 token 估算问题彻底解决 |
  | ② 保留自研，删掉 `text-splitter` 依赖 | 只需修 `estimate_tokens` 算法（见 C-6） | 零迁移成本，但中文切分质量不如 text-splitter |

  无论选哪个，`0.12` 都不能留在文档里。
- **理由**：依赖清单里出现一个"没人知道干嘛用"的库，是最容易被实现同学删掉、然后文档与代码再次分叉的东西。

### D-9｜`opentelemetry 0.21` / `tracing-opentelemetry 0.23` 会在依赖树里制造第二套 OTel

- **原文**（§13.1）：
  ```toml
  tracing-opentelemetry = "0.23"   # V1.6：可观测性
  opentelemetry = "0.21"
  ```
- **核实**：本工作区 `Cargo.lock` 里**已经有**：

  | crate | 版本 |
  |---|---|
  | `opentelemetry` | `0.32.0` |
  | `opentelemetry_sdk` | `0.32.1` |
  | `opentelemetry-otlp` / `-proto` / `-http` / `-appender-tracing` | 同代 |
  | `tracing-opentelemetry` | `0.33.0` |

  （由 `tauri` / `adk-*` 等间接引入。）若按 V1.6 写 `0.21/0.23`，Cargo 会**再引入一套 0.21 代**（`opentelemetry` 的 0.x 之间互不兼容），于是出现两个全局 `TracerProvider`、两个 `propagator` 注册点、双份 span 导出。
- **改为**：**与锁文件对齐，而不是"升到最新"**：
  ```toml
  opentelemetry         = "0.32"
  tracing-opentelemetry = "0.33"
  ```
  > 注意：最新是 `opentelemetry 0.33.1` / `tracing-opentelemetry 0.34.0`，但**直接升到最新反而会与工作区已有的 0.32/0.33 代冲突**，制造出同样的问题。正确做法是先 `cargo tree -i opentelemetry` 查清实际版本，再**取相同 minor**。
  >
  > 落地前执行 `cargo tree -i opentelemetry` 与 `cargo tree -i tracing-opentelemetry`，把结果写进 §13.1 作为版本锁定依据。
- **理由**：可观测性是横切关注点，**版本必须与运行时其他组件同代**，否则 `tracing` 的 span context 无法跨库传播 —— 表现是"链路断了但日志正常"，极难排查。

### D-10｜`ffmpeg-sidecar` 已在依赖里但从未被调用，且不应进生产默认构建

- **原文**（§13.1）：`ffmpeg-sidecar = "2.7.0"`；§5.1.2 VideoParser「ffmpeg-sidecar；ffprobe 元信息；抽音轨+场景抽帧」。
- **现状核对**：`crates/parser/Cargo.toml:23` 已声明 `ffmpeg-sidecar = "2.7.0"`，但 `lib.rs` 的 `parse_file_blocking`（:85-100）对 `image` / `audio` / `video` 三类**直接返回错误**（"本地解析尚未支持"），全 crate 内**没有任何一处调用 `ffmpeg_sidecar`**。
- **改为**：
  1. 依赖改为 **optional + feature 门控**：
     ```toml
     ffmpeg-sidecar = { version = "2.7.0", optional = true }
     [features]
     media = ["dep:ffmpeg-sidecar", "dep:symphonia", "dep:rubato"]
     ```
     `default = []`，生产构建默认**不启用**（当前它只是白占编译时间）。
  2. §12.5 的 ffmpeg LGPL 合规段落补一句：「**默认构建不包含 ffmpeg 相关代码路径**；启用 `media` feature 时才分发 ffmpeg 二进制，从而把 LGPL 义务限制在明确的功能开关之后。」
  3. `ffmpeg-sidecar` 的"自动下载二进制"能力在离线环境同样会失败，需与 D-6 一起在 POC 里验证。
- **理由**：`manifest` 里声明但从不调用的依赖不是"预留"，是**债务**：增加编译时间、增加 CVE 扫描噪音、还会让评审误以为音频/视频能力已经就绪。

### D-11｜许可证审查缺 `symphonia`（MPL-2.0）等平台项

- **原文**：§12.5 只覆盖 ffmpeg 的 LGPL；§13.1 列了 `symphonia = "0.6.1"`、`rubato = "5.0"`，但**整份文档没有许可证章节**。
- **核实与风险**：

  | 依赖 | 许可证 | 说明 |
  |---|---|---|
  | `symphonia` | **MPL-2.0** | 文件级 copyleft。链接方式不限，但**修改过的 MPL 文件必须开源**；必须在 NOTICE 里声明 |
  | `pdf_oxide` 的 `table-ml`（若启用） | 间接拉入 `pdfium-render` → **PDFium（BSD-3 + 专利条款）**，需 **C++ DLL** | 这才是"解析要子进程隔离"的真正理由；V1.6 §5.1.4 提了隔离，但没和具体依赖挂钩 |
  | `ffmpeg`（外部） | LGPL-2.1+（禁用 GPL 特性） | §12.5 已覆盖 |
  | `runledger*` | MIT | 已核，无问题 |
  | `tract-onnx` | MIT / Apache-2.0 | 已核，无问题 |

- **改为**：新增 **§14 许可证与合规清单**（一张表覆盖全部直接依赖），并要求：
  1. 仓库根新增 `NOTICE`，写入 `symphonia`（MPL-2.0）与 ffmpeg（LGPL）；
  2. CI 加一步 `cargo deny check licenses`，allow 列表显式包含 `MIT / Apache-2.0 / BSD-3-Clause / MPL-2.0 / LGPL-2.1`；
  3. §5.1.4 的"子进程隔离"理由补一句：「**`pdf_oxide` 的表格能力（`table-ml`）会引入 `pdfium-render`（C++ DLL），这是把解析放进子进程的主要动机之一**；若 V1.7 不启用 `table-ml`，则该理由改为纯鲁棒性（防解析崩溃拖垮主进程）」。
- **理由**：MPL-2.0 不是"不能用"，但**必须声明**。这类问题在商业化交付的尽调阶段会被逐条追问，届时再补要翻半年的提交历史。

---

## 5. 与现有代码的集成冲突 P1（7 条）

### C-1｜`ParseResult` 新增 `blocks` 必须同时改 Python 侧契约，否则 remote 模式静默丢结构

- **原文**（§6.1）：直接给出 `ParseResult { file_name, file_type, raw_text, images, metadata }`，并注释「音频、视频 metadata 内置 segments」。
- **现状核对**：Python 侧 `apps/doc-parser/models/parse_result.py` 是**同名同字段**的契约；Rust 侧 `crates/parser/src/types.rs` 的文档注释明确写着「字段与 Python 侧车 `models/parse_result.py` 的 `ParseResult` 严格一致」。`ParseResult` 还被 `crates/service/src/doc_parser.rs:17` `pub use` 出去，`conversation_service` 与知识库链路都在用。
- **问题**：B-1 要求新增 `blocks`。若只改 Rust 不改 Python：`remote` 后端返回的 JSON 没有 `blocks` → 反序列化得到空 `Vec`（因为 `#[serde(default)]`）→ **local 和 remote 产出不同的 chunk_id**（local 有结构、remote 退化）。这正是 §11「黄金对比测试 Jaccard≥0.9」要防的分叉，但 Jaccard 只看文本相似度，**看不出 ID 分叉**。
- **改为**：
  1. `apps/doc-parser/models/parse_result.py` 同步新增 `blocks: list[Block] = []`（Pydantic 默认空列表）；
  2. §11 的黄金对比测试**增加一条硬断言**：`local` 与 `remote` 对同一文件的 `chunk_id` 集合**完全一致**（不只是文本 Jaccard）；
  3. 明确**过渡期策略**：Python 侧未升级前，`local` 后端默认 `blocks_enabled = false`（退化为 `fallback_index`），保证两条后端行为一致；用环境变量 `PARSER_STRUCTURED_BLOCKS=1` 提前启用新行为。
- **理由**：双后端架构的第一原则是"输出契约完全一致"。加字段不写同步方案，双后端就会在**同一个字段的不同含义**上分叉，而这种分叉在文本层看不出来。

### C-2｜`crates/orchestrator` 不存在，而仓库已有两套工作流引擎 —— 有变成第三套的风险

- **原文**（§5.5.1 / §13.1）：新建 `crates/orchestrator`，`trait WorkflowOrchestrator`，双实现 `RunledgerOrchestrator` / `SimpleLocalOrchestrator`。
- **现状核对**：
  - `crates/` 现有：`api / database / engine / parser / service / storage / utils / workflow`（**没有 orchestrator**）；
  - 根 `Cargo.toml:86-87` 已经依赖 **`adk-graph 2.3.0`** + **`adk-rust 2.3.0 (features=["graph"])`**；
  - `crates/workflow/src/agent.rs` 已有基于这些 crate 的 agent 能力（并调用 `RAGRetriever::retrieve`）；
  - `docs/bpmninja-adk-graph 工作流引擎改造方案.md` 描述了另一套（bpmninja + adk-graph）路线。
- **问题**：再引入 `runledger` + `orchestrator`，系统里会同时存在 **adk-graph（DAG）+ bpmninja（审批流）+ runledger（任务队列）** 三套"流程"概念。文档没有任何一处说明**它们的分工边界**，实现者一定会问"这个步骤该写在 adk-graph 的 node 里还是 runledger 的 workflow_step 里"。
- **改为**：在 §5.5.1 前插入 **§5.5.0 与现有工作流引擎的边界**，写死职责：

  | 引擎 | 负责 | 不负责 |
  |---|---|---|
  | `adk-graph`（已有） | **交互式**多步 Agent 推理、工具调用、会话内编排（用户可感知、秒级） | 后台长任务、重试、孤儿回收 |
  | `bpmninja`（已有） | **审批流**（人参与、有状态机、有表单） | 数据加工流水线 |
  | `runledger`（新增，可选） | **后台数据加工**作业（分钟~小时级、lease 租约、DLQ、cron） | 用户交互、审批 |

  并明确：`crates/orchestrator` **只服务于 RAG ETL 这一类后台批处理**，不复用、不替代前两者的任何能力；若半年内 `runledger` 无法转正，则直接删掉 `orchestrator`，ETL 改为 `tokio::spawn` + 自建状态表（即 `SimpleLocalOrchestrator` 扶正）。
- **理由**：三个"流程引擎"并存而不划边界，是这个项目最容易失控的地方。写清边界比选对库更重要。

### C-3｜`ocr_language = "ch+en"` 与现有代码/引擎语义都不兼容

- **原文**（§10）：`ocr_engine = "ppocr"`、`ocr_language = "ch+en"`。
- **现状核对**：`crates/parser/src/options.rs:12`：
  ```rust
  /// OCR 语言缺省值（与 `parsers/pdf_parser.py:17` 一致）
  pub const DEFAULT_OCR_LANGUAGE: &str = "chi_sim+eng";
  ```
  这是 **Tesseract** 的语系命名（`chi_sim` / `eng` 是 tesseract 的 traineddata 名）。而 PP-OCR 的识别字典是**模型内置的固定字典**（`ppocr_keys_v1.txt`），**不能在运行时用 `"ch+en"` 这种字符串切换语系** —— 换字典等于换模型文件。
- **问题**：这个配置项在两种引擎下的语义完全不同：
  - tesseract：运行时 `--tessdata` 组合语言包（`chi_sim+eng` 有效）；
  - PP-OCR：只能选"用哪个 rec 模型文件"（`ch_PP-OCRv4_rec` / `en_PP-OCRv4_rec` / 多语种模型）。
- **改为**：拆成两个字段，语义各自明确：
  ```toml
  ocr_engine = "ppocr"                 # ppocr | tesseract
  ocr_model  = "ch_PP-OCRv4_rec"       # PP-OCR: rec 模型名；tesseract: 语言串（如 chi_sim+eng）
  ocr_det_model = "ch_PP-OCRv4_det"
  ```
  代码侧 `ParseOptions` 相应改为 `ocr_model: String`（保留 `ocr_language` 作为 `#[deprecated]` 别名读旧配置，避免破坏现有 `.env.toml`）。§5.1.2 的 ImageParser 一行补注：「**`ocr_model` 与 `ocr_engine` 必须匹配**：`ppocr` 下只接受 PP-OCR 模型名，`tesseract` 下只接受语言串；启动期校验，不匹配直接报错。」
- **理由**：一个配置项承载两种互斥语义，必然有人把 `ch+en` 传给 tesseract（报"找不到语言包"）或把 `chi_sim+eng` 传给 PP-OCR（静默走默认模型）。拆字段是唯一干净的解法。

### C-4｜配置落点冲突：V1.6 给的是独立 TOML 表，现有是 `.env.toml` + `from_env()`

- **原文**（§10）：给出 `[doc_parser]` / `[rag]` / `[pgvector]` / `[db_pool_business]` / `[runledger]` / `[orchestrator]` / `[datafusion]` 七段 TOML，其中 `[pgvector] primary_database_url = "postgresql://user:pass@..."`。
- **现状核对**：
  - 现有配置是 **`.env.toml` + 环境变量双轨**：`crates/service/src/doc_parser.rs:40` 注释写"由 `.env.toml` 的 `[doc_parser] backend` 映射而来"，实际读的是 `std::env::var("DOC_PARSER_BACKEND")`（:43）；数据库侧 `postgres.rs:98-110` 的 `PostgresConfig::from_env()` 读 `PG_HOST / PG_PORT / PG_DATABASE / PG_USERNAME / PG_PASSWORD / READ_HOSTS`，**不是 URL，而是分字段**。
  - `apps/backend/.env.example.toml` 已存在（`docs/架构问题与缺陷分析及整改排期.md:145` 提到"完全没有 `[doc_parser]` 段"，说明该文件的段落在演进中）。
- **问题**：V1.6 的 `primary_database_url` 字符串与现有 `PostgresConfig` 结构**不兼容**，也没说 TOML 怎么映射到 env。实现时会出现"配置里写了但代码读不到"，或者实现者自创第三套读取方式。
- **改为**：在 §10 开头加一节 **§10.0 配置落点约定**：
  ```text
  1. 所有新增配置项一律先进 apps/backend/.env.example.toml，再在代码中通过
     std::env::var 读取（与现有 DOC_PARSER_BACKEND / PG_HOST 一致），
     不在代码里直接解析 TOML。
  2. 数据库连接继续用 PostgresConfig 的「分字段」形式（host/port/database/...），
     不引入 URL 字符串形式；只读副本沿用现有 READ_HOSTS 的
     "host:port:weight,..." 语法。
  3. §10 中 [pgvector] 段的 primary_database_url / readonly_database_url 两个键删除，
     改为复用既有 PG_* 与 READ_HOSTS。
  4. 新增的 [rag] 段每一项都要在 .env.example.toml 里有对应键 + 注释 + 默认值。
  ```
  并给出一份**可直接粘贴的 `.env.example.toml` 增量**（把 §10 的键逐一映射成 `RAG_*` 环境变量名）。
- **理由**：配置系统的价值在于"文档写的键 = 代码读的键"。V1.6 给的是"另一套配置体系"，落地时要么被无视（文档失效），要么被迫写一个 TOML 加载层（多一套配置源）。

### C-5｜`rag_chunk` 与 `document_chunk` 双表并存 → 检索该查哪张？

- **原文**：§5.4 / §13.2 新建 `rag_chunk`；§8 开头「业务复用；`doc_meta`、`rag_chunk`、`tombstone`、`diff_repair_task` 保持不变」。
- **现状核对**：现有 `document_chunk`（`tenant_tables.sql:862`）挂在 `knowledge_asset(asset_id)` 上，带 `okf_type / tree_node_id / tags / title`，是**当前 `/ask` 问答唯一的数据源**（`rag_service.rs` 全部围绕它）。V1.6 的 `rag_chunk` 挂在 `doc_meta(doc_id)` 上，带 `source_type / start_sec / chunk_id / doc_version`。
- **问题**：两张表**语义高度重叠但主键体系不同**（`BIGINT asset_id` vs `TEXT doc_id`）。V1.6 全文没有一句说明：① 老数据怎么办；② 检索时查哪张；③ 前端"引用"返回的是 `document_chunk.id` 还是 `rag_chunk.chunk_id`；④ `knowledge_asset` 与 `doc_meta` 谁是谁的从属。**这是本文档最大的未定项**，直接决定 DDL 章节是否存在。
- **改为**：必须拍板（见 §9 决策 D2），三个选项：

  | 方案 | 做法 | 优点 | 代价 |
  |---|---|---|---|
  | **A：改造 `document_chunk`** | 给现有表加 `doc_id / doc_version / chunk_id / content_hash / logical_path / local_index / start_sec / page_no` 等列，`asset_id` 保留并与 `doc_meta` 建映射 | 只有一张表、存量数据不迁、`/ask` 与知识库共享一套 | 表变宽；`asset_id NOT NULL` 约束需放松或给纯文本文档造 `knowledge_asset` 影子行 |
  | **B（推荐）：新建 `rag_chunk`，双表并存 + 统一检索视图** | 新表承载增量体系；建 `v_rag_search` 视图 `UNION ALL` 两张表，`retrieve` 只查视图；老数据一次性回填 `rag_chunk` 后视图退化为单表 | 迁移风险最低、可灰度、可回滚；`document_chunk` 保持原样不动 | 短期内两套写入路径；需要 `RAG_USE_VECTOR` 特性开关隔离 |
  | C：全量替换 | 停写 `document_chunk`，数据迁移后 DROP | 最干净 | 不可回滚，风险最高，且 `document_chunk` 还被前端引用展示用 |

  > 本清单的 §7 DDL 按 **方案 B** 编写；若最终选 A 或 C，§7 需相应改写（A：全部改为 `ALTER TABLE`；C：需要一份数据迁移脚本 + 一次性停写窗口）。

- **理由**：这不是"实现细节"，是**决定整份文档能否开工的前置决策**。D2 未定就写 DDL，等于把返工写成交付物。

### C-6｜`TextChunker::estimate_tokens` 的算法会让 `chunk_size = 512` 严重失真

- **原文**（隐含）：§5.2「chunk：按 `file_type` 分发 `TextChunker` / `TimeChunker`」；§10 未给出 chunk_size 配置。
- **现状核对**（`rag_service.rs:73-84`）：
  ```rust
  /// 估算文本的 Token 数（中英文混合：中文约 1.5 字符/token，英文约 4 字符/token）
  fn estimate_tokens(&self, text: &str) -> i32 {
      let mut tokens = 0;
      for ch in text.chars() {
          if ch.is_ascii() { tokens += 1; } else { tokens += 2; }
      }
      (tokens / 4).max(1)
  }
  ```
- **问题**：注释说"中文约 1.5 字符/token"，代码给每个非 ASCII 字符记 2 分、最后整体 `/4` → 中文实际按 **0.5 token/字** 计算。真实的中文 tokenizer 大约是 **1 token/字**（甚至更多，取决于模型）。于是 `chunk_size = 512` 名义上是 512 token，实际切出来约 **1024 个汉字 ≈ 1000+ token**，**超预算约 2 倍**。
  另一个 bug：`overlap` 用的是 `chunk_overlap * 4` **字节**数（`rag_service.rs:49`），而 `current[overlap_start..]` 是对 `String` 按字节切片 —— 如果切点落在多字节字符中间，**会 panic**（`byte index is not a char boundary`）。
- **改为**：
  1. **字节切片必须按字符边界**：
     ```rust
     let overlap_start = current
         .char_indices()
         .rev()
         .nth(...)            // 或先算字节位置再用 char_boundary 校正
         .map(|(i, _)| i)
         .unwrap_or(0);
     ```
     更稳妥的写法：先收集 `Vec<(usize, char)>`，按"保留最后 N 个字符"取边界。
  2. `estimate_tokens` 二选一（与 D-8 联动）：
     - 选 D-8 ①：用 `text-splitter` 的真 tokenizer，`estimate_tokens` 直接删除；
     - 选 D-8 ②：改为 `tokens = ascii_chars/4 + cjk_chars`（CJK 按 1 token/字），并在 §5.2 写明这是**估算**，`token_count` 落库时标注 `estimate=true`。
  3. §10 新增 `[rag] chunk_size_tokens = 512` / `chunk_overlap_tokens = 64`（当前 overlap 128 相当于 25% 重叠，对 512 的块偏大，建议 64=12.5%）。
- **理由**：`chunk_size` 决定召回粒度与成本。估算偏 2 倍意味着：要么 prompt 超上下文被截断（丢内容），要么块太大导致召回不准。而**字节切片 panic** 是随时可能在生产上炸的崩溃点（只要文档里有中文 + 触发 overlap 分支）。

### C-7｜同仓已有《文档解析后向量化数据缓存方案》，结论与 §7.8 相反：LanceDB 主存储 vs pgvector 主存储

- **现象**：`docs/文档解析后向量化数据缓存方案.md`（26 KB，早于本清单存在，且是仓库里**唯一**一份讲缓存的文档）给出的架构是「**LanceDB 主读写存储 + pgvector 持久化归档 + Valkey 缓存 + 内存 LRU**」，本清单 §7.8 的结论是「**pgvector 唯一权威（冷层即热层）+ 热层延到 V1.8**」。逐项对照：
  | 维度 | 既有文档 | 本清单 |
  |---|---|---|
  | 主存储 | LanceDB（本地目录文件）`:9`、`:73-75` | pgvector `rag_chunk`（§7.2） |
  | pgvector 角色 | 归档 / 兜底，命中率「< 1%」、延迟「< 20ms」`:727` | 唯一权威，**所有**检索都走它（§7.3） |
  | 数据粒度 | 知识条目（`asset_knowledge.id`）`:83` | chunk（`chunk_id = sha256(doc_id + logical_path + local_index)`，B-1） |
  | 向量维度 | 固定 768 `:85` | 1536（B-2 统一后） |
  | 写入路径 | 先 LanceDB（同步）→ **异步**归档 PG `:460` | 单写 PG；异步只累加 `access_count`（§7.6 / §7.8 H） |
  | 一致性 | 最终一致 + 启动比对记录数 `:546` | 水位 + 抽样重算（§7.8 G / F-5） |
- **问题**：两份文档都在 `docs/` 下、都能被搜到、都"有出处"。后果不是"选一个"，而是**两个人各按一份实现**：写路径照既有文档写 LanceDB、检索路径照本清单查 pgvector → 「上传成功、解析成功、检索永远为空」。更糟的是它被设计成**最终一致**：归档任务失败一次就永久缺失，而 `tokio::spawn` 里 panic 不会有人知道。
  另外 LanceDB 有两个与本项目直接冲突的属性：① 它是**本地目录文件**（`:75` `{data_dir}/lancedb/`），而本项目是多租户 schema + 服务端多节点部署（G-1）；② 它**没有完整事务**（该文档自己在 `:24` 写「有限」），而 `doc_version` 版本共存（F-6）正依赖事务。
- **改为**：
  1. §7 **新增 §7.9**，把既有文档拆成「可直接引用 / 必须改造 / 必须否决」三堆，实现者只读 §7.9 就知道哪一段能抄；
  2. 既有文档**保留但加取代声明**：它的 Valkey 选型、缓存 key 的分类维度、命中率基线、软删除纪律仍然有效，只是「谁是权威」要倒过来；
  3. 依赖层面立一条可 grep 的红线：V1.7 的 `crates/*/Cargo.toml` 中出现 `lancedb` / `moka` / `valkey` 即为构建失败（§11.8 第 1 条），把"偷偷上热层"变成机器检查。
- **理由**：双主存储 = 双写一致性问题的完整形态；而这里的两个"主"还是**两个人分别按两份文档写的**，连双写都没实现，只是各写一半。文档层面的冲突不解决，代码层面的冲突是必然的。
- **验收**：§11.8 第 1、8 条；既有文档头部出现取代声明。

---

## 6. 结构性缺失（10 条，可带 TODO 进 V1.7.1）

> 本节不阻断 V1.7 开工，但**如果不写进文档，将来的实现一定各写一套**。每条给出"最少要补什么"。

### G-1｜全文没有"多租户 / schema 隔离"章节

- **现象**：V1.6 只有 `doc_meta.tenant_id TEXT` 一处体现租户概念（M-1），其余章节（DDL、检索 SQL、缓存 key、runledger 内部表、对账）**全部默认单租户**。
- **必须补**（§5.5.6，约半页）：
  1. 业务表 → **租户 schema 内**（`{schema}.doc_meta` 等），由 `init_tenant_tables` 建；
  2. `runledger` 内部表 → **独立 `runledger` schema**（没有租户概念，绝不能建在租户 schema 里，否则 N 个租户 = N 份调度队列，worker 无法共享）；
  3. 检索 SQL 的 schema 前缀必须走 `database::schema_prefix()`（现有约定，`rag_service.rs:96`），**禁止硬编码**；
  4. 缓存 key 必须带 `{schema}` 前缀（与 M-1、F-4 呼应）；
  5. 跨租户（超管视图）只能走 `docs/多租户超级管理员跨Schema查询设计方案.md` 的白名单机制。
- **为什么现在就必须写**：schema 前缀一旦写错，是**跨租户数据泄露**级别的事故，不是"功能不好用"。

### G-2｜桌面端（Tauri）与服务端（Docker）两种形态混在一份配置里

- **现象**：§10 同时给出 Patroni + PGBouncer + 独立对账进程（服务端），和 `[orchestrator] backend = simple_local`（桌面端）；§11 又要求"Tauri 桌面编译禁用 runledger"。
- **问题**：`Patroni` 是 PG 集群的**高可用组件**，对一个装在自己电脑上的 Tauri 桌面应用毫无意义；反过来，桌面端"用 SQLite 还是 PG"也没说（§3.2 只说"runledger 无 SQLite 支持"）。
- **必须补**（§3.3 部署形态）：一张两列对照表，明确"哪些能力在桌面端关闭"：

  | 能力 | 服务端 | 桌面端 |
  |---|---|---|
  | runledger DAG | ✅ | ❌（`SimpleLocalOrchestrator` / `tokio::spawn`） |
  | Patroni + PGBouncer + 只读副本 | ✅ | ❌（本地单实例 PG 或无 PG） |
  | 独立对账进程 + cron | ✅ | ❌（启动时一次性轻量校验） |
  | 蓝绿双索引 | ✅ | ❌（直接 ALTER + 全量重算） |
  | OCR/ASR 并发上限 | 由 CPU 核数决定 | 固定 1~2（笔记本散热） |

- **为什么**：不分离形态，就会出现"桌面端也在尝试连只读副本然后超时 30 秒"这类问题。

### G-3｜没有 OCR/ASR 的吞吐与容量预算

- **现象**：§11 只要求"基础RAG功能验收：全格式解析→入库→检索问答"，**没有任何性能/容量数字**。
- **已知事实**：现方案 OCR 走 `tract-onnx`（纯 Rust）。`tract` 是解释型推理引擎，**比 ONNX Runtime 慢一个数量级**（同模型约 5~20×）。一份 100 页扫描件，ORT 可能 60 秒、tract 可能 5~15 分钟。这直接影响：用户等待体验、并发解析上限（§10 `max_concurrent_parse = 2`）、超时阈值（`parse_timeout_sec = 600` 可能不够）与任务队列设计。
- **必须补**（§5.1.5 性能预算）：
  ```
  · 单页 A4 文本 PDF（纯提取）        P50 ≤ 50ms    P95 ≤ 200ms
  · 单页 A4 扫描件（300dpi + OCR）    P50 ≤ 6s      P95 ≤ 20s   ← tract 实测后填
  · 单页 A4 扫描件（对照：ORT）        P50 ≤ 1s      ← 仅作对比，不落地
  · 100 页扫描 PDF 端到端              ≤ 15min（含入库），超时阈值据此设
  · 并发：解析信号量 = min(CPU 核数, 4)；OCR 内部串行
  ```
  并把它接到 §10：`parse_timeout_sec` 改为**按页数 × 每页预算**动态计算（`max(120, pages * 20)` 秒），而不是固定 600。
- **为什么**：`tract` 的性能是**方案选型的代价**，必须量化后才谈得上"验收"。否则上线第一天就会被"解析一上午没出来"打回。

### G-4｜没有检索质量评测基线（只有解析层的 Jaccard）

- **现象**：§11「黄金对比测试：local vs remote，Jaccard≥0.9」—— 这是**解析层**的对比，与检索质量无关。整份文档没有：query 集合、相关文档标注、recall@k、MRR、NDCG。
- **问题**：`chunk_size` 改了、embedding 模型换了、加了 rerank、调了 `ef_search`……**这些改动的好坏无法判断**。B-3 修完之后，检索从"全 0 相似度"变成真向量检索，但"变好了多少"没人知道。
- **必须补**（§11.2 检索评测）：
  ```
  · 构建 ≥50 条 (question, expected_chunk_ids) 的标注集（可用现有知识库文档反推）
  · 指标：recall@8 ≥ 0.85、MRR ≥ 0.70、无结果率 ≤ 3%
  · 每次改 chunk_strategy_version / embedding_model_version 必须重跑并记录到 CI 产物
  · 冒烟门槛：改完 embedding 后 recall@8 下降 > 5% 视为回归，阻断合并
  ```
- **为什么**：RAG 系统的调参是**没有评测就没有优化**的典型。没有基线，所有"优化"都是猜。

### G-5｜§8.13「trace_id 从 HTTP 入口贯穿到 DAG 每步 handler」缺少落地方式

- **现象**：§8.13 承诺"统一注入 trace_id，从 HTTP 入口→runledger DAG 每步 handler→DB 读写全程携带"，但 `runledger` 是**外部库**，它的 worker 循环会不会透传 `tracing` span context，**V1.6 没核实**。
- **必须补**：
  1. 先做一次 10 分钟的验证：在 `runledger` 的 step handler 里 `info!()` 打印当前 `trace_id`，看能否取到提交端的 trace（若不能透传，则只能**自建关联**：submit 时把 `trace_id` 写进 job payload / `workflow_binding`，handler 里读出后 `span!(parent: None, "step", trace_id = %tid)`）。
  2. 明确 `trace_id` 的**落库字段**：`workflow_binding.trace_id VARCHAR(32)`（M-6 的 DDL 里未含，需补）。
  3. 明确 SLO 指标采集点（§8.12 的"单步 P95"从哪里来）：建议每步 handler 结束时写一条 `step_metrics(step_name, duration_ms, ok)`——这张表 V1.6 也没定义。
- **为什么**：可观测性最怕"设计得很好，实现时发现拿不到数据"。先验证 runledger 的 context 透传能力，再决定是"借它的"还是"自建"。

### G-6｜降级路径（B 计划）没有验收判据，只有"随 CI 持续测试"

- **现象**：§12.3「runledger 为 0.x，breaking-change｜高｜锁版本+季度评估+兼容性测试矩阵；orchestrator 抽象层一键降级 B 计划」；§11「调度抽象层验收：关闭 runledger feature，SimpleLocalOrchestrator 可跑通 rag_etl 基本流程」。
- **问题**："跑通基本流程"不是判据。真正需要的是：**降级后的能力缺口清单 + 数据一致性保证 + 切换 RTO**。
- **必须补**（§12.3 增补）：

  | 项 | 要求 |
  |---|---|
  | 能力缺口 | 无分布式多 worker（只能单机）、无 cron（改用 `tokio::time::interval`）、无 DLQ（改写入 `rag_failed_task` 表） |
  | 数据一致性 | 切换期间正在运行的 run：**必须等它们结束或标 `cancelled`**，不允许"半路换引擎" |
  | 切换 RTO | ≤ 1 次重启（≤ 2 分钟），因为切换只涉及编译 feature + 重启 |
  | 回滚 | 反向操作同样 ≤ 2 分钟 |
  | 演练频率 | **每季度一次**，写入运维手册，不是"随 CI" |

- **为什么**：B 计划的价值全在"出事那天能不能用"。没演练过的 B 计划等于没有。

### G-7｜§9 安全章节与真实架构有偏差（Windows 桌面 / Tauri IPC / 审计表）

- **现象与核对**：
  | §9 的表述 | 现实 |
  |---|---|
  | 「AXUM 接口启用 JWT 令牌鉴权」 | `crates/api` 确实用 `axum 0.8.9` + `jsonwebtoken 11`（`crates/api/Cargo.toml:11,35`），✅ 成立 |
  | 「解析进程隔离：独立子进程 + Landlock 沙箱」 | `crates/parser` 是**进程内库**（`lib.rs:73` 直接 `parse_file`），没有子进程边界；**Landlock 只存在于 Linux**，Windows 桌面端没有对应实现（AppContainer / Job Object 未提） |
  | 「所有文件解析、入库、删除操作写入不可篡改审计日志」 | 仓库里**没有审计日志表**（`tenant_tables.sql` 全文无 `audit` 相关表），也没说"不可篡改"如何实现（append-only？哈希链？） |
  | 「存储：PG 字段级加密（文档正文、敏感元数据）」 | `knowledge_asset.content TEXT` / `document_chunk.chunk_text TEXT` 都是明文；PG 原生不支持字段级加密（需 `pgcrypto` 或应用层） |
- **必须补**（§9.0 安全设计前提）：
  1. 明确"**进程内解析**是现状，子进程隔离是 V1.8 目标"；Windows 侧给出替代方案（`Job Object` 限制内存 + `CreateProcess` 超时 kill），或者干脆承认"只在服务端做隔离"。
  2. 审计日志**要么建表要么删掉这句话**：建表 = `{schema}.audit_log(id, trace_id, actor_id, action, target_type, target_id, payload, prev_hash, hash, created_at)`（哈希链）；删掉则改为"依赖 tracing 结构化日志 + 外部日志平台"。
  3. 字段级加密改为可执行方案：应用层 AES-GCM（密钥来自系统钥匙串）+ 只加密 `permission_json` 这类敏感小字段；**不要**加密 `chunk_text`（会让向量检索无法做关键词兜底，且性能不可接受）。
- **为什么**：安全章节最怕"看起来做了"。上面四条里有三条目前是**不存在的**，写进文档等于给审计一个错误印象。

### G-8｜对象存储与临时文件的生命周期完全没有定义（孤儿对象 / staged 残留 / 临时目录）

- **现象与核对**：
  | 资产 | 现状 | 后果 |
  |---|---|---|
  | S3 对象 | `clean_expired` 只清 `status IN ('pending','uploading')`（`upload.rs:731-745`） | `completed` 但从未 `commit` 的记录**永不回收**：用户传了 2GB 视频后关页面 → 对象永久泄漏 |
  | `completed` 之后的取消入口 | `abort` 要求 `status = 'uploading'`（`upload.rs:671-676`） | complete 之后**没有任何"放弃"通道**，只能靠定时任务兜底 |
  | `rag_chunk` 的 staged 版本 | 投机解析（§7.7）会先写 chunk、后放行 | 放弃保存时残留"已解析但永不生效"的向量，既占表也占 HNSW 索引 |
  | 解析临时目录 | 只在 `docs/知识库模块/文档解析与RAG记忆链路设计方案.md:190-206` 有定义，**V1.6 正文没有** | 解析失败时 GB 级临时文件堆积，Windows 桌面端吃满 C 盘 |
- **必须补**（写进 §12.3 风险表 + §7.7）：
  1. `purge_staged` 定时任务：`status='completed' AND commit_at IS NULL AND updated_at < now() - 24h` → 依次「写 tombstone → 删 staged 版本 chunk → 清 `staged_doc_version` → 软删 `file_uploads` → `delete_object`」（SQL 见 §7.6 step 6g）。**先 DB 后 S3**，S3 删除可重试。
  2. `file_uploads` 的状态机要允许"complete 之后放弃"（`status='cancelled'`），不再只限于 `uploading`。
  3. 临时目录用 RAII guard（`Drop`）删除，**解析失败路径也要删**；目录名带 `job_uuid`，便于异常退出后按前缀清理。
- **为什么**：这三类资产都**没有用户可见的症状**，泄漏只会在半年后的磁盘告警里出现。文档不写，就没人会做。

### G-9｜冷热分层的运维节奏全部缺失（热度统计 / 下沉 / 回热 / 索引重建 / 校验调度）

- **现象与核对**：
  | 缺失项 | 现状 | 后果 |
  |---|---|---|
  | 热度统计载体 | `doc_meta` 与 `rag_chunk` 都没有 `access_count` / `last_accessed_at` | 没有数据就无法判定冷热，分层只能靠猜 |
  | 下沉 / 回热任务 | 无 | 热层只增不减，内存被吃满；或者干脆不做热层 |
  | 索引碎片维护 | B-7 提到 HNSW 部分索引，但没有重建策略 | 高频增量写后召回率**静默下降**（延迟涨、召回掉，监控上看不出来） |
  | 全量校验调度 | §7.4 建了 `diff_repair_task` 表，但没有调度方与频率 | 工单表建了没人写，等于没有对账 |
  | Embedding 升级后的热层清理 | §8.2 只讲了维度迁移与蓝绿索引 | 热层残留旧模型向量 → 比不重建更糟（新旧向量不在同一语义空间） |
  | 仓库现状 | 8 个 crate 与根 `Cargo.toml` 中**没有** `redis` / `valkey` / `lancedb` / `qdrant` / `milvus` / `moka` 任何依赖；进程内只有 `dashmap 6.1.0` | 「热层」当前是零实现，不能按既有能力写文档 |
- **必须补**（写进 §7.8 + §12.3 风险表）：
  1. `doc_meta` 增 `access_count BIGINT` / `last_accessed_at TIMESTAMPTZ`（**文档级**热度，§7.1 已加）。**不做 chunk 级计数**：检索热路径上逐条 `UPDATE` 会把 P95 从毫秒推到几十毫秒，还会和 HNSW 写路径争锁；聚合到文档级足够支撑「整篇下沉 / 回热」；
  2. 三个定时任务：`demote_hot`（每小时）、`promote_on_hit`（惰性，命中时判定，不额外扫表）、`index_maintenance`（每周 + 阈值触发 `REINDEX CONCURRENTLY`）；
  3. `consistency_check` 的调度频率与责任人（建议每日增量、每周全量），且**源侧读主库**（F-5）。
- **为什么**：分层的收益**全在运维节奏里**。没有调度，冷热分层只是两套存储并存 —— 成本翻倍、收益为零，最后一定会被回滚。

### G-10｜缓存层的容量 / TTL / 失效 / 预热 / 命中率基线全部未定义

- **现象与核对**：
  | 缺失项 | 现状 | 后果 |
  |---|---|---|
  | 进程内缓存容量 | 只有 `dashmap`（`crates/database/Cargo.toml:18`），**无淘汰策略** | 无界增长：桌面端连开一周，内存只增不减 |
  | TTL / TTI | 未定义 | 旧 `doc_version` 的 query 向量与结果长期驻留，与 M-12 的失效规则脱节 |
  | 失效时机 | F-4 只规定了「key 要带版本」，没说「谁在什么时候清」 | 蓝绿切换、权限变更后残留脏 key |
  | 预热策略 | 未定义 | 冷启动首查延迟是常态的数倍，且**每次都一样慢**（缓存自愈不了） |
  | 命中率 / 延迟基线 | 未定义 | 无法判断缓存是否值得，**也无法给 POC-6 提供对照组** |
  | 可参考的现成数字 | 既有文档 `:682`（`memory_cache_size = 1000` / `search_cache_ttl = 300`）与 `:727`（命中率 60-80% / 95%+ / 70-90% / <1%） | 这些是**预估**不是实测，只能当起点，不能当验收标准 |
- **必须补**（写进 §7.8 H 的配置表 + §11.8 第 4、5 条）：
  1. 三个配置键（已并入 §7.8 H）：`local_cache_max_entries = 2000` / `local_cache_ttl_sec = 60` / `local_cache_tti_sec = 300`。容量按**条数**不按字节：单条 ≈ 1536×4B 向量 + 文本 ≈ 8 KB，2000 条 ≈ 16 MB 上限，桌面端可接受；
  2. 失效规则三条，与 M-12 的 key 一一对应：换版（`doc_version` 变 → key 变，无需主动清）、蓝绿切换（按 `rag:` 前缀 flush）、权限变更（`perm_hash` 变 → key 变）。**明确不做**「按 `doc_id` 精准清」——多节点无共享内存，做不到，写成"支持"就是假的；
  3. 缓存只放**不可变产物**：query 向量、hits 的 `(chunk_id, score)` 列表。**不放 chunk 正文** —— 正文随版本变，且会把内存占用放大一个数量级；命中后按 `chunk_id` 回表取正文（§7.3 的 SQL 已按 `chunk_id` 聚合，不需要改）；
  4. 预热：V1.7 桌面端**不做**启动预热（反而拖慢启动），只在首次检索后回填；集群版（V1.8）可预热 top-100 `doc_id`；
  5. 监控三个指标：命中率、缓存条目数、命中 vs 回表的 P95 差。**第三个指标是 POC-6 判断「热层值不值得」的唯一基准**。
- **为什么**：缓存的收益 = 命中率 × 单次节省。没有基线，「要不要上热层、要不要加缓存」就只能靠感觉，最后的常见结局是**用一致性风险换了一个测不出来的收益**。
- **验收**：§11.8 第 4、5 条。

---

## 7. 修正后的完整 DDL 与存储分层契约（V1.7 · 按方案 B）

> **落地方式**：新建 `crates/database/src/sql/rag_migration.sql`，沿用现有约定 —— `{schema}` 占位符由 `execute_sql_content_with_schema` 替换（`postgres.rs:716-724`），全部语句**幂等**（`IF NOT EXISTS` / `ON CONFLICT DO NOTHING`），并在 `postgres.rs` 增加 `migrate_tenant_rag_tables(pool, schema)` + `migrate_all_tenant_rag_tables(pool)`，照抄 `migrate_tenant_workflow_tables` / `migrate_all_tenant_workflow_tables`（`postgres.rs:751-775`）的写法，挂到启动流程第 11 步之后。
>
> **前置条件**：**PostgreSQL 扩展** `pgvector >= 0.5.0`（HNSW 的最低要求），生产推荐 `>= 0.7`；**Rust crate** 用 `pgvector = "0.4"`（最新 0.4.2）。⚠️ 这两者是**两套完全独立的版本号**（扩展 0.7/0.8 ≠ crate 0.4.2），本文凡写「pgvector 0.4」指 crate、写「pgvector ≥ 0.5.0」指扩展；`sqlx` 需启用 `json` feature。
>
> **与方案 A/C 的差异**：本 DDL 假设 `rag_chunk` 与现有 `document_chunk` **并存**（C-5 方案 B）。若最终选 A，则全部改为对 `document_chunk` 的 `ALTER TABLE`；选 C 则另需数据迁移脚本。

### 7.1 `doc_meta` —— 文档主元数据

```sql
-- ============================================================
-- 1. doc_meta 文档主元数据（RAG 增量维护体系的根）
-- ============================================================
CREATE TABLE IF NOT EXISTS {schema}.doc_meta (
    doc_id                  TEXT PRIMARY KEY,              -- sha256(带长度前缀拼接)，见 M-2
    tenant_id               BIGINT,                        -- 冗余审计；隔离靠 schema，见 M-1
    source_system           VARCHAR(64)  NOT NULL,
    source_doc_id           VARCHAR(512) NOT NULL,
    source_uri              TEXT,                          -- s3://{bucket}/{object_key}，见 M-10
    source_rev              BIGINT,                        -- 源版本 = file_uploads.version，见 M-10
    source_etag             VARCHAR(255),                  -- S3 ETag：对象内容指纹，见 M-10
    source_file_name        VARCHAR(512),
    source_file_type        VARCHAR(32),                   -- pdf/document/image/audio/video/text
    content_global_hash     CHAR(64),                      -- 整文规范化哈希：不变则整篇跳过
    doc_version             BIGINT NOT NULL DEFAULT 1,
    active_doc_version      BIGINT,                        -- 当前生效版本（F-6 方案①）；NULL=尚无生效版本
    staged_doc_version      BIGINT,                        -- 已解析待放行版本（§7.7 投机解析）；检索永不读它
    status                  VARCHAR(20) NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','processing','staged','active','failed','deleting','deleted','expired')),
    expire_time             TIMESTAMPTZ,
    permission_json         JSONB NOT NULL DEFAULT '{}'::jsonb,
    embedding_model_version VARCHAR(64),
    chunk_strategy_version  VARCHAR(32),
    access_count            BIGINT NOT NULL DEFAULT 0,     -- 热度：被检索命中的次数（文档级，§7.8）
    last_accessed_at        TIMESTAMPTZ,                   -- 热度：最近一次被检索命中时间；NULL=从未命中
    last_error              TEXT,
    created_by              BIGINT,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_by              BIGINT,
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted                 SMALLINT NOT NULL DEFAULT 0,
    CONSTRAINT uk_doc_meta_source UNIQUE (source_system, source_doc_id)
);

COMMENT ON TABLE  {schema}.doc_meta IS 'RAG 文档主元数据（增量维护体系根）';
COMMENT ON COLUMN {schema}.doc_meta.doc_id             IS '文档逻辑唯一ID：sha256(带长度前缀拼接)，内容修改不变';
COMMENT ON COLUMN {schema}.doc_meta.tenant_id          IS '租户ID（与 public.sys_tenant.id 同源）；仅审计，隔离由 schema 保证';
COMMENT ON COLUMN {schema}.doc_meta.doc_version        IS '版本号：每次 claim 自增，用于乐观锁与旧任务回写防护';
COMMENT ON COLUMN {schema}.doc_meta.active_doc_version IS '当前对外生效的版本；检索与展示只看该版本';
COMMENT ON COLUMN {schema}.doc_meta.staged_doc_version IS '已解析待放行的版本（投机解析）；**检索 SQL 禁止引用本列**，只有 commit 放行时才提升为 active_doc_version';
COMMENT ON COLUMN {schema}.doc_meta.source_rev         IS '源版本号：对应 file_uploads.version；非上传来源为 NULL';
COMMENT ON COLUMN {schema}.doc_meta.status             IS '对外可见性：pending/processing/staged 均不可检索；active 可检索；deleting/deleted/expired 不可检索';
COMMENT ON COLUMN {schema}.doc_meta.access_count       IS '热度统计：被检索命中的次数（文档级聚合，异步批量累加，禁止在检索热路径逐条 UPDATE）';
COMMENT ON COLUMN {schema}.doc_meta.last_accessed_at   IS '热度统计：最近一次被检索命中的时间；冷热下沉判据（§7.8 D）';
COMMENT ON COLUMN {schema}.doc_meta.last_error         IS '最近一次失败原因（status=failed 时必有值）';

CREATE INDEX IF NOT EXISTS idx_doc_meta_status ON {schema}.doc_meta(status, deleted);
CREATE INDEX IF NOT EXISTS idx_doc_meta_expire ON {schema}.doc_meta(expire_time) WHERE status = 'active';
CREATE INDEX IF NOT EXISTS idx_doc_meta_source ON {schema}.doc_meta(source_system, source_doc_id);
-- 解析队列的部分索引：worker 用 `WHERE status='pending' ORDER BY created_at FOR UPDATE SKIP LOCKED` 取任务（§7.7 D）
CREATE INDEX IF NOT EXISTS idx_doc_meta_pending ON {schema}.doc_meta(created_at) WHERE status = 'pending';
-- 冷热下沉候选扫描（§7.8 D）：只扫 active 且未删除，供 demote_hot 使用
CREATE INDEX IF NOT EXISTS idx_doc_meta_access ON {schema}.doc_meta(last_accessed_at NULLS FIRST) WHERE status = 'active' AND deleted = 0;
```

> **相比 V1.6 的改动**：`tenant_id TEXT → BIGINT`（M-1）；新增 `source_file_name / source_file_type`（M-9 的来源冗余）、`source_rev / source_etag`（M-10 的源锚点）、`active_doc_version`（F-6）、`staged_doc_version`（§7.7）、`last_error`、`deleted`（与全库 `deleted SMALLINT` 约定一致）、`uk_doc_meta_source`（防同源重复入库）、`idx_doc_meta_pending`（解析队列）、`access_count / last_accessed_at` 与 `idx_doc_meta_access`（G-9 热度统计，供 §7.8 冷热判定）。
>
> ⚠️ **三条 schema 级铁律**（写进 §5.4 与 code review checklist）：
> 1. **`status` 只表达"对外可见性"**，不表达"处理进度"（F-8）。进度看 `workflow_binding`。
> 2. **检索 SQL 只允许 `JOIN doc_meta ON c.doc_version = m.active_doc_version`**，禁止出现 `staged_doc_version`（§7.3 已按此写）。
> 3. **热度字段（`access_count` / `last_accessed_at`）只由异步任务写**，检索 SQL 只读不写 —— 在检索路径上执行一次 `UPDATE doc_meta`，会把 P95 从毫秒推到几十毫秒（G-9）。

### 7.2 `rag_chunk` —— 分片 + 向量

```sql
-- ============================================================
-- 2. rag_chunk 分片与向量（版本共存，见 F-6 方案①）
-- ============================================================
CREATE TABLE IF NOT EXISTS {schema}.rag_chunk (
    id                      BIGSERIAL PRIMARY KEY,
    chunk_id                TEXT NOT NULL,                  -- 方案B：sha256(doc_id+logical_path+local_index)
    doc_id                  TEXT NOT NULL REFERENCES {schema}.doc_meta(doc_id) ON DELETE CASCADE,
    doc_version             BIGINT NOT NULL,
    logical_path            TEXT NOT NULL DEFAULT '',       -- 标题层级锚点，如 "1/2/0"，见 B-1
    local_index             INT  NOT NULL DEFAULT 0,        -- heading_path 内序号
    chunk_index             INT  NOT NULL,                  -- 文档全局序号（仅展示排序）
    source_type             VARCHAR(16) NOT NULL
        CHECK (source_type IN ('text','pdf','document','image','audio','video')),
    text                    TEXT,
    image_path              TEXT,
    page_no                 INT,                            -- 来源页码（M-9）
    file_name               VARCHAR(512),                   -- 冗余，避免热路径 JOIN
    start_ms                BIGINT,                         -- 音视频时间轴，毫秒（M-8）
    end_ms                  BIGINT,
    content_hash            CHAR(64) NOT NULL,              -- 规范化文本 sha256（M-3）
    token_count             INT,                            -- M-7
    char_len                INT,                            -- M-7
    text_vector             vector(768),                    -- B-2：nomic-embed-text v1.5
    image_vector            vector(512),                    -- B-8 选②/③才有值，否则恒 NULL
    embedding_model_version VARCHAR(64) NOT NULL,
    chunk_strategy_version  VARCHAR(32) NOT NULL,
    is_deleted              BOOLEAN NOT NULL DEFAULT false,
    metadata                JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uk_rag_chunk_pos UNIQUE (chunk_id, doc_version)   -- 逻辑位置+版本（F-6）
);

COMMENT ON TABLE  {schema}.rag_chunk IS 'RAG 分片与向量（按 doc_version 版本共存，支持回滚）';
COMMENT ON COLUMN {schema}.rag_chunk.chunk_id     IS '逻辑位置ID：sha256(doc_id+logical_path+local_index)，内容变化不变';
COMMENT ON COLUMN {schema}.rag_chunk.local_index  IS '方案B 的 local_index：heading_path 内序号';
COMMENT ON COLUMN {schema}.rag_chunk.content_hash IS '规范化文本 sha256；未变则跳过 embedding 调用';
COMMENT ON COLUMN {schema}.rag_chunk.start_ms     IS '音视频片段起始毫秒；文档类为 NULL';
COMMENT ON COLUMN {schema}.rag_chunk.text_vector  IS '768维文本向量；必须与 sys_system_config.vector_dim_text 一致';

-- ---- 普通索引 ----
CREATE INDEX IF NOT EXISTS idx_rag_chunk_doc_ver ON {schema}.rag_chunk(doc_id, doc_version);
CREATE INDEX IF NOT EXISTS idx_rag_chunk_live    ON {schema}.rag_chunk(doc_id) WHERE is_deleted = false;
CREATE INDEX IF NOT EXISTS idx_rag_chunk_ver_created ON {schema}.rag_chunk(doc_version, created_at);

-- ---- 向量索引（部分索引：已删除的行不进索引，见 B-7）----
CREATE INDEX IF NOT EXISTS idx_rag_chunk_text_vec
    ON {schema}.rag_chunk USING hnsw (text_vector vector_cosine_ops)
    WITH (m = 16, ef_construction = 64)
    WHERE text_vector IS NOT NULL AND is_deleted = false;

CREATE INDEX IF NOT EXISTS idx_rag_chunk_image_vec
    ON {schema}.rag_chunk USING hnsw (image_vector vector_cosine_ops)
    WITH (m = 16, ef_construction = 64)
    WHERE image_vector IS NOT NULL AND is_deleted = false;
```

> **相比 V1.6 的改动**：唯一键 `chunk_id` → `(chunk_id, doc_version)`（F-6）；新增 `logical_path / local_index / chunk_index / content_hash / token_count / char_len / page_no / file_name / start_ms / end_ms`（B-1、M-3、M-7、M-8、M-9）；`start_sec/end_sec FLOAT8` → `start_ms/end_ms BIGINT`；HNSW 全部改为**部分索引**（B-7）；补 `CHECK` 与逐字段 `COMMENT`（与现有 SQL 文件风格一致）。

### 7.3 检索 SQL（唯一入口，替换现有 `0.0 AS similarity` 实现）

```sql
-- $1 查询向量（pgvector::Vector 或 $1::vector 文本）
-- $2 知识树根节点（可空）  $3 okf_type 过滤（可空）  $4 top_k  $5 min_similarity
WITH RECURSIVE sub_tree AS (
    SELECT id FROM {schema}.knowledge_tree WHERE id = $2::BIGINT AND deleted = 0
    UNION ALL
    SELECT kt.id FROM {schema}.knowledge_tree kt
    JOIN sub_tree st ON kt.parent_id = st.id
    WHERE kt.deleted = 0
)
SELECT
    c.chunk_id,
    c.text,
    c.chunk_index,
    c.page_no,
    c.file_name,
    c.start_ms,
    c.end_ms,
    m.doc_id,
    m.status,
    1 - (c.text_vector <=> $1::vector) AS similarity      -- 真实余弦相似度
FROM {schema}.rag_chunk c
JOIN {schema}.doc_meta  m ON m.doc_id = c.doc_id
WHERE c.is_deleted = false
  AND c.text_vector IS NOT NULL
  AND m.status = 'active'
  AND m.deleted = 0
  AND m.active_doc_version = c.doc_version                -- 只看生效版本（F-6）
  AND ($2::BIGINT IS NULL OR m.tree_node_id IN (SELECT id FROM sub_tree))
ORDER BY c.text_vector <=> $1::vector                     -- 走 HNSW 索引
LIMIT $4 * 4;                                             -- ×4 过采样（B-7）
-- 应用层再过一次 1-(<=>) >= $5 的阈值 + max_tokens 预算截断
```

> **说明**：
> 1. `ORDER BY` 必须是裸 `c.text_vector <=> $1`（**不要**包 `1 - (...)`），否则 pgvector 用不上索引；
> 2. `m.tree_node_id` 在 `doc_meta` 上不存在 —— 若检索仍需按知识树过滤，需给 `doc_meta` 加 `tree_node_id BIGINT`（**这一列 V1.6 漏了**，与 `document_chunk.tree_node_id` 对齐即可）；
> 3. 过采样 ×4 后的阈值过滤与 `max_tokens` 截断在 Rust 侧完成，并写 `metadata.degraded` 标记。

### 7.4 `tombstone` / `workflow_binding` / `diff_repair_task`

```sql
-- ============================================================
-- 3. tombstone 删除墓碑（落后更新拦截）
-- ============================================================
CREATE TABLE IF NOT EXISTS {schema}.tombstone (
    id            BIGSERIAL PRIMARY KEY,
    doc_id        TEXT NOT NULL,
    event_version BIGINT NOT NULL,
    event_type    VARCHAR(20) NOT NULL
        CHECK (event_type IN ('delete','expire','purge','restore')),
    operator_id   BIGINT,
    reason        TEXT,
    create_time   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uk_tombstone_doc_ver UNIQUE (doc_id, event_version)   -- M-5
);
COMMENT ON TABLE {schema}.tombstone IS '删除墓碑：高于本次版本的墓碑存在时拒绝写入';
CREATE INDEX IF NOT EXISTS idx_tombstone_doc_ver ON {schema}.tombstone(doc_id, event_version DESC);

-- ============================================================
-- 4. workflow_binding run_id ↔ doc_id 映射 + 业务侧运行状态
-- ============================================================
CREATE TABLE IF NOT EXISTS {schema}.workflow_binding (
    id            BIGSERIAL PRIMARY KEY,
    run_id        TEXT NOT NULL UNIQUE,
    doc_id        TEXT NOT NULL,
    doc_version   BIGINT NOT NULL,
    workflow_type VARCHAR(32) NOT NULL
        CHECK (workflow_type IN ('rag_etl','consistency_check')),
    status        VARCHAR(20) NOT NULL DEFAULT 'running'
        CHECK (status IN ('running','succeeded','failed','cancelled')),
    trace_id      VARCHAR(32),                              -- G-5
    started_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at   TIMESTAMPTZ,
    error_msg     TEXT
);
COMMENT ON TABLE {schema}.workflow_binding IS 'runledger run_id 与业务 doc_id 映射；同时承载业务侧运行状态（B计划无TUI时用）';
-- DB 层互斥兜底（M-6）：同一文档同一版本同一工作流只允许一个活跃 run
CREATE UNIQUE INDEX IF NOT EXISTS uk_wf_binding_active
    ON {schema}.workflow_binding(doc_id, doc_version, workflow_type)
    WHERE status = 'running';
CREATE INDEX IF NOT EXISTS idx_wf_binding_doc ON {schema}.workflow_binding(doc_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_wf_binding_trace ON {schema}.workflow_binding(trace_id);

-- ============================================================
-- 5. diff_repair_task 对账差异工单
-- ============================================================
CREATE TABLE IF NOT EXISTS {schema}.diff_repair_task (
    id               BIGSERIAL PRIMARY KEY,
    check_run_id     TEXT NOT NULL,
    doc_id           TEXT NOT NULL,
    chunk_id         TEXT NOT NULL,
    src_content_hash CHAR(64),
    tgt_content_hash CHAR(64),
    diff_type        VARCHAR(20) NOT NULL
        CHECK (diff_type IN ('src_only','tgt_only','hash_mismatch')),
    status           VARCHAR(20) NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','fixed','ignored')),
    note             TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at      TIMESTAMPTZ,
    CONSTRAINT uk_diff_repair UNIQUE (check_run_id, doc_id, chunk_id, diff_type)   -- M-4
);
CREATE INDEX IF NOT EXISTS idx_diff_repair_check_run ON {schema}.diff_repair_task(check_run_id, status);
```

### 7.5 存量表最小修复（与方案选择无关，无论 A/B/C 都要做）

```sql
-- ============================================================
-- 6. document_chunk 存量表修复（B-4 / B-6 / M-6 姊妹项）
-- ============================================================
-- 6.1 补唯一约束：现有表完全没有唯一约束，"重新向量化"会产生重复分片
CREATE UNIQUE INDEX IF NOT EXISTS uk_chunk_asset_idx
    ON {schema}.document_chunk(asset_id, chunk_index)
    WHERE deleted = 0;

-- 6.2 补向量模型版本列（换模型后可识别哪些行需要重算）
ALTER TABLE {schema}.document_chunk
    ADD COLUMN IF NOT EXISTS embedding_model_version VARCHAR(64) NOT NULL DEFAULT 'legacy';

-- 6.3 补 HNSW 索引（注释里一直声称有，实际没有 —— B-6）
CREATE INDEX IF NOT EXISTS idx_chunk_embedding
    ON {schema}.document_chunk USING hnsw (embedding vector_cosine_ops)
    WITH (m = 16, ef_construction = 64)
    WHERE embedding IS NOT NULL AND deleted = 0;
```

> **注意**：`document_chunk.embedding` 是 `vector(1536)`。若按 B-2 把文本向量统一到 768 维，**必须**先迁移维度：
> ```sql
> -- 前提：现有 embedding 数据可以丢弃（当前代码根本没写入过向量，见 B-4）
> ALTER TABLE {schema}.document_chunk
>     ALTER COLUMN embedding TYPE vector(768) USING NULL;
> ```
> 若库里已有真实 1536 维数据，则**不能**用 `USING NULL`，必须走 §8.2 的"加新列 + 回填 + 切名"三步。

### 7.6 关键写路径 SQL 模板（把 F-1/F-2/F-3/F-6/F-7/F-8 的结论固化成语句）

```sql
-- ============ step -1：upload_complete 钩子（幂等；F-7 / M-10 / §7.7）============
-- 前置：CompleteMultipartUpload 已成功 + head_object 自检通过（对象 size == file_uploads.file_size）
-- doc_id 在 upload_init 之后即可算出：sha256(长度前缀拼接(tenant_id,'upload',file_group_id))（M-2）
--   所以"解析早于业务绑定"在技术上是可行的 —— 这是 §7.7 投机解析的前提
-- 参数：$1 doc_id   $2 tenant_id   $3 file_group_id   $4 file_uploads.version
--       $5 file_uploads.etag      $6 s3://{bucket}/{object_key}
--       $7 original_filename      $8 文件类型     $9 created_by
INSERT INTO {schema}.doc_meta
    (doc_id, tenant_id, source_system, source_doc_id, source_rev, source_etag,
     source_uri, source_file_name, source_file_type,
     doc_version, active_doc_version, staged_doc_version, status,
     created_by, created_at, updated_at)
VALUES ($1, $2, 'upload', $3, $4, $5,
        $6, $7, $8,
        1, NULL, NULL, 'pending',
        $9, NOW(), NOW())
ON CONFLICT (source_system, source_doc_id) DO UPDATE
   SET source_rev       = EXCLUDED.source_rev,
       source_etag      = EXCLUDED.source_etag,
       source_uri       = EXCLUDED.source_uri,
       source_file_name = EXCLUDED.source_file_name,
       updated_at       = NOW()
 WHERE {schema}.doc_meta.status IN ('pending','staged','active','failed');
-- 冲突但 status ∈ (deleting,deleted,expired) → WHERE 不成立 → 0 行
--   → 应用层返回「文档已删除，请重建」，而不是静默把已删文档复活
-- 幂等性：前端重试 / 网关重放只会刷新 source_* 四列，不会新建第二行、不会触发第二次解析
-- 换版：行已存在且 status='active' → 只更新 source_rev/etag/uri，status 与 active_doc_version 不动

-- ============ step 0：claim（只做版本 CAS，**不改 status** —— F-8）============
-- 返回新版本号 N；0 行 = 被别人抢先，直接放弃本次任务
UPDATE {schema}.doc_meta
   SET doc_version             = doc_version + 1,
       embedding_model_version = $2,
       chunk_strategy_version  = $3,
       updated_at              = NOW()
 WHERE doc_id = $1
   AND doc_version = $4              -- 期望版本（乐观锁）
   AND deleted = 0
RETURNING doc_version;               -- → N
-- ⚠️ 这里**故意不写 status**：换版时 status 必须保持 'active'
--    否则"用户点替换 → 旧版本立刻消失 → 新版本解析完才回来"，召回中断（F-8 / B-7）
-- 互斥的主手段就是上面这条 CAS：并发 claim 只有一个能 UPDATE 成功（行锁 + 版本等值）
-- 兜底才是 uk_wf_binding_active，但它含 doc_version 列，只能兜住"同一版本重复登记"：
INSERT INTO {schema}.workflow_binding (run_id, doc_id, doc_version, workflow_type, status, trace_id)
VALUES ($5, $1, $6, 'rag_etl', 'running', $7)
ON CONFLICT DO NOTHING;              -- 0 行 = 已登记过，记 claim_dup 指标后继续（不阻断）

-- ============ step 4：diff（双键比对四类，M-3）============
-- 输入：本次解析出的 (chunk_id, content_hash) 集合（应用层传数组）
SELECT c.chunk_id, c.content_hash
  FROM {schema}.rag_chunk c
 WHERE c.doc_id = $1
   AND c.doc_version = (SELECT active_doc_version FROM {schema}.doc_meta WHERE doc_id = $1)
   AND c.is_deleted = false
   AND c.chunk_id = ANY($2::text[]);
-- 应用层据此算出 unchanged / changed / added / removed（removed = 旧集合 - 新集合）

-- ============ step 6：store（只写 changed+added，版本门控，F-2）============
INSERT INTO {schema}.rag_chunk
    (chunk_id, doc_id, doc_version, logical_path, local_index, chunk_index,
     source_type, text, image_path, page_no, file_name, start_ms, end_ms,
     content_hash, token_count, char_len, text_vector,
     embedding_model_version, chunk_strategy_version, metadata)
SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
       $14, $15, $16, $17::vector, $18, $19, $20::jsonb
  FROM {schema}.doc_meta m
 WHERE m.doc_id = $2
   AND m.doc_version = $3            -- ← 版本门控：只允许写入"当前最新版本"的块
ON CONFLICT (chunk_id, doc_version) DO UPDATE
   SET text        = EXCLUDED.text,
       content_hash= EXCLUDED.content_hash,
       text_vector = EXCLUDED.text_vector,
       token_count = EXCLUDED.token_count,
       metadata    = EXCLUDED.metadata,
       is_deleted  = false,
       updated_at  = NOW();
-- 0 行 = 版本已被更新（旧任务晚到），记录 skip_stale_write 指标，正常结束不重试

-- ============ step 6b：标记消失的 chunk（removed）============
UPDATE {schema}.rag_chunk
   SET is_deleted = true, updated_at = NOW()
 WHERE doc_id = $1 AND doc_version = $2
   AND chunk_id = ANY($3::text[])
   AND is_deleted = false;

-- ============ step 6e：登记 staged 版本（解析 + 校验通过后，§7.7）============
-- 新文档（从未生效）：status → 'staged'，commit 放行前不可检索
-- 换版文档（已有 active）：**status 保持 'active'**，旧版本继续对外服务 —— 这是本方案最大收益
UPDATE {schema}.doc_meta
   SET staged_doc_version  = $2,
       status              = CASE WHEN active_doc_version IS NULL THEN 'staged' ELSE 'active' END,
       content_global_hash = $3,
       last_error          = NULL,
       updated_at          = NOW()
 WHERE doc_id = $1
   AND doc_version = $2;             -- ← 版本门控：旧任务晚到 → 0 行

-- ============ step 6f：放行（由 upload_commit 在**同一事务**内触发，§7.7）============
-- 0 行 = 已经放过 / 本次没有待放行版本 → 幂等成功（前端重复点"保存"安全）
UPDATE {schema}.doc_meta
   SET active_doc_version = staged_doc_version,
       staged_doc_version = NULL,
       status             = 'active',
       updated_at         = NOW()
 WHERE doc_id = $1
   AND staged_doc_version IS NOT NULL;

-- ============ step 6g：放弃保存的回收（purge_staged 定时任务，G-8 / §7.7 E）============
-- 候选集：已落 S3，但 24h 内始终没有 commit
SELECT f.id, f.bucket, f.object_key, m.doc_id, m.staged_doc_version
  FROM {schema}.file_uploads f
  JOIN {schema}.doc_meta m
    ON m.source_system = 'upload' AND m.source_doc_id = f.file_group_id
 WHERE f.status = 'completed'
   AND f.commit_at IS NULL
   AND f.deleted = 0
   AND f.updated_at < NOW() - INTERVAL '24 hours';
-- 对每个候选按 ①→⑤ 收缩（**先 DB，后 S3**；S3 删除失败只打日志、下轮重试）：
-- ① 墓碑先行：拦住在途 worker 的回写（§7.4）
INSERT INTO {schema}.tombstone (doc_id, event_version, event_type, reason)
VALUES ($1, $2, 'purge', 'upload aborted before commit')
ON CONFLICT (doc_id, event_version) DO NOTHING;
-- ② 只删 staged 版本的分片，不动 active 版本
DELETE FROM {schema}.rag_chunk WHERE doc_id = $1 AND doc_version = $2;
-- ③ 文档状态：无 active → deleted；有 active（换版被放弃）→ 只清 staged
UPDATE {schema}.doc_meta
   SET staged_doc_version = NULL,
       status    = CASE WHEN active_doc_version IS NULL THEN 'deleted' ELSE status END,
       updated_at = NOW()
 WHERE doc_id = $1 AND doc_version = $2;
-- ④ 上传记录软删（G-8 第 2 条：commit 之后也要允许 cancelled）
UPDATE {schema}.file_uploads
   SET deleted = 1, status = 'cancelled', updated_at = NOW()
 WHERE id = $3;
-- ⑤ 最后才调 S3Client::delete_object(bucket, object_key)（s3.rs:367）

-- ============ step 6h：异步清理旧版本（保留 1 个版本用于回滚；必须在 6f 之后）============
DELETE FROM {schema}.rag_chunk
 WHERE doc_id = $1
   AND doc_version < (SELECT active_doc_version - 1 FROM {schema}.doc_meta WHERE doc_id = $1);
```

> **这套模板同时解决了 6 个问题**：
> - **F-1**：`diff` 在 `embed` 之前，只对 `changed + added` 调 embedding；
> - **F-2**：`WHERE m.doc_version = $3` 把"旧任务晚到"变成**写入 0 行**，而不是覆盖新数据；
> - **F-3**：`doc_version` 自增 + 乐观锁 CAS 让"谁有权写"只有一个人（`status` 不再参与互斥，见 F-8）；
> - **F-6**：`(chunk_id, doc_version)` + `active_doc_version` 让新旧版本共存，切版本是**一次 UPDATE**，回滚也是一次 UPDATE（把 `active_doc_version` 改回 N-1）。
> - **F-7**：`step -1` 把"上传完成"变成"解析可开始"的**唯一入口**，且幂等（重放不会产生第二个对象/第二次解析）。
> - **F-8**：`step 0` **只做版本 CAS、不改 `status`** —— 换版期间旧版本继续对外服务，`active_doc_version` 只被 `step 6f` 一次性推进。

---

### 7.7 上传 → 对象存储 → 解析 → 放行 的衔接契约（F-7 / F-8 / G-8 / M-10）

> **一句话**：`complete` 落 S3 之后**立刻投机解析**，`commit` 只做一次"放行" UPDATE。
> "为什么必须 S3 在前"见 F-7；本节 A~E 是可施工的契约，F 是被否掉的替代方案与三个可调参数，G 是要动的代码点。

#### A. 三个字段 + 一条铁律

| 字段 | 作用 | 谁写 |
|---|---|---|
| `doc_meta.staged_doc_version` | 已解析、已校验、**待放行**的版本 | 解析 worker（step 6e） |
| `doc_meta.source_rev` | 对应 `file_uploads.version`（M-10） | complete 钩子（step -1） |
| `doc_meta.status = 'staged'` | 新文档"解析完但未放行"的可见性闸门 | worker（step 6e） |

> **铁律：检索只认 `active_doc_version`，`staged_doc_version` 永不参与检索。**
> §7.3 的检索 SQL 已按此写。违反这条 = "用户还没点保存的文件，已经能回答别人的问题"，同时违反 §9 的权限预期。

#### B. 状态映射（`file_uploads` 六态 → `doc_meta.status`）

| `file_uploads.status` | 触发点 | `doc_meta` 动作 | 检索可见 |
|---|---|---|---|
| `pending` | `upload_init`（`upload.rs:141`） | **不建行**（此时还没有 `doc_id` 的稳定来源以外的信息） | — |
| `uploading` | `upload_start` 下发 presign（`upload.rs:240`） | 不动 | — |
| — | 分片直传 + `upload_report_chunk`（`upload_routes.rs:133`） | 不动（服务器**零字节**） | — |
| `completed` | `CompleteMultipartUpload` 成功（`upload.rs:496`） | **step -1**：UPSERT，`status='pending'`，入队 | ✗ |
| —（解析中） | worker claim（**step 0**） | 版本 CAS，`status` **不变** | 新文档 ✗ / 换版 ✓（旧版本仍在服务） |
| —（解析完成） | worker（**step 6e**） | 写 `staged_doc_version`；新文档→`staged`，换版→`active` | 同上 |
| `committed` | `upload_commit`（`upload.rs:610`） | **step 6f**：`active=staged`、`staged=NULL`、`status='active'` | ✓（下一条查询即可见） |
| `cancelled` | `abort`（`upload.rs:659`）/ `purge_staged`（**step 6g**） | **step 6g**：清 staged + 软删 | ✗ |
| `failed` | 上传失败 | 不动（无 `doc_meta` 行） | — |

> ⚠️ 现有 `abort` 只允许 `status = 'uploading'`（`upload.rs:671-676`），**complete 之后无法取消**。G-8 第 2 条要求放开到 `cancelled`，否则"用户传了 2GB 后改主意"只能等 24h 的 `purge_staged` 兜底。

#### C. 完整时序（编号与代码位置一一对应）

```text
① create_knowledge_asset / 选树节点                → 业务 id 先占位（与 RAG 无关）
② upload_init(context_type, context_id)           → file_uploads 行 status='pending'（不碰 S3）
③ upload_start                                    → CreateMultipartUpload + presigned URLs
④ 分片直传 + upload_report_chunk                  → 服务器零字节
⑤ upload_complete                                 → CompleteMultipartUpload，status='completed'
⑥ 【新增】head_object 自检 + step -1 UPSERT + 入队   ← F-7 的缺口正在这一步（当前代码完全没有）
      ├──────────── 解析从这里开始跑（投机）────────────┐
⑦ worker: claim(step 0) → download_object（流式，B-9）→ temp/{job_uuid}/original.ext
⑧ parse → chunk → diff(step 4) → embed(仅 changed/added) → store(step 6, 版本门控)
⑨ step 6e: staged_doc_version=N；新文档→'staged'；换版→status 保持 'active'
      └──────────────────────────────────────────────┘
⑩ upload_commit → 同一事务：file_uploads='committed' + step 6f 放行 → 立即返回"已生效"
```

> **⑥ 的前提**：`doc_id` 在 ② 之后就能算出来（`sha256(长度前缀拼接(tenant_id,'upload',file_group_id))`，M-2），**不需要等 `commit` 拿到 `context_id`**。这是投机解析能成立的唯一前提 —— 若将来把 `doc_id` 改成依赖 `context_id`，本节方案作废，必须退回"commit 后解析"。
> **⑩ 的延迟 ≈ 0**：解析已在 ⑥~⑨ 完成，`commit` 只是一条 UPDATE；用户点"保存"不再等 OCR。



#### D. 解析队列（不新增表，直接用 `doc_meta` 当队列）

```sql
-- worker 取任务：由 §7.1 的 idx_doc_meta_pending 部分索引支撑
SELECT doc_id, doc_version, source_uri, source_rev
  FROM {schema}.doc_meta
 WHERE status = 'pending' AND deleted = 0
 ORDER BY created_at
   FOR UPDATE SKIP LOCKED
 LIMIT 1;
```

- ⚠️ `FOR UPDATE SKIP LOCKED` **必须在事务内**（`pool.begin()` → `SELECT` → 处理 → `COMMIT`）。在连接池上直接执行时锁在语句结束即释放，队列会**重复取到同一个任务**（`upload.rs` 里现有的 `FOR UPDATE` 就是拼在 `&self.pool` 上直接执行的，属于同一类问题，本次一并修）。
- 并发上限：OCR 是 CPU 密集（进程内 `tract-onnx`，B-5 / G-3），建议 `min(2, cpu_cores / 2)`；投机解析另有开关，见 F。
- **不需要第二个队列**：投机解析与"commit 后解析"执行路径完全相同，唯一差别在 `step 6e` 里 `status` 走哪个分支（`staged` 还是保持 `active`）。

#### E. 三种终局

| 终局 | 触发 | 动作 | 检索影响 |
|---|---|---|---|
| **放行** | `upload_commit` | step 6f | 下一次查询即可见 |
| **解析失败** | worker 报错 | 新文档：`status='failed'` + `last_error`；**换版文档：`status` 保持 `'active'`，仅清 `staged_doc_version` 并写 `last_error`** | **换版失败时旧版本继续服务** —— 本方案最大收益 |
| **放弃** | 永不 commit（用户关页面） | step 6g（24h 后） | staged 版本从未可见，但占磁盘与向量 |

> 失败分支的 SQL **必须**区分这两种情况，否则"换版解析失败"会把一个**正在正常服务**的文档标成 `failed`，线上立刻失去这个知识源：

```sql
-- step 6f'：解析失败（不是 step 6f，别混）
UPDATE {schema}.doc_meta
   SET status             = CASE WHEN active_doc_version IS NULL THEN 'failed' ELSE 'active' END,
       staged_doc_version = NULL,
       last_error         = $2,
       updated_at         = NOW()
 WHERE doc_id = $1 AND doc_version = $3;
```

#### F. 被否掉的替代方案 + 三个可调参数

| 方案 | 做法 | 结论 |
|---|---|---|
| **A（选中）投机解析 + 放行闸门** | complete 后立刻解析，commit 放行 | ✅ commit 返回值≈0；换版失败不影响线上。代价：可能白跑 OCR + 多一个 `staged` 态 |
| B 仅 commit 后解析 | commit 触发解析 | ❌ 用户点"保存"要等 OCR/GPU（GB 视频几分钟）；换版期间必须中断旧版本 |
| C 先解析后上传 | 浏览器传后端 → 解析 → 再写 S3 | ❌ 放弃 presigned 直传（`upload_routes.rs:133`），服务器经手全量字节 → 直接撞 B-9；且 chunk 的 `source_uri` 无稳定来源 |

| 键（`.env.toml` 的 `[rag]` 段） | 默认 | 说明 |
|---|---|---|
| `speculative_parse_enabled` | `true` | 关掉即退化为方案 B。默认开，换取 commit 零延迟 |
| `speculative_parse_max_bytes` | `52428800`（50MB） | 超过此大小**不投机**，等 commit 再解析 —— 避免"2GB 视频解析完被放弃"白烧算力 |
| `purge_staged_after_hours` | `24` | step 6g 的回收阈值。24h 给用户"回来继续"的窗口；磁盘紧张改 `2` |

> 三个键都必须在代码里有读取点（与 B-8 同口径：`grep -r "speculative_parse_enabled" src/` 非空）。

#### G. 本次衔接需要动代码的地方（与 §8.4 实施顺序配套）

| 位置 | 改动 | 关联项 |
|---|---|---|
| `crates/storage/src/s3.rs:330` | `download_object` 改流式写盘；`create_dir_all` 前置 | **B-9** |
| `crates/storage/src/upload.rs:496` `complete()` | 追加 `head_object`（`s3.rs:468`）自检（对象 size == `file_size`）+ step -1 UPSERT + 入队，同一事务 | **F-7** |
| `crates/storage/src/upload.rs:610` `commit()` | 事务内追加 step 6f 放行；返回值带上 `active_doc_version` | **F-7** |
| `crates/storage/src/upload.rs:659` `abort()` | 允许 `status='completed'`：先 DB 收缩（step 6g ①~④），成功后 `delete_object` | **G-8** |
| `crates/storage/src/upload.rs:731` `clean_expired()` | 扩展为 `purge_staged`：覆盖 `completed && commit_at IS NULL` | **G-8** |
| 新增 worker（`crates/rag` 或 `crates/service`） | claim → download → parse → chunk → diff → embed → store → step 6e；按 D 限流 | F-1 / F-8 |
| `crates/service/src/doc_parser.rs:98` | 调用方改为传本地临时路径；临时目录 RAII guard | G-8 |

### 7.8 检索侧存储分层：热库 / 冷库 / 会话记忆（三者不可混用）

> 本节回答三个问题：① 哪些数据放内存库、哪些放分布式库；② 读写要不要分；③ **会话记忆为什么不是热数据**。
> 结论先给：**V1.7 只落「单库 pgvector + 缓存 key 规范 + 路由接口」，热层（Redis-VSS / Valkey）留到 V1.8** —— 依据是 §10 POC-6 的实测结论，而不是「先不做」。
> 与 **§7.9** 的分工：本节定**边界**（三层各自是什么、怎么路由），§7.9 定**既有《文档解析后向量化数据缓存方案》哪段能抄、哪段禁止** —— 那份文档把 LanceDB 定为主存储，与本节的结论相反（C-7）。

#### A. 先把三个存储模块切开

| 模块 | 存什么 | 生命周期 | 典型实现 | 丢了会怎样 | 与本方案的关系 |
|---|---|---|---|---|---|
| **会话记忆**（Session Memory / Conversation History） | 最近 N 轮 user/assistant 消息、工具调用记录 | **分钟~小时**，会话结束或 TTL 到期即消失 | Valkey / Redis（`String` / `List` / `Stream`）或会话表 | 用户需重述上下文；**知识正确性不受影响** | **不进向量库**，不参与相似度打分 |
| **向量库（热层）** | 高频访问的 chunk 文本 + 向量 | 短（随热度浮动，可整体丢弃） | Redis-VSS（`FT.CREATE ... VECTOR`）、Milvus 内存索引段 | 只是变慢 | §7.2 `rag_chunk` 的加速副本 |
| **向量库（冷层）** | 全量 chunk 文本 + 向量 + 过滤元数据 | **长期**，与 `doc_meta` 同寿 | pgvector（HNSW）、Milvus 集群、Qdrant、Weaviate、LanceDB | 知识库失效，回答没有依据 | **本方案的主存储**（§7.2 / §7.3） |
| **业务库** | `doc_meta` / `knowledge_asset` / `file_uploads` | 永久 | PostgreSQL | 一切失效 | 事实源（§7.1） |

> ⚠️ **最常见的设计错误**：把会话记忆当成热数据塞进向量库，理由是「它访问频率高」。这两者**只有「频率高」一个属性相同**，其余全部相反：
> - 会话记忆**短命**（写入即开始过期），向量库**长命**（同一 chunk 可能一年后还在服务）；
> - 会话记忆**不参与相似度检索**（按 `session_id` 顺序取），向量库的核心就是近似最近邻；
> - 会话记忆**不能下沉**（半小时前的对话挪到磁盘再取回来已经没有意义），而热层数据的下沉是常规操作；
> - 一致性要求相反：会话记忆允许最终一致（少一轮上下文可接受），向量库不允许（少一个 chunk 就答不出来）。
>
> 一句话：**热数据 ≠ 短期数据。热 = 访问频率高 + 可能频繁变化 + 对延迟敏感，并且仍需长期保存。**

#### B. 冷热的判据（四个条件同时满足才算热）

```text
热数据 = 访问频率高 ∧ 数据可能频繁变化 ∧ 对延迟敏感 ∧ 仍需长期存在
冷数据 = 变更少 ∧ 访问少 ∧ 对延迟不敏感 ∧ 体量大
```

| 维度 | 内存向量库（热） | 分布式向量库（冷） |
|---|---|---|
| 典型实现 | Redis-VSS、Milvus 内存索引段 | pgvector（HNSW on disk）、Milvus 集群、Qdrant、Weaviate、LanceDB |
| 延迟 | 亚毫秒~毫秒级 | 毫秒~几十毫秒 |
| 成本 | 高（内存贵，同样的钱买到的容量小） | 低（磁盘为主） |
| 容量 | 小 | 大，可水平扩展 |
| 适合数据量 | 几万~几十万 chunk | 千万~亿级 |
| 更新代价 | 低，可高频写（但每次都是内存写，贵） | 较高，批量写更优 |
| 碎片化风险 | 高（高频小批量写 → 内存索引碎片，须定期重建） | 中（可批量整理） |

> **二八定律的落法**：20% 高频 chunk 放热库承接 80% 查询的超低延迟路径，80% 长尾放冷库用低成本扛容量。
> ⚠️ 但这条定律成立的前提是**访问分布真的偏斜**。若访问是均匀的（每个文档都被偶发问一次），热库只会变成「第二份冷库」，纯亏内存 —— **先测偏斜再建热层**，这正是 POC-6 要回答的问题。

#### C. 路由：变更事件 → 热度判定 → 热库 / 冷库

```text
信息变更事件（上传 commit / 文档替换 / 删除 / 归档）
        │
        ├─ 路由判定（输入：access_count_24h / last_accessed_at / doc_type / is_pinned / 业务状态）
        │        ├─ 热 → 写热库（Valkey / Redis-VSS）＋ 异步写冷库（冷库是权威副本）
        │        └─ 冷 → 只写冷库
        └─ 两边共用同一个 doc_version / chunk_id（否则无法对账）
```

三条硬约束（违反任意一条，分层就不成立）：

1. **冷库是权威副本，热库是可丢弃的加速层。** 任何时候清空热库，系统都必须能正常工作（只是变慢）。这条决定了「热库挂了不叫故障」，可以直接降级为只查冷库。
2. **热库的写入必须带 `doc_version`**，否则用户在热库看到新版本、检索却读到冷库的旧版本（与 F-4 的缓存 key 缺维度是同一类错误）。
3. **热库不得成为事实源**：只有冷库 / 业务库能触发对热库的写入，热库自身不产生数据，也不做「热库有、冷库没有」的合并。

#### D. 下沉与回热（同一条数据会由热变冷）

| 方向 | 触发 | 判据 | 动作 |
|---|---|---|---|
| **下沉**（热→冷） | 定时任务 `demote_hot`（每小时） | `now - last_accessed_at > demote_after_days` **或** 所属项目 / 活动已归档 **或** `access_count` 近 7 天低于阈值 | 从热库 `DEL` 该 `doc_id` 的全部 chunk；冷库本来就有，无需再写 |
| **回热**（冷→热） | 检索命中时惰性提升 `promote_on_hit` | 同一 `doc_id` 在 1h 内命中 ≥ N 次 | 从冷库读 chunk → 写热库（带 TTL `hot_tier_ttl_sec`） |

- **下沉必须幂等 + 可重放**：先删热库、再更新元数据；顺序反过来会留下「热库里有一条查不到的记录」。
- **业务事件要早于访问频率触发下沉**：活动结束、项目归档这类信号比 `last_accessed_at` 更准确 —— 业务上已经结束的东西，再热也是历史数据。
- 下沉 / 回热都要打点：`rag_hot_promote_total` / `rag_hot_demote_total`（带 `doc_type` 标签），否则无法证明分层在工作。

#### E. 读写分离（按操作类型分，与冷热是两个正交维度）

- **分类依据**：对数据做的是读还是写。**目的**：提升读并发，避免写操作阻塞读。
- **热库也可以读写分离**（主库写、从库读），但热库延迟已足够低，读写分离的收益主要是**扛并发**而不是降延迟。
- **冷库的读写分离需求更强烈**：冷库读多写少、体量大，用多个只读副本扛查询流量是性价比最高的扩容手段。
- ⚠️ **主从复制延迟本身就是实时性挑战**（毫秒~秒级）。这既是 F-5 的根因（对账源侧必须读主库），也是检索侧的既有事实：**读从库时，刚 `commit` 的文档可能几秒内检索不到**。（V1.8 启用只读副本时）该窗口值必须在 §11 验收清单里**显式登记**并接受，而不是当成 bug 反复排查。
- 落地顺序：V1.7 单实例（无读写分离）→ V1.8 冷库加只读副本（PG 流复制 / Qdrant 副本）→ 热层按 POC-6 结论决定。

#### F. 增量更新：能力、边界，以及它治不好的三种病

**核心流程**（顺序由 F-1 定死：`embed` 在 `diff` 之后，只重算变更 chunk）：

```text
信息变更（commit / 替换）→ 提取变更字段 → 只对变更 chunk 重新 Embedding
   → 向量库 Upsert（新 chunk 插入 / 变更 chunk 覆盖 / 消失 chunk 标记删除）
   → 结构化元数据同步（doc_meta / file_name / page_no 等过滤字段）
```

| 技术点 | 要求 | 本方案落点 |
|---|---|---|
| **chunk 级更新** | 一条文档切成 N 个 chunk，只重算受影响的，不整篇重切 | F-1 + §7.6 step 4（双键比对 `(chunk_id, content_hash)`） |
| **Upsert 语义** | 按主键覆盖：Milvus `Delete+Insert`、Qdrant `upsert`、pgvector `ON CONFLICT (chunk_id, doc_version) DO UPDATE` | §7.6 step 6 |
| **元数据同步** | 向量更新时过滤字段必须同步，否则 `WHERE page_no / file_name` 会命中旧值 | M-9 的冗余列 + step 6 的 `SET` 列表 |
| **幂等性** | 消息可能重投，重复写不能产生重复向量 | `ON CONFLICT`（天然幂等）+ `workflow_binding` 唯一索引 |

**必须写明的三个固有局限**（否则会被当成 bug 反复排查）：

1. **索引碎片化**：高频小批量写会让 HNSW 图的边变得不优，召回率与延迟同时退化，且**监控上看不出来**。对策：写入量 / 表膨胀达到阈值后 `REINDEX CONCURRENTLY`（pgvector），每周例行整理（`index_maintenance`）。
2. **链路长了会丢消息**：`上传 → 队列 → 消费 → 解析 → 写入` 每一跳都可能丢。对策：**不把 MQ 的「至少一次」当作正确性保证**，而是靠 §7.6 / §7.7 的数据库状态机 —— `status='pending'` 的行本身就是待办队列，丢消息时行还在，下一轮扫描会重新捡起（这也是 §7.7 D 用 `doc_meta` 当队列的原因）。
3. **Embedding 模型升级 → 新旧向量不在同一语义空间，必须全量重建**。这条最贵：放弃「只重算变更」，对全库重跑，走 B-2 的三步迁移 + §8.2 蓝绿索引；**热层里的旧向量必须一并清空**，否则热层继续返回旧模型向量，比不重建更糟。

#### G. 定时全量校验：校验什么（与 F-5 的对账互补，不另写一套）

| 校验项 | 要发现的问题 | 判据 / 手段 |
|---|---|---|
| **数据完整性** | 增量更新漏更：业务库有、向量库没有 | `doc_meta` LEFT JOIN `rag_chunk` 的计数差 = 0；缺失项写 `diff_repair_task`（`src_only`） |
| **向量一致性** | 向量是否与**当前** `embedding_model_version` 的输出一致 | 抽样 1%（至少 50 条）用现模型重算，余弦 ≥ 0.999；不达标 → 该 `embedding_model_version` 整体重建 |
| **索引健康度** | HNSW 是否碎片化、是否需要重建 | `pg_relation_size` 增速 vs 行数增速；检索 P95 环比恶化 > 50% → `REINDEX CONCURRENTLY` |
| **元数据对齐** | 向量库的过滤字段与业务库不一致（改名 / 换页没同步） | 抽样比对 `file_name / page_no / active_doc_version` |

> **校验结果的正确性取决于快照**：源侧读主库、记录水位，水位漂移超阈值则标记 `unreliable` 并丢弃本次结果 —— 见 F-5，不要另写一套对账逻辑。

#### H. 本仓库的落地范围（V1.7 / V1.8 分期）

现状核对（**事实**，非建议）：
- `crates/` 下 8 个 crate 与根 `apps/backend/src-tauri/Cargo.toml` 中**没有** `redis` / `valkey` / `lancedb` / `qdrant` / `milvus` / `moka` 任何依赖；进程内缓存载体只有 `dashmap 6.1.0`（`crates/database`），重启即失效、多实例不共享。
- 仓库唯一的向量表是 `document_chunk.embedding vector(1536)`（B-2 / B-4）；本方案新增 `rag_chunk`。
- 因此「热层 / 冷层 / 会话记忆」三件套，**当前一件都没有**，不能按既有能力写。
- 会话记忆**已有现成落点**：`{schema}.conversation`（`tenant_tables.sql:892`）/ `{schema}.message`（`:917`）—— 它天然就在业务库里、与向量库物理隔离，**不需要也不应该**挪进向量库。

| 阶段 | 做什么 | 明确不做什么 |
|---|---|---|
| **V1.7** | 单库 pgvector（冷层 = 热层）；`rag:*` 缓存 key 五元组规范（F-4）；`doc_meta` 当解析队列（§7.7 D）；会话记忆走 PG 会话表，**与向量库完全分离** | 不引入 Valkey / Redis-VSS；不做读写分离；不做热冷迁移 |
| **V1.8** | 按 POC-6 实测结果决定是否加热层；冷库加只读副本；`demote_hot` / `promote_on_hit` / `index_maintenance` 三个任务 | 不在 V1.7 阶段「预留一半」实现 —— 半成品热层 = 双写不一致，比没有更糟 |

配置键（`.env.toml` 的 `[rag]` 段，沿用 C-4 的落点规则）：

| 键 | 默认 | 说明 |
|---|---|---|
| `hot_tier_enabled` | `false` | V1.7 固定 `false`；为 `true` 才允许写热库 |
| `hot_tier_ttl_sec` | `86400` | 回热后的 TTL |
| `demote_after_days` | `7` | 下沉判据之一（配合业务归档事件） |
| `session_memory_ttl_sec` | `1800` | **会话记忆**的 TTL，与向量完全无关的独立配置 |
| `index_rebuild_growth_ratio` | `1.5` | 表膨胀倍数达到该值触发 `REINDEX CONCURRENTLY` |
| `local_cache_max_entries` | `2000` | 进程内缓存**条数**上限（G-10）；不按字节，2000 条 ≈ 16MB |
| `local_cache_ttl_sec` | `60` | 检索结果缓存的**绝对**过期时间（G-10） |
| `local_cache_tti_sec` | `300` | 空闲过期（TTI）；V1.8 接 moka 时直接映射 `time_to_idle` |

> **落地红线**：会话记忆与向量库（V1.8 引入热层后）**不得共用同一个 DB index**，且**禁止 key 前缀混用**。共用实例可以，但清理会话缓存时一次 `FLUSHDB` 会连热向量一起清掉 —— 这是 §7.8 A 那条错误的最终形态。

### 7.9 与既有《文档解析后向量化数据缓存方案》的引用边界

> 背景：`docs/文档解析后向量化数据缓存方案.md`（26 KB）是本仓库**已经存在**的一份完整缓存方案。它不是待改的 V1.6，但也不能照抄 —— 它的**主次结构与 §7.8 相反**（LanceDB 当主存储、pgvector 当归档）。
> 与 §7.8 的分工：**§7.8 定边界**（三层各自是什么、怎么路由），**§7.9 定既有材料怎么用**（哪段能抄、哪段要改、哪段禁止）。
> 行号约定：`:NNN` 指该文档的行号（生成时内容，`Select-String` 可复核）。

#### A. 一句话结论

分层思想可用，**主次结构必须倒过来**：V1.7 以 pgvector 为唯一权威（§7.2 / §7.3），LanceDB 只能是热层加速器（D7 选项 C），且必须先过 POC-6。

> 该文档自己的「集群版」部署（`:27`）就暴露了矛盾：LanceDB 是**本地目录文件**（`:75` `{data_dir}/lancedb/`），而本项目是 schema-per-tenant（G-1）+ 服务端多节点 + S3 对象存储。多节点各持一份 LanceDB 文件时「谁是权威」没有答案；放到共享盘，就丢掉了它唯一比 pgvector 强的那点（本地读写快）。

#### B. 可直接引用（4 项，V1.7 采纳）

| # | 引用点 | 原文位置 | 采纳方式 |
|:--:|---|---|---|
| 1 | 分布式缓存选 **Valkey**（BSD-3-Clause、Linux 基金会托管、与 `redis` crate 协议兼容、`valkey/valkey` 镜像） | `:143` | 写进 §13.1：V1.8 不再讨论 Redis vs Valkey，直接用 Valkey + `redis` crate |
| 2 | 缓存 key 的**维度划分**（query 向量 / 检索结果 / 元数据 / 倒排 / 访问时间 五类分开） | `:163` | 采纳「分类」这一层；key 内容按 **M-12** 七元组重写（加 `{schema}` / `{perm_hash}`） |
| 3 | 命中率与延迟的**量级基线**（内存 <1μs 60-80% / 本地向量 <5ms 95%+ / 分布式 <1ms 70-90% / DB 兜底 <20ms <1%） | `:727` | 作为 G-10 的监控阈值起点与 POC-6 的对照组。**是预估，不是实测**，不能当验收标准 |
| 4 | 三条工程纪律：软删除必须同步清缓存、启动预热、记录各层命中率 | `:769-771` | 与 §7.8 H 的红线合并；其中「预热」按 G-10 第 4 条降级为「首次检索后回填」 |

> 引用方式：以上写进 §13.1 与 §11.8，**不复制**该文档的代码块 —— 它的代码是「LanceDB 必选主存储」的结构（`:573` 的 `default = ["lancedb-store"]`），照抄即回到双主存储。

#### C. 必须改造后才能用（4 项）

| # | 原文档做法 | 问题 | V1.7 改法 |
|:--:|---|---|---|
| 1 | 写入先 LanceDB（同步）→ 异步归档 pgvector（`:460`） | 权威副本靠异步追：无重试、无幂等、无水位 → 归档失败即永久丢失，且无人知晓 | 单写 pgvector（§7.6 step 1~6）；异步只做 `access_count` 累加（§7.8 H） |
| 2 | `SEARCH:{md5(query)}:{top_k}`（`:163`） | 缺 `{schema}` / `{doc_version}` / `{embedding_model_version}` / 权限 → 跨租户串结果、换版返回旧 chunk | 七元组 key（**M-12**） |
| 3 | 一致性 =「最终一致 + 启动比对记录数」（`:546`） | 记录数相等 ≠ 内容一致：更新丢失、换模型、正文改了向量没改，全都检测不到 | 水位 + 抽样重算（§7.8 G / F-5）；记录数降级为「哨兵」 |
| 4 | 索引 `IVF-PQ(num_partitions=256, num_sub_vectors=96)`（`:106`） | PQ 是**有损量化**，召回率下降不可控；且与本方案 HNSW（§7.1）不是一套索引语义，混用等于两套 | 统一 HNSW（`m=16, ef_construction=64`）；PQ 只在 POC-6 证明内存不够时作为 V1.8 选项 |

#### D. 必须否决（4 项）

| # | 原文档做法 | 否决理由 |
|:--:|---|---|
| 1 | `ALTER TABLE {schema}.asset_knowledge ALTER COLUMN vector_data TYPE vector(768) USING vector_data::text::vector`（`:122`） | `vector_data` 现状是 **`BYTEA`**（`tenant_tables.sql:313`）。`BYTEA::text` 得到的是 `\x0a1f...` 十六进制串，`::vector` 解析不了 → 有数据的表上必然失败；即便空表能过，768 也与本项目的 1536（B-2）冲突。正确做法是 §8.2 三步法：新列 → 回填/重算 → 切读 → 最后 DROP 旧列 |
| 2 | 把解析产物写进 `asset_knowledge`（`:83`） | 该表是「知识条目」粒度（一条 = 一个知识条目，`chunk_index` 只是个 int），**没有** `chunk_id` / `heading_path` / `page_no`，支撑不了 B-1 的 `chunk_id = sha256(doc_id + logical_path + local_index)` 与 M-9 的页码回链。解析产物必须落 `rag_chunk`（§7.2） |
| 3 | 权限只按 `permission_level` 过滤（`:768`「所有查询时必须带上 `permission_level` 过滤条件」） | 本项目租户隔离是 **schema 级**（G-1），`asset_knowledge` 另有 `owner_type` / `owner_id`（`tenant_tables.sql:344-346`）与 `deleted` 标记。只按 `permission_level` 过滤 = 同租户内跨 owner 可见 + 已删记录可命中。检索 SQL 必须带 schema 前缀 + `is_deleted = false` + 权限谓词（§7.3） |
| 4 | 「定期备份 `{data_dir}/lancedb/` 目录」（`:772` 第 6 条） | 这是把**主存储**当文件备份：备份窗口内写入的新数据不在备份里，恢复后与 pgvector 归档层版本错位，且没有权威副本可重建。V1.7 的备份就是 `pg_dump`（权威）；热层（若 V1.8 引入）**不需要备份** —— 它可随时从冷层重建 |

#### E. 依赖版本核对（该文档 `:573-585` 声称的三个 crate）

| crate | 该文档写的 | crates.io 实际（2026-10-28 查询） | 结论 |
|---|---|---|---|
| `lancedb` | `0.16`（`:577`） | `max_stable = 0.40.0`（2026-10-07）；`rust_version = 1.88.0`；`0.23.1` 的 `default` features 还会拉入 `aws / gcs / azure / dynamodb / oss` | 落后 **24 个 minor**；V1.8 若选 C 用 `0.40`，并先钉 MSRV（工作区 `Cargo.toml:8` 只有 `edition = "2021"`，**未声明** `rust-version`） |
| `moka` | `0.12`（`:580`） | `max_stable = 0.12.16`（2026-08-09）；`features = ["future"]` 存在 | ✅ 版本声明正确（但 V1.7 不引入） |
| `redis` | `0.27`（`:583`） | `max_stable = 1.8.0`（2026-10-23）；`newest = 2.0.0-rc.1`（预发布）；license `BSD-3-Clause`；features 含 `vector-sets` / `tokio-comp` / `json` | 落后一个大版本；V1.8 用 `1.8`。`vector-sets` 说明「热层」还有第二条路（不依赖 Redis Search / Valkey Search 模块），但 **Valkey 侧对等命令需另行核实**，不进 V1.7 决策 |

> 结论：**V1.7 一个都不引入**（与 §7.8 H 的仓库现状一致）；V1.8 若上热层，按上表实际版本重写 `Cargo.toml`，不要复制原文档的 `0.16 / 0.27`。

#### F. V1.7 的落地动作（3 条，可勾选）

- [ ] 在 `docs/文档解析后向量化数据缓存方案.md` **头部加取代声明**：「本方案的分布式缓存选型与工程纪律仍有效；**主存储结论（LanceDB 主 / pgvector 归档）已被 V1.7 §7.8 / §7.9 取代**」—— 否则后来者会照旧实现。
- [ ] §13.1 依赖清单把 `lancedb` / `moka` / `redis` 标为 **V1.8 候选**，注明 gated by `D7` + `POC-6`，并附 E 表的实际版本号。
- [ ] §11.8 第 1 条的 grep 进 CI（`crates/*/Cargo.toml` 命中 `lancedb` / `moka` / `valkey` 即失败），把「不许偷偷上热层」变成机器检查。

---

## 8. 迁移、回滚与灰度

### 8.1 迁移脚本落位（沿用现有机制，不引入 flyway/sqlx-migrate）

| 步骤 | 动作 | 文件 / 函数 |
|---|---|---|
| 1 | 新增 DDL 脚本 | `crates/database/src/sql/rag_migration.sql`（§7 全部内容，`{schema}` 占位） |
| 2 | 新增迁移函数 | `postgres.rs::migrate_tenant_rag_tables(pool, schema)` + `migrate_all_tenant_rag_tables(pool)`，写法照抄 `migrate_tenant_workflow_tables`（`postgres.rs:751`） |
| 3 | 挂到启动流程 | `init_postgres_database` 第 11.2 步（现有 11.1 是 workflow 迁移，`postgres.rs:935-937`） |
| 4 | `init_tenant_tables` 末尾追加 | 新租户开通时自动建表（与 `migrate_tenant_workflow_tables` 同一位置，`postgres.rs:746`） |
| 5 | 启动期断言 | **扩展** `pgvector` 版本 ≥ 0.5.0；`rag_chunk.text_vector` 的 `atttypmod` = 768+4；`cargo tree -i sqlx` 中 `sqlx` 只出现 **1 个版本**（出现 2 个 → pgvector 的 `<0.10` 上界被绕过，见 D-1） |

> **为什么不用 `sqlx::migrate!`**：现有代码用的是"`include_str!` 内嵌 SQL + 手写遍历"，且已经处理了 `{schema}` 占位符替换。引入第二种迁移机制会让"到底谁建的表"变得不可知。

### 8.2 维度迁移（1536 → 768）的三步法

```sql
-- 步骤 1：加新列（不停写、不阻塞）
ALTER TABLE {schema}.document_chunk ADD COLUMN IF NOT EXISTS embedding_768 vector(768);

-- 步骤 2：后台回填（分批，每批 500 行，带 statement_timeout）
UPDATE {schema}.document_chunk
   SET embedding_768 = NULL              -- 或 重新调用 embedding 写入
 WHERE id IN (SELECT id FROM {schema}.document_chunk
               WHERE embedding_768 IS NULL AND deleted = 0
               ORDER BY id LIMIT 500);
-- 循环直到 0 行；其间新写入同时写两列（应用层双写）

-- 步骤 3：切换（一次短事务内完成，秒级）
BEGIN;
  ALTER TABLE {schema}.document_chunk DROP COLUMN embedding;
  ALTER TABLE {schema}.document_chunk RENAME COLUMN embedding_768 TO embedding;
COMMIT;
-- 然后重建 HNSW 索引（§7.5 的 6.3）
```

### 8.3 回滚

| 场景 | 回滚动作 | RTO |
|---|---|---|
| 新 DDL 有问题 | 全部是 `CREATE TABLE IF NOT EXISTS`，**不删表**即可；功能开关 `RAG_USE_VECTOR=false` 直接让检索走旧路径 | 重启 1 次 |
| 维度迁移失败 | 步骤 3 未执行前，`embedding` 原列**一直存在**；直接删 `embedding_768` 即可 | < 1 分钟 |
| 版本切换错误 | `UPDATE doc_meta SET active_doc_version = <旧版本> WHERE doc_id = ?` | 1 条 SQL |
| 蓝绿索引升级回退 | 把 `active_doc_version` / `embedding_model_version` 指回旧模型；旧向量按"保留 1 个版本"策略仍在库里 | 1 条 SQL |

> **蓝绿双索引的落地建议**：V1.6 只写了"双索引并行校验"没写怎么做。最简单可靠的方式是**同表双列**：
> ```sql
> ALTER TABLE {schema}.rag_chunk ADD COLUMN text_vector_v2 vector(1024);
> CREATE INDEX ... ON {schema}.rag_chunk USING hnsw (text_vector_v2 vector_cosine_ops) ...;
> -- 校验通过后：RENAME COLUMN text_vector TO text_vector_v1; RENAME COLUMN text_vector_v2 TO text_vector;
> ```
> 而不是建一张新表 —— 新表意味着数据迁移 + 外键 + 权限全套重做。

### 8.4 上传衔接改造的实施顺序（约 4 人日，不含 POC）

| 步 | 内容 | 工作量 | 前置 |
|---|---|---:|---|
| 1 | §7 DDL 增量（`doc_meta` 三列 + `staged` 枚举 + `idx_doc_meta_pending` + §7.7 契约）落成 `rag_migration.sql` | 0.5d | **D2 拍板** |
| 2 | `download_object` 流式（B-9）；全仓 `FOR UPDATE` 补事务（现拼在 `&self.pool` 上）；`upload_init` 的 `MAX(version)+1` 并发去重（并发 init 会撞 `uk_file_group_version`，现无重试） | 0.5d | — |
| 3 | `complete` 钩子：`head_object` 自检 + step -1 UPSERT + 入队 | 0.5d | 1 |
| 4 | worker：claim → download → parse → chunk → diff → embed → store → step 6e，按 §7.7 D 限流 | 1.5d | 1、3 |
| 5 | `commit` 放行（事务内 step 6f）+ `purge_staged`（step 6g） | 0.5d | 1 |
| 6 | 前端：`complete` 后展示"预解析中"，`commit` 后一次性变"已生效" | 0.5d | 4 |

> - **第 2 步必须先做**：B-9 与"`FOR UPDATE` 缺事务"都是**现有代码缺陷**（不是本次新增需求），不修则第 4 步"看起来对、跑起来炸"。
> - 这 4 人日是 §7 整体施工之外的**额外**衔接成本；比"仅 commit 触发"多约 1.5 天，换来的是 commit 返回值 ≈ 0。
> - 若某天决定 `doc_id` 不能在上传阶段算出来（§7.7 C 的前提失效），则第 3~5 步全部作废，退回方案 B。

---

## 9. 待决策项（需人工拍板，不拍板就无法写最终版文档）

| 编号 | 决策点 | 选项 | 影响面 | 建议 |
|---|---|---|---|---|
| **D2** | 表策略：`rag_chunk` 与 `document_chunk` 如何共存 | **A** 改造 `document_chunk` / **B** 双表并存 + 视图统一 / **C** 全量替换 | §7 全部 DDL、`rag_service.rs` 全部查询、前端引用字段 | **B**：可灰度、可回滚，见 C-5 |
| **D3** | 是否保留 `image_vector` | ① 砍掉（图片走 OCR+VLM 文本）/ ② 本地 ONNX CLIP / ③ 独立 TEI 服务 | §5.4 DDL（`vector(512)` 列）、§7.2/7.4 链路、B-8 | **①**：V1.6 的 `image_vector` 依赖做不到的能力 |
| **D4** | `runledger` 是否转正 | ① 保留双实现 / ② 只留 `SimpleLocalOrchestrator` | 双 sqlx 成本（D-5）、`workflow_binding` 设计、G-6 演练负担 | 先做 §10.3 的双 sqlx 代价测量，再定；若 >5MB 建议 ② |
| **D5** | 分块器 | ① `text-splitter 0.33` 替换自研 / ② 保留自研但修 bug | `chunk_strategy_version` 迁移、D-8、C-6 | **①**：C-6 的估算 bug 与 panic 风险一次性解决 |
| **D6** | 桌面端存储 | ① 也连 PostgreSQL / ② 本地 SQLite / ③ 仅服务端有知识库 | G-2 部署形态表、RAG 在桌面端的可用性 | 需产品决策；技术上 ② 代价最大（两套 SQL 方言） |
| **D7** | 检索侧要不要现在就上热层（Redis-VSS / Valkey） | **A** 只做 V1.7 单库 pgvector，热层延到 V1.8（按 POC-6 数据决定）/ **B** V1.7 直接引入热层 + 冷热双写 / **C** 引入 LanceDB 当本地或冷层（既有《文档解析后向量化数据缓存方案》就是这条路的**更激进版本**：LanceDB 当主存储，见 **C-7** / **§7.9**） | §7.8 全章、**§7.9**、G-9 / G-10 的定时任务与配置键、依赖清单（选 B 加 `redis` / `valkey`；选 C 另加 `lancedb`） | **A**：先用 POC-6 量「单库 P95 是否已够 + 访问是否偏斜」，不够再上热层，避免开局就背双写一致性 |

> **D2 是硬前置**：§7 的 DDL 已按 B 写好，若最终选 A/C，§7 需整体重写（约 1 天）。
>
> **D7 不是硬前置**：§7.8 已按「V1.7 单库 + V1.8 热层」写完，选 B 只增配置与依赖、不动 DDL；选 C 会牵动桌面端存储栈（D6）。既有缓存方案（LanceDB 主）的冲突见 **C-7 / §7.9** —— D7 选 C 等于兑现那份文档的路线，须同时废弃它的「pgvector 归档」定位。

---

## 10. POC 验证清单（6 项微实验，合计约 2 天，不改生产代码）

> 目的：把本清单里的**假设**变成**测量值**。每项给出"通过判据"与"不通过时怎么改文档"。

### 10.1 POC-1｜`pgvector 0.4 + sqlx 0.9` 能否编译并绑定 768 维向量

```powershell
cd apps\backend\src-tauri
# 在 crates/database/Cargo.toml 临时加：pgvector = { version = "0.4", features = ["sqlx"] }
cargo build -p database
cargo tree -i sqlx                      # 期望：只列出一个 sqlx v0.9.0（pgvector 复用同一份，不拉第二份）
cargo update -p sqlx --precise 0.10.0   # 期望：失败（crates.io 上尚无 0.10）
```
- **通过判据**：
  1. `pgvector::Vector` 能绑定进 `WHERE embedding <=> $1`，`fetch_all` 能解码回 `Vector`，`cargo build` 绿；
  2. `cargo tree -i sqlx` **只列出一个** `sqlx v0.9.0`。若出现两个 → 体积与类型双付代价，直接改走降级路径；
  3. `cargo update -p sqlx --precise 0.10.0` 的**报错原文**贴进 §13.1，作为「`<0.10` 上界」的实证。若将来 0.10 已发布且此命令成功，必须立刻跑 `cargo tree -d` 确认没有出现第二份 sqlx（见 D-1 的上界失效风险）。
- **不通过（trait bound 不匹配）**：走 D-1 的降级路径 —— 不启用 `sqlx` feature，用 `$1::vector` 文本绑定（`Vec<f32>` → `"[0.1,0.2,...]"`），结论写进 §5.4。
- **耗时**：约 1 小时（含首次编译）。

### 10.2 POC-2｜`tract-onnx` 单页 OCR 的真实耗时

```powershell
cd apps\backend\src-tauri
cargo build -p parser --features ocr --example spike_ocr
# 用一份真实扫描件 PDF（≥5 页）跑，记录单页耗时
```
- **通过判据**：单页 P50 ≤ 10s、P95 ≤ 25s（300dpi 中文）。
- **不通过**：G-3 的性能预算按实测值改写，`parse_timeout_sec` 改为按页数动态计算；若 > 30s/页，需重新评估改用 `pdf_oxide/ocr`（ORT，接受 B-5 的 rc.13 锁定）。

### 10.3 POC-3｜双 sqlx 的实际代价（决定 D4）

```powershell
cd apps\backend\src-tauri
cargo tree -d                              # ① 基线：先只引入 pgvector，记录重复依赖（sqlx 必须只有 1 份）
cargo build --release                      # ② 基线产物体积 / 冷编译时间
# ③ 再加入 runledger 三件套，重跑上面两条，做差
cargo tree -d | Select-String sqlx         # ④ 此时才应出现 0.8.x + 0.9.0 两份
```
- **通过判据**：产物体积增量 ≤ 5MB、冷编译增量 ≤ 2 分钟。
- **必须分别归因**：步骤 ① 里若有第二份 sqlx，是 **pgvector 的 `<0.10` 上界问题（D-1）**，与 runledger 无关；只有 ④ 里多出来的那份才算作 D-5 的成本。两者混在一起算，会得出「runledger 便宜/昂贵」的错误结论。
- **不通过**：走 D-5 第 4 条 —— `SimpleLocalOrchestrator` 扶正，删掉 runledger。

### 10.4 POC-4｜离线构建（决定 D6 的 `download-binaries` 是否可用）

```powershell
cd apps\backend\src-tauri
cargo build -p parser --features asr --offline   # 或 sherpa 的最小 example
```
- **通过判据**：`--offline` 下构建成功（说明二进制已缓存或随包）。
- **不通过**：内网交付无法用 `download-binaries`，需改为"预处理阶段下载 + 随包分发"，结论写进 D-6 与 §12.1。

### 10.5 POC-5｜大文件流式下载的内存与耗时（决定 B-9 的改法）

```powershell
cd apps\backend\src-tauri
# 先用稀疏文件造一个 1.5GB 的测试对象，上传到本地 MinIO
fsutil file createnew big.bin 1610612736
# 逐块流式写到临时目录，过程中采样进程峰值内存
cargo run -p storage --example spike_download -- --bucket test --key big.bin
Get-Process <pid> | Select-Object WorkingSet64    # 另一终端反复采样
```
- **通过判据**：① 进程峰值 RSS 增量 < 100MB；② 磁盘写入速率与下载速率同步增长（不出现"下载完才开始写盘"的阶梯）；③ 对象大小翻倍时 RSS 不变。
- **不通过（仍全量进内存）**：说明 `ByteStream` 被内部缓冲或改动没落到位 —— 退路是用 `presign_get_object`（`s3.rs:300`）拿 URL，交给 `reqwest` 的流式响应直接 `io::copy` 到文件，绕开 SDK。
- **为什么必须做**：B-9 是"顺手修"的改动，但**判据必须实测**，否则 GB 级文件在桌面端仍会静默 OOM（Tauri 进程被杀，用户只看到应用退出，没有错误信息）。
- **耗时**：约 1 小时（含 MinIO 准备）。

### 10.6 POC-6｜pgvector 的延迟曲线 + 访问偏斜度（决定 D7 要不要热层）

```powershell
cd apps\backend\src-tauri
# ① 造数：10 万 / 100 万 chunk（768 维；随机向量可跑通链路，真实分布才有决策价值）
# ② 同一批 top-20 查询各跑 200 次，记录 P50 / P95（分别开、关 HNSW 的部分索引）
# ③ 统计访问偏斜：把命中次数按 doc_id 降序累计，画累计占比曲线（看是否符合二八）
```
- **通过判据（不是「过/不过」，而是「走哪条路」）**：
  - 100 万 chunk 下 P95 ≤ 50ms 且偏斜**不成立**（Top 20% 文档承接不到 60% 命中）→ **不加热层**（D7 选 A）。此时热库只会变成「第二份冷库」，纯亏内存；
  - P95 > 50ms 且偏斜成立 → 值得加热层，按 §7.8 C / D 设计，V1.8 实施，并把「热层省掉的 P95」记为收益基线。
- **不通过（延迟已不可接受）**：先按 B-7 调 `hnsw.ef_search` / `m` / 部分索引，再谈 §7.8 H 的 V1.8 方案；**不要**用「加 Redis」掩盖索引参数没调好的事实。
- **为什么必须做**：热层是**成本翻倍**的决策，能撑住它的证据只有两个数：延迟曲线 + 偏斜比例。没有这两个数就上 Redis，等于用内存买「感觉更快」。
- **耗时**：约 0.5 天（造数占大半，可复用 POC-1 的造数脚本）。

### 10.7 执行顺序

```
POC-2（最长，先起） → POC-1（等编译时并行） → POC-5（不占编译） → POC-3 → POC-4 → POC-6
```
POC-2 决定 §5.1.5 性能预算与超时配置；POC-3 决定 **D4**（要不要 runledger），是影响面最大的一项；POC-5 决定 **B-9** 的改法是否成立（不成立就要绕开 SDK 走 presigned URL + `io::copy`）；**POC-6 决定 D7**（要不要热层），是唯一影响「运维成本结构」而非「代码结构」的一项。

---

## 11. 放行验收清单（V1.7 文档定稿前必须全部打勾）

### 11.1 文档完整性

- [ ] 57 条整改项**逐条**在 V1.7 正文中有对应修改（不是只存在于整改清单）
- [ ] §13.1 依赖清单与 §10 配置项**一一对应**（每个配置键都能在代码里找到读它的地方）
- [ ] §13.2 DDL 与 §7 完全一致（不能两处各写一版）
- [ ] 删除项已彻底清除：`datafusion`（依赖 + §5.5.5 + §11 + §12.3 + §10）、`ort`、`rxrust`、`[pgvector] *_database_url`
- [ ] 新增章节齐备：§3.3 部署形态、§5.1.5 性能预算、§5.5.0 引擎边界、§5.5.6 多租户、§10.0 配置落点、§11.2 检索评测、§14 许可证、**§7.7 上传→解析衔接、§8.4 衔接实施顺序、§7.8 存储分层（热/冷/会话记忆）、§7.9 既有缓存方案引用边界**

### 11.2 可编译性

- [ ] `cargo build -p parser --features ocr` 绿（无 `ort` 双版本）
- [ ] `cargo build -p database` 绿（`pgvector` 引入后）
- [ ] `cargo tree -d` 中除**已声明的双 sqlx**（D-5）外，无其他重复 major 依赖
- [ ] `Cargo.lock` 中 `opentelemetry` 只有一代（0.32）

### 11.3 数据正确性

- [ ] `SELECT count(*) FROM {schema}.rag_chunk WHERE text_vector IS NULL AND is_deleted = false` = 0
- [ ] `SELECT count(*) FILTER (WHERE embedding IS NULL) FROM {schema}.document_chunk` = 0（B-4 修复验证）
- [ ] `EXPLAIN` 检索计划出现 `Index Scan using idx_rag_chunk_text_vec`，无 `Seq Scan`
- [ ] 同一文档连续两次 `rag_etl`，`rag_chunk` 行数不变（幂等）
- [ ] 手动回退 `doc_version` 后重放旧任务，`rag_chunk` **无变化**（F-2 版本门控）
- [ ] 文档中间插入一段后重跑：只有该段所在 `heading_path` 下的 `chunk_id` 变化（B-1 方案 B 定义性检验）

### 11.4 可观测性与降级

- [ ] `workflow_binding.trace_id` 能从提交端串到 step handler（或已改为自建关联，G-5）
- [ ] `SimpleLocalOrchestrator` 跑通完整 `rag_etl`，并演练过"切换 + 回滚"各一次（G-6）
- [ ] `RAG_USE_VECTOR=false` 时系统行为与改造前一致（回滚开关有效）

### 11.5 合规

- [ ] 根目录 `NOTICE` 含 `symphonia (MPL-2.0)` 与 ffmpeg (LGPL-2.1)
- [ ] `cargo deny check licenses` 绿

### 11.6 上传 → 解析衔接（第二轮新增，对应 §7.7）

| # | 验收动作 | 期望结果 | 覆盖项 |
|:--:|---|---|---|
| 1 | 上传并 `complete`，**不** `commit`，立即跑 §7.3 检索 | 检索**不到**该文档（`status='staged'`，`active_doc_version IS NULL`） | F-7 / 铁律 A |
| 2 | 继续 `commit` | 返回体含 `active_doc_version`；下一次检索立刻命中；`staged_doc_version IS NULL` | step 6f |
| 3 | 对 `active` 文档触发换版，解析**进行中**跑检索 | 仍能召回到**旧版本** chunk（`status` 未变） | **F-8** |
| 4 | 换版解析**失败** | `status` 仍为 `'active'`、`active_doc_version` 未动、`last_error` 有值、`staged_doc_version IS NULL` | E 终局表 |
| 5 | 重复调用 `complete`（模拟前端重试） | `doc_meta` 只有 1 行、S3 只有 1 个对象、只触发 1 次解析 | step -1 幂等 |
| 6 | `complete` 后 24h 不 commit，跑 `purge_staged` | 对象已删、`rag_chunk` 无该 `doc_version`、`file_uploads.status='cancelled'`、`doc_meta.staged_doc_version IS NULL` | **G-8** / step 6g |
| 7 | 上传 1.5GB 文件并跑完整链路 | 进程峰值 RSS 增量 < 100MB；解析失败后临时目录为空 | **B-9** / POC-5 |
| 8 | 对任一 `status='active'` 的文档 | `SELECT source_uri, source_rev FROM doc_meta` 能唯一定位到一个 `file_uploads` 行 | **M-10** |
| 9 | 检索 SQL 全仓 grep | `grep -rn "staged_doc_version" crates/` 只出现在写路径（step 6e/6f/6g），**检索路径 0 命中** | 铁律 A |
| 10 | `head_object` 自检失败（size 不符） | `complete` 返回明确错误，不写 `doc_meta`、不入队 | step -1 前置 |

### 11.7 存储分层与会话记忆边界（第三轮新增，对应 §7.8 / M-11 / G-9）

| # | 验收动作 | 期望结果 | 覆盖项 |
|:--:|---|---|---|
| 1 | 全仓 grep `session_memory` 与向量检索 SQL | 会话记忆**不出现**在任何检索 SQL 或向量表里；`session_memory_ttl_sec` 只被会话层读取 | **M-11** |
| 2 | 清空会话记忆（模拟 `FLUSHDB` 或 TTL 到期）后跑检索 | 检索结果**完全不受影响**；反向：清空热层（V1.8）后检索仍正确，只是 P95 变差 | M-11 / §7.8 A |
| 3 | 检查 `[rag]` 配置 | `hot_tier_enabled=false` 时，代码路径里**没有任何**热库客户端被构造（不是构造了不用） | D7 选 A / §7.8 H |
| 4 | `SELECT count(*) FROM {schema}.doc_meta WHERE last_accessed_at IS NOT NULL AND access_count = 0` | 结果 = 0（两个字段必须同进同退） | **G-9** |
| 5 | 检索 200 次后查 SQL 日志 | 检索路径**没有** `UPDATE doc_meta SET access_count`；热度累加只出现在异步批量任务里 | G-9 第 1 条 |
| 6 | 数据量翻倍后看 `pg_relation_size` 与 P95 | 尺寸线性增长、P95 不劣化 50% 以上；超过 `index_rebuild_growth_ratio` 时 `index_maintenance` 真有 `REINDEX` 日志 | §7.8 F 局限 1 / G |
| 7 | 人为删一条 `rag_chunk`（模拟漏更）后跑全量校验 | 报出 `src_only` 差异并写 `diff_repair_task`；**且**校验期间源侧读的是主库（`pg_stat_activity` 可查） | §7.8 G / F-5 |
| 8 | 模拟换 `embedding_model_version` | 校验报「向量不一致」→ 触发该版本全量重建；建成后热层（若开）里查不到旧模型向量 | §7.8 F 局限 3 |

> 第 1、2 条是**边界**验收（证明三个模块没有混用），第 4~6 条是**运维节奏**验收（证明分层不会失控），第 7、8 条是**校验有效性**验收（证明对账不是摆设）。

### 11.8 缓存层与既有方案引用（第四轮新增，对应 §7.9 / M-12 / G-10 / C-7）

| # | 验收动作 | 期望结果 | 覆盖项 |
|:--:|---|---|---|
| 1 | `Select-String -Path crates/*/Cargo.toml -Pattern 'lancedb','moka','valkey'`（CI 用等价 grep） | **0 命中**：V1.7 的 8 个 crate 里不存在这三个依赖（V1.8 引入前这条一直有效） | **C-7** / §7.9 E |
| 2 | 打开 debug 日志，两个不同租户各问同一个问题一次 | 两个 `rag:hits:*` key **不同**（各含自己的 `{schema}` 与 `{perm_hash}`）；`rag:qvec:*` key **相同**（纯查询向量可共享） | **M-12** |
| 3 | 换版（`doc_version` 1→2）后立刻重问同一个问题 | 不返回旧 `chunk_id`；返回的每个 `chunk_id` 都能在 `rag_chunk` 里按 `doc_version = 2` 查到（无悬空引用） | M-12 / F-8 |
| 4 | 清空进程内缓存（重启进程）后重跑同一批查询 | 结果**完全一致**，只有 P95 变差 —— 证明缓存是加速器、不是数据源 | **G-10** / §7.8 C |
| 5 | 连续检索 1000 次，观察缓存条目数 | 条目数不超过 `local_cache_max_entries`，且超过 TTI 后条目数下降（淘汰真的生效） | G-10 / §7.8 H |
| 6 | 对业务库执行 `\d+ {schema}.asset_knowledge` | `vector_data` 仍为 **`bytea`**，表上**没有** `vector(768)` 列 —— 证明 §7.9 D-1 那条错误 ALTER 没有被误实施 | §7.9 D |
| 7 | 蓝绿换 embedding 模型期间不做任何清理，直接检索 | 灰度期间新旧向量不混用（key 带 `{embedding_model_version}` 自然隔离）；切流量后旧 key 不再被读 | F-4 / M-12 |
| 8 | 检查 `docs/文档解析后向量化数据缓存方案.md` 头部 | 存在指向 §7.8 / §7.9 的取代声明，且注明「LanceDB 主存储」结论已作废 | **C-7** / §7.9 F |

> 第 1、6、8 条是**路线**验收（防止照旧文档实现），第 2、3、7 条是**正确性**验收（防止跨租户 / 跨版本串味），第 4、5 条是**缓存本质**验收（缓存只能是加速器，不能是数据源）。

---

## 附录 A：核实证据索引

### A.1 代码事实（本仓库，行号为生成时内容）

| 事实 | 位置 |
|---|---|
| `document_chunk` 实际 DDL（`vector(1536)`、**无 HNSW 索引**、无唯一约束） | `crates/database/src/sql/tenant_tables.sql:862-888` |
| 该表注释谎称"HNSW索引加速" | `tenant_tables.sql:879` |
| `knowledge_asset` DDL（`content TEXT` 明文、`file_md5`） | `tenant_tables.sql:625-671` |
| `RAGRetriever::retrieve` 用 `0.0 AS similarity` + `ORDER BY dc.id`，不用 `question` | `crates/service/src/rag_service.rs:98-121` |
| `chunk_and_vectorize` 的 INSERT 不含 `embedding`，却 `RETURNING embedding` | `crates/service/src/rag_service.rs:176-181` |
| `TextChunker` 按 `\n\n` 切、`estimate_tokens` 中文按 0.5 token/字、overlap 按字节切片 | `crates/service/src/rag_service.rs:31-84` |
| `DocumentChunk.embedding: Option<Vec<f32>>`（sqlx 无 `vector` 解码） | `crates/database/src/models.rs:848-864` |
| `RetrieveParams{question, top_k, max_tokens, min_similarity}` | `crates/database/src/models.rs:1126-1135` |
| 4 处调用方硬编码 `min_similarity: 0.0` | `conversation_service.rs:430/484/1095`、`crates/workflow/src/agent.rs:476` |
| `ParseResult` 5 个扁平字段 | `crates/parser/src/types.rs:12-25` |
| `ocr_pdf()` 直接返回 `None`（OCR 未实现） | `crates/parser/src/ocr.rs:18-25` |
| `ocr = ["pdf_oxide/ocr-tract"]`（无 `ort`） | `crates/parser/Cargo.toml:31-33` |
| `ffmpeg-sidecar` 已声明但全 crate 未调用 | `crates/parser/Cargo.toml:23` + `src/lib.rs:85-100` |
| `DEFAULT_OCR_LANGUAGE = "chi_sim+eng"`（tesseract 风格） | `crates/parser/src/options.rs:12` |
| `LOCAL_SUPPORTED_EXTS` 只有 pdf/doc/docx/xls/xlsx | `crates/parser/src/lib.rs:46` |
| `{schema}` 占位替换 + `include_str!` 迁移机制 | `crates/database/src/postgres.rs:679-724` |
| `migrate_all_tenant_workflow_tables` 遍历写法（可照抄） | `postgres.rs:751-775` |
| `CREATE EXTENSION IF NOT EXISTS vector`（无版本断言） | `postgres.rs:920-925` |
| `PostgresConfig::from_env` 读 `PG_HOST/...`（非 URL） | `postgres.rs:98-110` |
| 工作区 `sqlx 0.9.0` features（无 `json`） | `apps/backend/src-tauri/Cargo.toml:61-68` |
| 已有 `adk-graph 2.3.0` / `adk-rust 2.3.0` | `Cargo.toml:86-87` |
| `axum 0.8.9` + `jsonwebtoken 11` + `futures 0.3.33` | `crates/api/Cargo.toml:11,35,36` |
| `crates/` 现有 8 个 crate（无 `orchestrator`） | `apps/backend/src-tauri/crates/` |
| `Cargo.lock` 中 `opentelemetry 0.32.0` / `tracing-opentelemetry 0.33.0`；**无 pgvector** | `apps/backend/src-tauri/Cargo.lock:5531,9741` |
| Python 侧 `EMBEDDING_MODEL=bge-small-zh-v1.5` / `EMBEDDING_DIM=512` | `apps/doc-parser/config.py:31-32` |
| `file_uploads` DDL：`file_group_id` / `version` / `etag` / `commit_at` / `deleted`，`UNIQUE(file_group_id, version)` | `crates/database/src/sql/tenant_tables.sql`（搜 `file_uploads`） |
| `upload_init` 以 `MAX(version)+1` 生成源版本 | `crates/storage/src/upload.rs:154-171` |
| `complete()` 只做 CompleteMultipartUpload + 置 `completed`，**无任何解析钩子** | `upload.rs:496-603` |
| `complete()` 拼接 `file_url`（依赖 `public_base_url`，换域名即失效） | `upload.rs:581` |
| `commit()` 只更新 `context_type/context_id/commit_at/status` | `upload.rs:610-653`（SQL 在 `:634-650`） |
| `abort()` 仅允许 `status='uploading'` → **complete 之后无法取消** | `upload.rs:671-676` |
| `clean_expired()` 仅清 `pending`/`uploading` → completed 未 commit 的永不回收 | `upload.rs:731-745` |
| `download_object` 用 `body.collect()` 全量进内存；`create_dir_all` 排在下载之后 | `s3.rs:330-364` |
| `head_object` / `presign_get_object` / `delete_object` 已存在（衔接可直接复用） | `s3.rs:468` / `:300` / `:367` |
| 分片直传路由（presigned，服务器不接字节） | `crates/api/src/routes/upload_routes.rs:133` |
| 8 个 crate 与根 `Cargo.toml` 中**无** `redis` / `valkey` / `lancedb` / `qdrant` / `milvus` / `moka` 依赖（热层为零实现） | `crates/*/Cargo.toml`、`apps/backend/src-tauri/Cargo.toml` |
| 进程内并发容器只有 `dashmap 6.1.0`（非共享、重启即失效） | `crates/database/Cargo.toml:18` |
| 会话记忆已有落点：`conversation` / `message` 两表（与向量库物理隔离，见 §7.8 A） | `crates/database/src/sql/tenant_tables.sql:892,917` |
| 唯一向量表 `document_chunk.embedding vector(1536)`；`doc_meta` / `rag_chunk` 原本无 `access_count` / `last_accessed_at` 热度字段 | `crates/database/src/sql/tenant_tables.sql`（搜 `document_chunk`） |
| `DocParser::parse_file(file_path: &str)` 只接受本地路径；remote 分支 POST `{"file_path": ...}` | `crates/service/src/doc_parser.rs:98-107` / `:146-150` |
| `asset_knowledge.vector_data` 现为 **`BYTEA`**（不是 `vector(768)`）；表注释为「资产知识库表（RAG检索 + 大模型微调专用）」 | `crates/database/src/sql/tenant_tables.sql:313`、`:324` |
| `asset_knowledge` 的权限不是单一维度：`permission_level` + `owner_type` + `owner_id` + `deleted` 四列并存 | `tenant_tables.sql:342-346` |
| 工作区 `Cargo.toml` 只有 `edition = "2021"`，**未声明** `rust-version`（`lancedb 0.40` / `redis 1.8` 均要求 Rust ≥ 1.88） | `apps/backend/src-tauri/Cargo.toml:8` |

### A.2 外部事实（crates.io，2026-10-28 查询）

| crate | V1.6 写的 | 实际 | 结论 |
|---|---|---|---|
| `pgvector` | `0.7` | `max_stable = 0.4.2`（2026-05-22 发布） | **不存在** → 改 `0.4` + `features=["sqlx"]`；其 `sqlx` 依赖范围为 **`>=0.8, <0.10`** |
| `datafusion` | `40` | `max_stable = 55.2.0` | **不存在** → 删除（场景也不成立） |
| `sqlx` | `0.7` + `jsonb` | 工作区 `0.9.0`；crates.io `max_stable = newest = 0.9.0`（2026-05-21），**0.10 尚未发布**；无 `jsonb` feature | 三重错误 → 对齐 0.9 + 改 `json`；0.10 发布后须先确认 pgvector 的 `<0.10` 上界 |
| `runledger-core` | `=0.9.0` | `max_stable = 0.12.0`，依赖 `sqlx ^0.8.6` | 改 `=0.12.0`，并记录双 sqlx 成本 |
| `text-splitter` | `0.12` | `max_stable = 0.33.1` | 版本落后 + 用途未定义 |
| `opentelemetry` | `0.21` | `max_stable = 0.33.1`；工作区已有 **0.32.0** | 用 `0.32` 与锁文件对齐 |
| `tracing-opentelemetry` | `0.23` | `max_stable = 0.34.0`；工作区已有 **0.33.0** | 用 `0.33` |
| `sherpa-rs` | `0.6.8` | `0.6.8` ✅ 存在 | 版本可用，但降级为"待 spike" |
| `reqwest` | `0.13` | `max_stable = 0.13.5` | ✅ 正确（此前曾怀疑有误，已核实存在） |
| `symphonia` | `0.6.1` | MPL-2.0 | 需进 NOTICE |
| `pdf_oxide` / `office_oxide` | `0.3.78` / `0.1.15` | 均存在 | ✅ 正确 |
| `lancedb` | 既有缓存方案写 `0.16` | `max_stable = 0.40.0`（2026-10-07）；`rust_version = 1.88.0`；`0.23.1` 的 `default` features 含 `aws/gcs/azure/dynamodb/oss` | 落后 24 个 minor；V1.8 若选 C 用 `0.40`，先钉 MSRV |
| `moka` | 既有缓存方案写 `0.12` | `max_stable = 0.12.16`（2026-08-09）；`features = ["future"]` 存在 | ✅ 声明正确（V1.7 不引入） |
| `redis` | 既有缓存方案写 `0.27` | `max_stable = 1.8.0`（2026-10-23），`newest = 2.0.0-rc.1`；license `BSD-3-Clause`；features 含 `vector-sets` / `tokio-comp` / `json` | 落后一个大版本；V1.8 用 `1.8` |

### A.3 上一轮评审中已更正的两条

| 原判断 | 更正 |
|---|---|
| 「`sherpa-rs` 与 `ort` 存在 ORT 版本冲突」 | **不成立**。`sherpa-rs` 走 `sherpa-rs-sys` 自带 C 库，不使用 `ort`。真实问题改为「`download-binaries` 离线构建风险」与「尚未决策」 |
| 「需要把 sqlx 降回 0.8 才能用 pgvector」 | **不成立**。`pgvector 0.4.2` 的 `sqlx` 依赖是 **`>=0.8, <0.10`**：0.8.x / 0.9.x 都在范围内，故与工作区 0.9.0 兼容，无需降级 |
| 「pgvector 0.4 兼容 sqlx 0.8 / 0.9 / 0.10」（评审反馈） | **前两项成立、第三项不成立**：声明上界是 `<0.10`，0.10 超出范围；且 crates.io 上 sqlx 0.10 **尚不存在**（`newest = 0.9.0`）。若将来确认存在满足 0.10 的 pgvector 版本，以该版本 `Cargo.toml` 的 `[dependencies.sqlx] version` 原文为准，本行随之更新 |

### A.4 同仓既有设计文档（`docs/文档解析后向量化数据缓存方案.md`，行号为生成时内容）

| 事实（该文档的主张） | 位置 |
|---|---|
| 「LanceDB 是主读写存储 / pgvector 是持久化归档 / Valkey 是分布式缓存 / 内存 LRU 是顶层」 | `:9-14` |
| 部署模式表：桌面版与集群版**都以 LanceDB 为主** | `:27-32` |
| LanceDB「主存储」位置 = `{data_dir}/lancedb/`（本地目录） | `:73-75` |
| LanceDB schema：主键为知识条目 `id`、向量固定 `768` 维、分块只有 `chunk_index` 一个 int | `:83-97` |
| IVF-PQ 索引 `num_partitions=256` / `num_sub_vectors=96` | `:101`（`num_partitions` 在 `:106`） |
| `ALTER TABLE {schema}.asset_knowledge ALTER COLUMN vector_data TYPE vector(768) USING vector_data::text::vector` | `:122` |
| Valkey vs Redis 选型（BSD-3-Clause / Linux 基金会 / `redis` crate 协议兼容） | `:143` |
| 缓存 key：`VECTOR:{id}` / `SEARCH:{md5(query)}:{top_k}`（TTL 300）/ `META:{id}` / `INVERTED:{keyword}` / `ACCESS_TIME:{id}` | `:163` |
| 内存缓存用 moka（`sync::Cache` / TTL / TTI / SLRU） | `:182` |
| 写入：LanceDB 同步 → 内存同步 → pgvector **异步**归档 | `:460` |
| 查询：内存 → LanceDB → Valkey → pgvector 兜底 + 回填 | `:475` |
| 一致性：「最终一致」+ 启动时对比记录数 | `:546` |
| 依赖：`lancedb 0.16` / `moka 0.12` / `redis 0.27` | `:577` / `:580` / `:583` |
| 部署配置：`primary = "lancedb"`、`memory_cache_size = 1000`、`search_cache_ttl = 300` | `:682` |
| 性能预期（内存 <1μs 60-80% / LanceDB <5ms 95%+ / Valkey <1ms 70-90% / pgvector <20ms <1%） | `:727` |
| 注意事项：「所有查询时必须带上 `permission_level` 过滤条件」 | `:765`（该条在 `:768`） |

---

> **文档结束**。
> 下一步（需 Act 模式）：① 按 §9 拍板 D2~D7（D2 是硬前置，决定 §7 是否整体重写；D7 只决定 V1.8 要不要热层）；② 执行 §10 的 6 项 POC；③ 据 POC 结果把本清单合入 V1.7 正文（建议直接在 docx 上改，导出 V1.7 并保留本清单作为变更依据）。
> **第二轮（上传衔接）已定事项**：§7.7 方案 A（投机解析 + 放行闸门）为默认方案；三个参数默认值 `speculative_parse_enabled=true` / `speculative_parse_max_bytes=50MB` / `purge_staged_after_hours=24` 已写入 §7.7 F，若产品侧不同意只需改配置、不改代码结构。
> **第三轮（存储分层）已定事项**：§7.8 按「V1.7 单库 pgvector（冷层即热层）+ V1.8 热层」写；**会话记忆 / 向量库（热·冷）/ 业务库**三者边界为强制约定（M-11），会话记忆永不进向量库、永不参与相似度检索；`doc_meta` 已加 `access_count` / `last_accessed_at` 两列（G-9，文档级热度，禁止在检索热路径逐条 UPDATE）。默认值：`hot_tier_enabled=false` / `hot_tier_ttl_sec=86400` / `demote_after_days=7` / `session_memory_ttl_sec=1800` / `index_rebuild_growth_ratio=1.5`；是否上热层由 **POC-6** 的延迟曲线与访问偏斜度决定（D7）。
> **第四轮（缓存层引用）已定事项**：既有 `docs/文档解析后向量化数据缓存方案.md` **保留但降级为「选型备查」**，其「LanceDB 主存储 / pgvector 归档」结论**作废**（C-7 / §7.9 D）。可直接引用的只有：Valkey 选型、缓存 key 的分类维度（内容按 M-12 七元组重写）、命中率基线（当起点不当验收）、三条工程纪律。V1.7 依赖零新增（`lancedb` / `moka` / `redis` 一律不引入，CI grep 卡口）；V1.8 若上热层，按 §7.9 E 的实际版本（`lancedb 0.40.0` / `moka 0.12.16` / `redis 1.8.0`）重写依赖，不要复制原文档的 `0.16 / 0.27`。新增缓存配置键：`local_cache_max_entries=2000` / `local_cache_ttl_sec=60` / `local_cache_tti_sec=300`（G-10，已并入 §7.8 H）。

