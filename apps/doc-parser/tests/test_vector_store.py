"""向量存储测试：Embedding 维度、写入可查、幂等去重（SurrealDB mem:// 嵌入式）

运行：在 apps/doc-parser 目录下执行
    /home/ubuntu/conda/envs/aiagent/bin/python -m pytest tests/test_vector_store.py -v -p asyncio

说明：使用 SurrealDB mem://（内存嵌入式）模式，无需外部服务。
Embedding 模型首次使用会下载（BAAI/bge-small-zh-v1.5）。
"""

import pytest

from services.text_chunker import chunk_video_text
from services.vector_store import VectorStore, EmbeddingService
import config


def _make_store() -> VectorStore:
    """创建使用 mem:// 的 VectorStore 实例（独立命名空间，避免测试间污染）"""
    import uuid

    return VectorStore(
        url="mem://",
        ns=f"test_{uuid.uuid4().hex[:8]}",
        db="knowledge",
        table="video_knowledge",
        dim=config.EMBEDDING_DIM,
    )


def _sample_chunks(video_id: str = "video_test_001", file_name: str = "ceshi.mp4"):
    return chunk_video_text(
        video_id=video_id,
        file_name=file_name,
        segments=[
            {"time": 5, "type": "voice", "text": "工业机器人控制软件由实时系统和非实时系统组成"},
            {"time": 15, "type": "ocr", "text": "控制系统组成图：控制器、示教器、机器人本体"},
            {"time": 40, "type": "voice", "text": "实时部分负责运动学算法的调度与规划"},
        ],
        window_sec=30,
        max_chars=500,
        duration_sec=180,
    )


class TestEmbedding:
    """Embedding 维度测试（不依赖 SurrealDB）"""

    @pytest.mark.asyncio
    async def test_embedding_dimension(self):
        """Embedding 输出维度与配置一致"""
        vecs = await EmbeddingService.embed(["你好", "机器人"])
        assert len(vecs) == 2
        for v in vecs:
            assert len(v) == config.EMBEDDING_DIM


class TestVectorStore:
    """SurrealDB 向量存储测试（mem:// 嵌入式）"""

    @pytest.mark.asyncio
    async def test_write_and_search(self):
        """写入可查：入库后向量检索能命中含关键词切片"""
        store = _make_store()

        chunks = _sample_chunks()
        await store.ensure_schema()
        written = await store.add_chunks(chunks)
        assert written == len(chunks), "应写入所有切片"

        # 检索含"控制系统组成图"的切片
        results = await store.search("控制系统组成图", top_k=3)
        assert len(results) > 0
        assert results[0]["video_id"] == "video_test_001"
        assert "控制系统组成图" in results[0]["content"]
        assert "start_sec" in results[0] and "end_sec" in results[0]
        assert 0 <= results[0]["score"] <= 1.001

    @pytest.mark.asyncio
    async def test_idempotent_dedup(self):
        """幂等去重：同一 video_id 二次入库跳过"""
        store = _make_store()
        chunks = _sample_chunks()

        await store.ensure_schema()
        first_written = await store.add_chunks(chunks)
        assert first_written == len(chunks)

        # 第二次入库同一 video_id → 跳过（返回 0）
        second_written = await store.add_chunks(chunks)
        assert second_written == 0, "同一 video_id 重复入库应跳过（幂等）"

        # 数据仅一份
        results = await store.search("机器人", top_k=50)
        assert len(results) == len(chunks)

    @pytest.mark.asyncio
    async def test_video_id_filter(self):
        """video_id 过滤：不同视频不互相干扰"""
        store = _make_store()
        await store.ensure_schema()

        await store.add_chunks(_sample_chunks(video_id="video_a"))
        await store.add_chunks(_sample_chunks(video_id="video_b"))

        # 限定 video_a 检索，不应出现 video_b 结果
        results = await store.search("运动学算法", top_k=10, video_id="video_a")
        assert len(results) > 0
        assert all(r["video_id"] == "video_a" for r in results)

    @pytest.mark.asyncio
    async def test_soft_delete(self):
        """软删除：deleted=1 后不再出现在检索结果"""
        store = _make_store()
        await store.ensure_schema()
        await store.add_chunks(_sample_chunks(video_id="video_del"))

        # 删除前可检索到
        before = await store.search("示教器", top_k=5)
        assert len(before) > 0

        # 软删除
        deleted = await store.soft_delete("video_del")
        assert deleted is True

        # 删除后检索不到
        after = await store.search("示教器", top_k=5)
        assert len(after) == 0