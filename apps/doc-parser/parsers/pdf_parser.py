"""PDF 解析器：先尝试文字提取，不够则 OCR 兜底"""

import time
import pdfplumber
from models.parse_result import ParseResult
from utils.ocr import ocr_image


class PdfParser:
    """PDF 解析器"""

    MIN_TEXT_LENGTH = 50  # 少于 50 字符 → 当作扫描件

    async def parse(self, file_path: str, options: dict = None) -> ParseResult:
        start = time.time()
        options = options or {}
        ocr_lang = options.get("ocr_language", "chi_sim+eng")

        page_count = 0
        pages = []

        with pdfplumber.open(file_path) as pdf:
            page_count = len(pdf.pages)
            for page in pdf.pages:
                text = page.extract_text() or ""
                pages.append(text)

        raw_text = "\n".join(pages)

        # 文字不足 → 扫描件 OCR 兜底
        if len(raw_text.strip()) < self.MIN_TEXT_LENGTH:
            raw_text = await self._ocr_fallback(file_path, lang=ocr_lang)
            source_type = "ocr"
        else:
            source_type = "text"

        return ParseResult(
            file_name=file_path.rsplit("/", 1)[-1],
            file_type="pdf",
            raw_text=raw_text,
            metadata={
                "page_count": page_count,
                "source_type": source_type,
                "parse_duration_ms": int((time.time() - start) * 1000),
            },
        )

    async def _ocr_fallback(self, file_path: str, lang: str) -> str:
        """OCR 兜底：每页转图片后识别"""
        from pdf2image import convert_from_path

        images = convert_from_path(file_path, dpi=300)
        texts = [ocr_image(img, lang=lang) for img in images]
        return "\n".join(texts)