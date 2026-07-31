"""视频解析器：ffmpeg 抽帧 + Whisper 音频转写 → 帧路径进 images（语义描述由 Rust 调用 VLM）"""

import os
import time
import tempfile
import ffmpeg
from models.parse_result import ParseResult
from parsers.audio_parser import AudioParser


class VideoParser:
    """
    视频解析器。

    职责：
    - ffmpeg 提取音频 → Whisper 转写（进 raw_text）
    - ffmpeg 定时抽帧 → 帧路径列表进 images（Rust 负责逐帧 VLM 解读）

    核心参数：
    - frame_interval: 抽帧间隔（秒），默认 30
    - 视频过长时自动降级（>30分钟只解析前后各5分钟）
    """

    MAX_DURATION_FOR_FULL = 1800  # 30 秒以上只解析首尾
    DEFAULT_FRAME_INTERVAL = 30

    async def parse(self, file_path: str, options: dict = None) -> ParseResult:
        start = time.time()
        options = options or {}
        frame_interval = options.get("frame_interval", self.DEFAULT_FRAME_INTERVAL)

        # 1. 获取视频元数据
        probe = ffmpeg.probe(file_path)
        video_stream = next(
            (s for s in probe["streams"] if s["codec_type"] == "video"), None
        )
        audio_stream = next(
            (s for s in probe["streams"] if s["codec_type"] == "audio"), None
        )

        duration = float(probe["format"].get("duration", 0))
        width = int(video_stream.get("width", 0)) if video_stream else 0
        height = int(video_stream.get("height", 0)) if video_stream else 0
        has_audio = audio_stream is not None
        frame_paths = []

        with tempfile.TemporaryDirectory() as tmpdir:
            # 2. 提取音频 → Whisper 转写
            audio_text = ""
            if has_audio:
                audio_path = os.path.join(tmpdir, "audio.wav")
                ffmpeg.input(file_path).output(
                    audio_path, acodec="pcm_s16le", ac=1, ar=16000
                ).run(quiet=True, overwrite_output=True)

                audio_parser = AudioParser()
                audio_result = await audio_parser.parse(audio_path)
                audio_text = audio_result.raw_text

            # 3. 定时抽帧 → 帧路径进 images（VLM 描述由 Rust 完成）
            if duration > self.MAX_DURATION_FOR_FULL:
                # 只解析前5分钟和后5分钟
                timestamps = (
                    list(range(0, 300, frame_interval))
                    + list(range(int(duration) - 300, int(duration), frame_interval))
                )
            else:
                timestamps = list(range(0, int(duration), frame_interval))

            for t in timestamps:
                frame_path = os.path.join(tmpdir, f"frame_{t:06d}.jpg")
                try:
                    ffmpeg.input(file_path, ss=t).output(
                        frame_path, vframes=1
                    ).run(quiet=True, overwrite_output=True, capture_stderr=True)

                    if os.path.exists(frame_path):
                        frame_paths.append(frame_path)
                except Exception as e:
                    print(f"[VideoParser] 帧提取失败 [{t}s]: {e}")

        return ParseResult(
            file_name=file_path.rsplit("/", 1)[-1],
            file_type="video",
            raw_text=audio_text,
            images=frame_paths,
            metadata={
                "duration_sec": round(duration),
                "resolution": f"{width}x{height}",
                "has_audio": has_audio,
                "frames_extracted": len(frame_paths),
                "frame_interval_sec": frame_interval,
                "parse_duration_ms": int((time.time() - start) * 1000),
            },
        )