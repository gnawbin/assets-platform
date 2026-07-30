"""视频解析器：分离音频转写 + 定时抽帧 VLM 解读 → 合并"""

import os
import time
import tempfile
import ffmpeg
from models.parse_result import ParseResult
from parsers.audio_parser import AudioParser
from vlm import describe_image


class VideoParser:
    """
    视频解析器。

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

            # 3. 定时抽帧 → VLM 解读
            if duration > self.MAX_DURATION_FOR_FULL:
                # 只解析前5分钟和后5分钟
                timestamps = (
                    list(range(0, 300, frame_interval))
                    + list(range(int(duration) - 300, int(duration), frame_interval))
                )
            else:
                timestamps = list(range(0, int(duration), frame_interval))

            frame_descriptions = []
            for t in timestamps:
                frame_path = os.path.join(tmpdir, f"frame_{t:06d}.jpg")
                try:
                    ffmpeg.input(file_path, ss=t).output(
                        frame_path, vframes=1
                    ).run(quiet=True, overwrite_output=True, capture_stderr=True)

                    if os.path.exists(frame_path):
                        desc = await describe_image(frame_path)
                        frame_descriptions.append(f"[{t}s] {desc}")
                except Exception as e:
                    frame_descriptions.append(f"[{t}s] 帧提取失败: {e}")

            # 4. 合并文本
            if audio_text and frame_descriptions:
                combined = (
                    f"【音频文稿】\n{audio_text}\n\n"
                    f"【关键帧解读】\n" + "\n".join(frame_descriptions)
                )
            elif audio_text:
                combined = audio_text
            elif frame_descriptions:
                combined = "【关键帧解读】\n" + "\n".join(frame_descriptions)
            else:
                combined = ""

        return ParseResult(
            file_name=file_path.rsplit("/", 1)[-1],
            file_type="video",
            raw_text=combined,
            metadata={
                "duration_sec": round(duration),
                "resolution": f"{width}x{height}",
                "has_audio": has_audio,
                "frames_analyzed": len(frame_descriptions),
                "parse_duration_ms": int((time.time() - start) * 1000),
            },
        )