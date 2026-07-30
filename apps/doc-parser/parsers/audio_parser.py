"""音频解析器：Whisper 语音转写"""

import time
import whisper
from models.parse_result import ParseResult


class AudioParser:
    """
    音频解析器：Whisper 语音转写。

    模型选择（配置 WHISPER_MODEL）：
    - tiny:   最快，~1GB VRAM
    - base:   平衡速度与准确度（默认）
    - small:  准确度较好，~2GB VRAM
    - medium: 更准确，~5GB VRAM
    - large:  最准确，~10GB VRAM
    """

    _model = None  # 进程级单例，只加载一次

    def _get_model(self, model_name: str = "base"):
        if self._model is None or getattr(self._model, "_model_name", None) != model_name:
            self._model = whisper.load_model(model_name)
            self._model._model_name = model_name  # 记录模型名
        return self._model

    async def parse(self, file_path: str, options: dict = None) -> ParseResult:
        start = time.time()
        options = options or {}
        model_name = options.get("whisper_model", "base")

        model = self._get_model(model_name)
        result = model.transcribe(
            file_path,
            language=None,       # 自动检测语言
            task="transcribe",   # transcribe=转写, translate=翻译成英文
            verbose=False,
        )

        return ParseResult(
            file_name=file_path.rsplit("/", 1)[-1],
            file_type="audio",
            raw_text=result["text"],
            metadata={
                "duration_sec": round(result.get("duration", 0)),
                "language": result.get("language", "unknown"),
                "segments_count": len(result.get("segments", [])),
                "whisper_model": model_name,
                "parse_duration_ms": int((time.time() - start) * 1000),
            },
        )