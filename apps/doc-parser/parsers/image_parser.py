"""图片解析器：Pillow 解码 + OCR 提取文字 → 产出图片路径（语义描述由 Rust 调用 VLM）"""

import time
from PIL import Image
from models.parse_result import ParseResult
from utils.ocr import ocr_image_path
import config


class ImageParser:
    """图片解析器：只做解析（元数据 + OCR 文字），原图路径交给 Rust 做 VLM 语义描述"""

    async def parse(self, file_path: str, options: dict = None) -> ParseResult:
        start = time.time()
        options = options or {}
        ocr_lang = options.get("ocr_language", config.OCR_LANGUAGE)

        # 1. Pillow 解码获取元数据
        with Image.open(file_path) as img:
            width, height = img.size
            img_format = img.format or "unknown"
            mode = img.mode

        # 2. OCR 提取图中文字（失败则降级为空文本）
        ocr_text = ""
        try:
            ocr_text = ocr_image_path(file_path, lang=ocr_lang).strip()
        except Exception as e:
            print(f"[ImageParser] OCR 失败: {e}")

        # 3. 原图路径进 images → Rust 负责 VLM 语义描述
        return ParseResult(
            file_name=file_path.rsplit("/", 1)[-1],
            file_type="image",
            raw_text=ocr_text,
            images=[file_path],
            metadata={
                "width": width,
                "height": height,
                "format": img_format,
                "mode": mode,
                "parse_duration_ms": int((time.time() - start) * 1000),
            },
        )