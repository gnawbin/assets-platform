"""视频解析测试：VideoParser 将目标视频转为文本（集成测试）

用法（在 apps/doc-parser 目录下执行）：
    python -m pytest tests/test_video_parse.py -v

说明：
- 调用 doc-parser 的 VideoParser（ffmpeg 提取音频 → Whisper 转写 + 帧 OCR）
- 混合文本保存到 tests/output/1-1_工业机器人控制软件非实时系统组成.txt
"""

import os

import pytest

from parsers.video_parser import VideoParser

# 目标视频文件（绝对路径）
VIDEO_FILE = r"/mnt/c/project/ceshi.mp4"

# 转写文本输出文件名（保存到 tests/output/ 下）
OUTPUT_FILE = "1-1_工业机器人控制软件非实时系统组成.txt"


def _output_dir() -> str:
    """转写文本输出目录：apps/doc-parser/tests/output"""
    return os.path.join(os.path.dirname(__file__), "output")


def _output_path() -> str:
    return os.path.join(_output_dir(), OUTPUT_FILE)


class TestVideoParse:
    """视频 → 文本 转写测试类"""

    def test_video_file_exists(self):
        """目标视频文件必须存在"""
        assert os.path.exists(VIDEO_FILE), (
            f"目标视频不存在: {VIDEO_FILE}\n请确认文件位置，或修改测试中的 VIDEO_FILE 路径。"
        )

    @pytest.mark.asyncio
    async def test_video_parse_to_text(self):
        """调用 VideoParser 将视频转写为文本，并保存结果文件"""
        # 1. 解析视频
        parser = VideoParser()
        result = await parser.parse(VIDEO_FILE)

        # 2. 断言解析结果
        assert result.file_type == "video"
        assert os.path.basename(VIDEO_FILE) in result.file_name, (
            f"file_name 应包含视频文件名，实际: {result.file_name}"
        )
        assert result.raw_text and len(result.raw_text.strip()) > 10, (
            f"转写文本为空或过短（实际长度: {len(result.raw_text.strip())}）\n"
            "可能原因：视频无音轨 / Whisper 未识别出语音 / 模型下载失败。"
        )
        assert result.metadata.get("has_audio") is True
        assert result.metadata.get("duration_sec", 0) > 0

        # 全量转文本增强断言：video_id（SHA-256）+ 带时间戳 segments
        assert result.metadata.get("video_id"), "metadata 应包含 video_id（SHA-256）"
        segments = result.metadata.get("segments", [])
        assert len(segments) > 0, "metadata 应包含带时间戳的 segments"
        types = {s["type"] for s in segments}
        assert "voice" in types, "语音转写段应存在"
        print(
            f"\n视频时长: {result.metadata['duration_sec']}s | "
            f"帧数: {result.metadata.get('frames_extracted', 0)} | "
            f"OCR 帧数: {result.metadata.get('ocr_frames', 0)} | "
            f"文本段数: {len(segments)} | "
            f"解析耗时: {result.metadata.get('parse_duration_ms', 0)}ms"
        )
        print(f"混合文本预览（前 300 字）:\n{result.raw_text.strip()[:300]}")

        # 3. 保存转写文本（便于直接查看完整结果）
        os.makedirs(_output_dir(), exist_ok=True)
        with open(_output_path(), "w", encoding="utf-8") as f:
            f.write(result.raw_text.strip())

        # 4. 确认文本文件已生成且非空
        assert os.path.exists(_output_path()), f"转写文本文件未生成: {_output_path()}"
        with open(_output_path(), "r", encoding="utf-8") as f:
            saved = f.read()
        assert len(saved.strip()) > 0
        print(f"\n转写文本已保存: {_output_path()}（共 {len(saved)} 字）")