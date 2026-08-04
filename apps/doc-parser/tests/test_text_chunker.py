"""文本切片器单元测试：切片数量、时间戳边界、中文 500 字二次切分"""

import pytest

from services.text_chunker import chunk_video_text, VideoChunk


class TestTextChunker:
    """VideoChunk 切片逻辑测试（无外部依赖，纯内存）"""

    def test_empty_segments(self):
        """空片段返回空列表"""
        chunks = chunk_video_text("v1", "test.mp4", [])
        assert chunks == []

    def test_basic_window_split(self):
        """按时间窗口切分：30s 窗口内语音+OCR 归入同一切片"""
        segments = [
            {"time": 10, "type": "voice", "text": "工业机器人控制软件可以分为两部分"},
            {"time": 15, "type": "ocr", "text": "控制系统组成图"},
            {"time": 50, "type": "voice", "text": "实时部分负责运动学算法"},
        ]
        chunks = chunk_video_text(
            video_id="abc123",
            file_name="ceshi.mp4",
            segments=segments,
            window_sec=30,
            max_chars=500,
            duration_sec=90,
        )

        # 两个窗口：0-30s（2 条）、30-60s（1 条）
        assert len(chunks) == 2
        assert chunks[0].start_sec == 0
        assert chunks[0].end_sec == 30
        assert chunks[0].type == "mixed"  # voice + ocr
        assert chunks[1].start_sec == 30
        assert chunks[1].type == "voice"

        # chunk_index 连续编号
        assert [c.chunk_index for c in chunks] == [0, 1]
        # 时间戳前缀存在
        assert "[10-40 voice]" in chunks[0].text or "[00-30" in chunks[0].text or "语音" in chunks[0].text

    def test_type_classification(self):
        """类型分类：全 voice → voice；全 ocr → ocr"""
        voice_chunks = chunk_video_text(
            "v1", "a.mp4",
            [{"time": 5, "type": "voice", "text": "语音内容A"}],
            window_sec=30, max_chars=500,
        )
        assert voice_chunks[0].type == "voice"

        ocr_chunks = chunk_video_text(
            "v1", "a.mp4",
            [{"time": 5, "type": "ocr", "text": "画面文字B"}],
            window_sec=30, max_chars=500,
        )
        assert ocr_chunks[0].type == "ocr"

    def test_max_chars_secondary_split(self):
        """超长切片按字符数二次切分（500 字阈值）"""
        long_text = "这是一段视频讲解。关于运动学算法的调度与规划。内容非常详细。" * 40  # 远超 500 字
        segments = [
            {"time": 5, "type": "voice", "text": long_text},
        ]
        chunks = chunk_video_text(
            "v1", "a.mp4",
            segments,
            window_sec=30, max_chars=500,
        )
        # 应被切分为多个切片
        assert len(chunks) > 1
        # 每个切片不超过阈值+单句溢出（单句超长硬切后可能略超，允许小幅容差）
        for c in chunks:
            assert len(c.text) <= 500 + 100

    def test_segments_sorted_and_indexed(self):
        """切片按时间排序、chunk_index 连续"""
        segments = [
            {"time": 55, "type": "voice", "text": "后段内容"},
            {"time": 10, "type": "voice", "text": "前段内容"},
        ]
        chunks = chunk_video_text(
            "v1", "a.mp4", segments, window_sec=30, max_chars=500, duration_sec=90
        )
        assert chunks[0].start_sec == 0
        assert chunks[1].start_sec == 30
        assert [c.chunk_index for c in chunks] == [0, 1]
        assert all(isinstance(c, VideoChunk) for c in chunks)
        assert all(c.video_id == "v1" for c in chunks)
        assert all(c.file_name == "a.mp4" for c in chunks)
        assert all(c.created_at for c in chunks)