"""图片解析器：VLM 描述画面与文字内容"""

import time
from PIL import Image
from models.parse_result import ParseResult
from vlm import describe_image


class ImageParser:
    """图片解析器：Pillow 解码 → VLM 描述"""

    DEFAULT_PROMPT = (
        "请详细描述这张图片的内容。"
        "如果包含文字，请完整提取；"
        "如果包含图表/表格，请描述结构和数据；"
        "如果包含人物/场景，请描述细节。"
    )

    async def parse(self, file_path: str, options: dict = None) -> ParseResult:
        start = time.time()
        options = options or {}
        prompt = options.get("vlm_prompt", self.DEFAULT_PROMPT)

        # Pillow 解码获取元数据
        with Image.open(file_path) as img:
            width, height = img.size
            img_format = img.format or "unknown"
            mode = img.mode

        # VLM 描述
        description = await describe_image(file_path, prompt=prompt)

        return ParseResult(
            file_name=file_path.rsplit("/", 1)[-1],
            file_type="image",
            raw_text=description,
            metadata={
                "width": width,
                "height": height,
                "format": img_format,
                "mode": mode,
                "parse_duration_ms": int((time.time() - start) * 1000),
            },
        )