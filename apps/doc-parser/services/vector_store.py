"""向量记忆存储：Embedding + SurrealDB 入库/检索/幂等去重

设计参考：
- docs/知识库模块/视频全量转文本与向量化记忆设计方案.md 第 5 章
- apps/backend/src-tauri/tests/surrealdb_tests.rs（Rust 端 MTREE 向量索引约定）

SurrealDB 支持两种部署模式（通过 SURREALDB_URL 切换）：
- 远程分布式：ws://127.0.0.1:8000（与 Rust 端共用 NS/DB）
- 嵌入式：file://./data/surrealdb 或 mem://（测试/单机）
"""

import asyncio
import time

import config


class EmbeddingService:
    """文本向量化服务（sentence-transformers 单例）

    默认模型 BAAI/bge-small-zh-v1.5 → 512 维；
    切换 BAAI/bge-base-zh-v1.5 → 768 维（需同步 SURREALDB DIMENSION）。
    """

    _model = None
    _model_name = None
    _encode_lock = asyncio.Lock()

    @classmethod
    def _get_model(cls):
        from sentence_transformers import SentenceTransformer

        if cls._model is None or cls._model_name != config.EMBEDDING_MODEL:
            cls._model = SentenceTransformer(config.EMBEDDING_MODEL)
            cls._model_name = config.EMBEDDING_MODEL
        return cls._model

    @classmethod
    async def embed(cls, texts: list[str]) -> list[list[float]]:
        """批量文本向量化（线程安全，模型推理在独立线程避免阻塞事件循环）"""
        if not texts:
            return []

        def _run():
            model = cls._get_model()
            vectors = model.encode(
                texts, normalize_embeddings=True, show_progress_bar=False
            )
            return [v.tolist() for v in vectors]

        async with cls._encode_lock:
            loop = asyncio.get_running_loop()
            return await loop.run_in_executor(None, _run)

    @classmethod
    async def embed_one(cls, text: str) -> list[float]:
        vecs = await cls.embed([text])
        return vecs[0] if vecs else []


class VectorStore:
    """SurrealDB 向量记忆存储

    - 建表/建向量索引（幂等）
    - 批量入库（同一 video_id 重复入库自动跳过 → 幂等去重）
    - 向量检索（cosine 距离，支持 video_id/权限过滤）
    - 软删除
    """

    def __init__(
        self,
        url: str = None,
        user: str = None,
        password: str = None,
        ns: str = None,
        db: str = None,
        table: str = None,
        dim: int = None,
    ):
        self.url = url or config.SURREALDB_URL
        self.user = user or config.SURREALDB_USER
        self.password = password or config.SURREALDB_PASS
        self.ns = ns or config.SURREALDB_NS
        self.db = db or config.SURREALDB_DB
        self.table = table or config.SURREALDB_TABLE
        self.dim = dim or config.EMBEDDING_DIM

    # ─── 连接管理 ─────────────────────────────────────

    async def _connect(self):
        """建立 SurrealDB 连接并完成认证/选库（由调用方关闭）

        surrealdb 2.0：AsyncSurreal(url) 返回异步连接（支持 __aenter__）；
        Surreal(url) 返回阻塞同步连接。必须用 AsyncSurreal 才能在 async 场景使用。
        """
        from surrealdb import AsyncSurreal

        client = AsyncSurreal(self.url)
        await client.__aenter__()

        # 嵌入式（file://mem://）通常无需认证；远程 ws 需要
        try:
            await client.signin(
                {
                    "user": self.user,
                    "pass": self.password,
                }
            )
        except Exception:
            # 嵌入式模式可能不需要 signin，忽略
            pass

        await client.use(self.ns, self.db)
        return client

    # ─── Schema ───────────────────────────────────────

    async def ensure_schema(self) -> None:
        """建表 + 建向量索引（幂等，重复调用安全）

        surrealdb 2.0 语法：DEFINE TABLE（非 CREATE TABLE）；
        索引：COLUMNS embedding MTREE DIMENSION n（无 DIST TYPE，2.0 不支持）。
        """
        client = await self._connect()
        try:
            # 建表（幂等；DEFINE TABLE 重复执行报已存在，忽略）
            try:
                await client.query(f"DEFINE TABLE {self.table}")
            except Exception as e:
                print(f"[VectorStore] 建表跳过（可能已存在）: {e}")
            # 建向量索引（幂等：已存在则忽略）
            index_sql = (
                f"DEFINE INDEX idx_embedding ON {self.table} "
                f"COLUMNS embedding MTREE DIMENSION {self.dim}"
            )
            try:
                await client.query(index_sql)
            except Exception as e:
                print(f"[VectorStore] 索引创建跳过（可能已存在）: {e}")
        finally:
            await client.__aexit__(None, None, None)

    # ─── 幂等去重 ─────────────────────────────────────

    async def exists(self, video_id: str) -> bool:
        """检查某视频是否已入库（deleted=0 存在即视为已记忆）"""
        client = await self._connect()
        try:
            sql = (
                f"SELECT id FROM {self.table} "
                f"WHERE video_id = $video_id AND deleted = 0 LIMIT 1"
            )
            rows = await client.query(sql, {"video_id": video_id})
            records = self._first_result(rows)
            return len(records) > 0
        finally:
            await client.__aexit__(None, None, None)

    # ─── 入库 ─────────────────────────────────────────

    async def add_chunks(self, chunks: list) -> int:
        """批量入库切片，返回实际写入条数

        同一 video_id 已存在（deleted=0）时跳过写入 → 幂等去重。
        """
        if not chunks:
            return 0

        video_id = chunks[0].video_id

        # 幂等：已存在则直接返回 0（不重复入库）
        if await self.exists(video_id):
            return 0

        # 构造带向量的记录
        texts = [c.text for c in chunks]
        vectors = await EmbeddingService.embed(texts)

        client = await self._connect()
        try:
            created_at = time.strftime("%Y-%m-%dT%H:%M:%S+00:00", time.gmtime())
            for i, chunk in enumerate(chunks):
                record = {
                    "id": f"{video_id}_{chunk.chunk_index}",
                    "video_id": chunk.video_id,
                    "embedding": vectors[i],
                    "chunk_index": chunk.chunk_index,
                    "start_sec": float(chunk.start_sec),
                    "end_sec": float(chunk.end_sec),
                    "type": chunk.type,
                    "content": chunk.text,
                    "file_name": chunk.file_name,
                    "permission_level": "private",  # 多租户：由上层传入，默认私有
                    "doc_source": f"video:{video_id}",
                    "deleted": 0,
                    "created_at": created_at,
                }
                await client.create(self.table, record)

            return len(chunks)
        finally:
            await client.__aexit__(None, None, None)

    # ─── 检索 ─────────────────────────────────────────

    async def search(
        self,
        query: str,
        top_k: int = 10,
        video_id: str = None,
        permission_level: str = None,
    ) -> list[dict]:
        """向量检索相关切片

        返回：[{video_id, chunk_index, start_sec, end_sec, type, content, score}]
        score = vector::similarity::cosine 相似度（0~1，越大越相关）
        """
        query_vector = await EmbeddingService.embed_one(query)

        client = await self._connect()
        try:
            where = "deleted = 0"
            binds: dict = {
                "query": query_vector,
                "top_k": max(1, int(top_k)),
            }
            if video_id:
                where += " AND video_id = $video_id"
                binds["video_id"] = video_id
            if permission_level:
                where += " AND permission_level = $perm"
                binds["perm"] = permission_level

            sql = (
                f"SELECT id, video_id, chunk_index, start_sec, end_sec, type, content, file_name, "
                f"vector::similarity::cosine(embedding, $query) AS score "
                f"FROM {self.table} WHERE {where} "
                f"ORDER BY score DESC LIMIT $top_k"
            )
            rows = await client.query(sql, binds)
            records = self._first_result(rows)

            results = []
            for r in records:
                results.append(
                    {
                        "video_id": r.get("video_id"),
                        "chunk_index": r.get("chunk_index"),
                        "start_sec": r.get("start_sec"),
                        "end_sec": r.get("end_sec"),
                        "type": r.get("type"),
                        "content": r.get("content"),
                        "file_name": r.get("file_name"),
                        "score": round(float(r.get("score", 0.0)), 4),
                    }
                )
            return results
        finally:
            await client.__aexit__(None, None, None)

    # ─── 软删除 ───────────────────────────────────────

    async def soft_delete(self, video_id: str) -> bool:
        """按 video_id 软删除（deleted=1）"""
        client = await self._connect()
        try:
            sql = f"UPDATE {self.table} SET deleted = 1 WHERE video_id = $video_id"
            rows = await client.query(sql, {"video_id": video_id})
            records = self._first_result(rows)
            return len(records) > 0
        finally:
            await client.__aexit__(None, None, None)

    # ─── 工具 ─────────────────────────────────────────

    @staticmethod
    def _first_result(rows) -> list[dict]:
        """解析 SurrealDB query 返回：取第一条结果集

        surrealdb 2.0：query() 已解包，直接返回 list[dict]（无 Response 包装）。
        兼容旧版（list[Response] 含 result key 或嵌套 list）。
        """
        if isinstance(rows, list):
            first = rows[0] if rows else None
            if isinstance(first, dict) and "result" in first:
                return first["result"] or []
            return [r for r in rows if isinstance(r, dict)] or []
        if isinstance(rows, (list, tuple)) and rows and isinstance(rows[0], list):
            return rows[0] or []
        return rows or []


# 进程级单例
vector_store = VectorStore()