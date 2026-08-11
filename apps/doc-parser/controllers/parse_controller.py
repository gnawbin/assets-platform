"""解析 API 路由：POST /parse、POST /parse/batch"""

from fastapi import APIRouter

from models import (
    ParseRequest,
    ParseResult,
    BatchParseRequest,
    BatchParseResult,
)
from services import ParseService

router = APIRouter(prefix="/parse", tags=["parse"])

# 进程级单例
parse_service = ParseService()


@router.post("", response_model=ParseResult)
async def parse_file(req: ParseRequest):
    """
    解析指定路径的文件，返回提取的纯文本内容。

    支持文件类型：PDF / Word / Excel / JPG / PNG / MP3 / WAV / MP4 / AVI ...
    """
    return await parse_service.detect_and_parse(req.file_path, req.options or {})


@router.post("/batch", response_model=BatchParseResult)
async def parse_batch(req: BatchParseRequest):
    """
    批量解析多个文件。
    """
    files = [(item.file_path, item.options or {}) for item in req.files]
    return await parse_service.parse_batch(files)