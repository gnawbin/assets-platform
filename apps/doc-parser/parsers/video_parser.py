"""视频解析器：ffmpeg 抽帧 OCR + Whisper 音频转写 → 带时间戳混合文本

通道1（语音）：ffmpeg 提取音频 → Whisper 转写（含断句时间戳）
通道2（画面）：ffmpeg 定时抽帧 → pytesseract OCR → 带时间戳文本
输出：raw_text = 语音 + 画面 OCR 混合文本；images = 帧路径（供 Rust VLM 并行解读）
"""

import hashlib
import os
import time
import tempfile
import ffmpeg
from models.parse_result import ParseResult
from parsers.audio_parser import AudioParser
from utils.ocr import ocr_image_path
import config


def _sha256(file_path: str) -> str:
    """计算文件 SHA-256（分块读取，避免大文件占满内存）"""
    h = hashlib.sha256()
    with open(file_path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def _fmt_ts(sec: float) -> str:
    """秒 → mm:ss"""
    sec = max(0, int(sec))
    return f"{sec // 60:02d}:{sec % 60:02d}"


def _txt_similarity(a: str, b: str) -> float:
    """简易文本相似度（字符集合 Jaccard），用于连续帧 OCR 去冗余"""
    if not a or not b:
        return 0.0
    sa, sb = set(a), set(b)
    if not sa or not sb:
        return 0.0
    return len(sa & sb) / len(sa | sb)


class VideoParser:
    """
    视频解析器。

    职责：
    - ffmpeg 提取音频 → Whisper 转写（语音通道）
    - ffmpeg 定时抽帧 → 每帧 OCR → 画面通道文本
    - 合并为带时间戳的混合文本（raw_text）
    - 帧路径列表进 images（Rust 负责逐帧 VLM 语义解读）

    核心参数：
    - frame_interval: 抽帧间隔（秒），默认 30（固定间隔兜底）
    - scene_detect:   是否启用场景变化检测抽帧（PPT 翻页即抽），默认 True
    - 视频过长仍全量抽帧（避免中间内容丢失），靠 OCR 相似去重控制冗余
    """

    DEFAULT_FRAME_INTERVAL = config.FRAME_INTERVAL_SEC
    SCENE_DETECT_THRESHOLD = 0.3  # 场景变化阈值，低于此值视为场景切换
    OCR_DUPLICATE_THRESHOLD = 0.9  # 连续帧 OCR 文本相似度高于此值则跳过

    async def parse(self, file_path: str, options: dict = None) -> ParseResult:
        start = time.time()
        options = options or {}
        frame_interval = options.get("frame_interval", self.DEFAULT_FRAME_INTERVAL)
        scene_detect = options.get("scene_detect", True)
        ocr_lang = options.get("ocr_language", config.OCR_LANGUAGE)

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

        # 2. 计算视频唯一 ID（SHA-256，用于幂等去重）
        video_id = _sha256(file_path)

        # 3. 语音通道（Whisper）
        voice_segments = []  # [{start, end, text}]
        audio_text = ""
        if has_audio:
            with tempfile.TemporaryDirectory() as audio_tmp:
                audio_path = os.path.join(audio_tmp, "audio.wav")
                ffmpeg.input(file_path).output(
                    audio_path, acodec="pcm_s16le", ac=1, ar=16000
                ).run(quiet=True, overwrite_output=True)

                audio_parser = AudioParser()
                audio_result = await audio_parser.parse(audio_path)
                audio_text = audio_result.raw_text
                voice_segments = list(audio_result.metadata.get("segments", []))

        # 4. 画面通道（抽帧 OCR）
        ocr_segments = []  # [{time, text}]
        with tempfile.TemporaryDirectory() as tmpdir:
            # 抽帧时间点：场景检测 + 固定间隔合并，保证全量覆盖
            frame_timestamps = self._build_frame_timestamps(
                video_path=file_path,
                duration=duration,
                frame_interval=frame_interval,
                scene_detect=scene_detect,
            )

            last_ocr_text = ""
            for t in frame_timestamps:
                if t >= duration:
                    continue
                frame_path = os.path.join(tmpdir, f"frame_{int(t):06d}.jpg")
                try:
                    ffmpeg.input(file_path, ss=t).output(
                        frame_path, vframes=1
                    ).run(quiet=True, overwrite_output=True, capture_stderr=True)

                    if not os.path.exists(frame_path):
                        continue
                    frame_paths.append(frame_path)

                    # 帧 OCR
                    try:
                        ocr_text = ocr_image_path(frame_path, lang=ocr_lang)
                    except Exception as e:
                        print(f"[VideoParser] OCR 失败 [{t}s]: {e}")
                        continue

                    clean = " ".join(ocr_text.split())
                    if not clean:
                        continue

                    # 连续帧 OCR 高度相似则跳过（去冗余）
                    if (
                        last_ocr_text
                        and _txt_similarity(last_ocr_text, clean)
                        >= self.OCR_DUPLICATE_THRESHOLD
                    ):
                        continue

                    last_ocr_text = clean
                    ocr_segments.append({"time": float(t), "text": clean})
                except Exception as e:
                    print(f"[VideoParser] 帧提取失败 [{t}s]: {e}")

        # 5. 合并为带时间戳的混合文本
        mixed_lines = []
        for seg in voice_segments:
            mixed_lines.append(
                f"[{_fmt_ts(seg['start'])}-{_fmt_ts(seg['end'])} 语音] {seg['text']}"
            )
        for ocr in ocr_segments:
            t = ocr["time"]
            mixed_lines.append(
                f"[{_fmt_ts(t)}-{_fmt_ts(t + frame_interval)} 画面] {ocr['text']}"
            )

        raw_text = "\n".join(mixed_lines)
        if not raw_text.strip():
            raw_text = audio_text  # 兜底：无语音无 OCR

        # 6. 组装返回结果
        metadata_segments = []
        for seg in voice_segments:
            metadata_segments.append(
                {"time": float(seg["start"]), "type": "voice", "text": seg["text"]}
            )
        for ocr in ocr_segments:
            metadata_segments.append(
                {"time": float(ocr["time"]), "type": "ocr", "text": ocr["text"]}
            )
        metadata_segments.sort(key=lambda x: float(x["time"]))

        return ParseResult(
            file_name=file_path.rsplit("/", 1)[-1],
            file_type="video",
            raw_text=raw_text,
            images=frame_paths,
            metadata={
                "video_id": video_id,
                "duration_sec": round(duration),
                "resolution": f"{width}x{height}",
                "has_audio": has_audio,
                "frames_extracted": len(frame_paths),
                "frame_interval_sec": frame_interval,
                "ocr_frames": len(ocr_segments),
                "segments": metadata_segments,
                "scene_detect": scene_detect,
                "parse_duration_ms": int((time.time() - start) * 1000),
            },
        )

    # ─── 抽帧时间点构建 ───────────────────────────────

    def _build_frame_timestamps(
        self,
        video_path: str,
        duration: float,
        frame_interval: int,
        scene_detect: bool,
    ) -> list[float]:
        """构建抽帧时间点列表

        - 场景检测：ffmpeg scene 滤镜，PPT 翻页/镜头切换即抽帧
        - 固定间隔兜底：与场景检测时间点合并，保证全量均匀覆盖
        """
        timestamps = set()

        # 方式1：场景变化检测
        if scene_detect:
            try:
                out, _ = (
                    ffmpeg.input(video_path, vsync="vfr")
                    .output(
                        "pipe:",
                        format="null",
                        vf=f"select='gt(scene,{self.SCENE_DETECT_THRESHOLD})',showinfo",
                    )
                    .run_async(quiet=True, pipe_stdout=True, pipe_stderr=True)
                )
                stderr = out.stderr.read().decode("utf-8", errors="ignore")
                out.wait()
                for line in stderr.splitlines():
                    if "pts_time:" in line:
                        try:
                            pts_str = line.split("pts_time:", 1)[1].split()[0]
                            timestamps.add(float(pts_str))
                        except (ValueError, IndexError):
                            continue
            except Exception as e:
                print(f"[VideoParser] 场景检测抽帧失败，回退固定间隔: {e}")

        # 方式2：固定间隔兜底（全量覆盖，不截断首尾）
        for t in range(0, int(duration), frame_interval):
            timestamps.add(float(t))

        return sorted(timestamps)