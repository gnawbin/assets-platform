"""解析编排服务：负责文件类型识别与解析器分发"""

import os

from fastapi import HTTPException

from models import (
    ParseResult,
    BatchParseResult,
)
from parsers.pdf_parser import PdfParser
from parsers.image_parser import ImageParser
from parsers.audio_parser import AudioParser
from parsers.video_parser import VideoParser
from parsers.docs_parser import DocsParser


class ParseService:
    """文档解析编排服务

    职责：
    - 维护支持的文件格式映射
    - 根据扩展名分发到对应解析器
    - 提供单文件解析与批量解析（含失败收集）
    """

    # ─── 支持的文件格式 ─────────────────────────────────
    SUPPORTED_FORMATS = {
        "pdf": ["pdf"],
        "document": ["doc", "docx", "xls", "xlsx"],
        "image": ["jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff"],
        "audio": ["mp3", "wav", "ogg", "flac", "m4a", "aac"],
        "video": ["mp4", "avi", "mov", "mkv", "wmv", "flv"],
    }

    # 扁平化格式映射（扩展名 → 类型）
    EXT_TO_TYPE: dict[str, str] = {}
    for _type, exts in SUPPORTED_FORMATS.items():
        for ext in exts:
            EXT_TO_TYPE[ext] = _type

    def __init__(self):
        # 实例化解析器（进程级单例）
        self.pdf_parser = PdfParser()
        self.image_parser = ImageParser()
        self.audio_parser = AudioParser()
        self.video_parser = VideoParser()
        self.docs_parser = DocsParser()

    @staticmethod
    def _get_file_ext(file_path: str) -> str:
        """获取文件扩展名（小写，无点）"""
        return file_path.rsplit(".", 1)[-1].lower() if "." in file_path else ""

    async def detect_and_parse(self, file_path: str, options: dict | None = None) -> ParseResult:
        """根据文件扩展名选择解析器"""
        options = options or {}
        ext = self._get_file_ext(file_path)
        file_type = self.EXT_TO_TYPE.get(ext)

        if file_type is None:
            raise HTTPException(
                status_code=400,
                detail=f"不支持的文件类型: .{ext}",
            )

        if not os.path.exists(file_path):
            raise HTTPException(status_code=400, detail=f"文件不存在: {file_path}")

        if file_type == "pdf":
            return await self.pdf_parser.parse(file_path, options)
        elif file_type == "document":
            return await self.docs_parser.parse(file_path, options)
        elif file_type == "image":
            return await self.image_parser.parse(file_path, options)
        elif file_type == "audio":
            return await self.audio_parser.parse(file_path, options)
        elif file_type == "video":
            return await self.video_parser.parse(file_path, options)

        raise HTTPException(status_code=500, detail=f"解析器未实现: {file_type}")

    async def parse_batch(
        self, files: list[tuple[str, dict | None]]
    ) -> BatchParseResult:
        """批量解析，单个失败不影响其他文件"""
        results = []
        failed = []

        for file_path, options in files:
            try:
                result = await self.detect_and_parse(file_path, options)
                results.append(result)
            except Exception as e:
                failed.append({"file_path": file_path, "error": str(e)})

        return BatchParseResult(results=results, failed=failed)