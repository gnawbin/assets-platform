"""
doc-parser — 多模态文档解析服务

一个专注于「非文本 → 纯文本」转换的轻量解析引擎。
通过 HTTP API 为 Tauri/Rust 后端提供文档解析能力。

认证：除 /health 外，所有请求需携带 X-API-Token 请求头
（token 由 Tauri 启动时通过环境变量 DOC_PARSER_TOKEN 注入）

启动方式：
    python -m uvicorn main:app --host 127.0.0.1 --port 8321 --reload
"""

import hmac
import os
import config
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import JSONResponse

from models import (
    ParseResult,
    ParseRequest,
    BatchParseRequest,
    BatchParseResult,
    HealthResponse,
)
from parsers.pdf_parser import PdfParser
from parsers.image_parser import ImageParser
from parsers.audio_parser import AudioParser
from parsers.video_parser import VideoParser
from parsers.docs_parser import DocsParser

app = FastAPI(
    title="doc-parser",
    description="多模态文档解析服务：PDF / Word / Excel / 图片 / 音频 / 视频 → 纯文本",
    version="1.0.0",
)

# 实例化解析器
pdf_parser = PdfParser()
image_parser = ImageParser()
audio_parser = AudioParser()
video_parser = VideoParser()
docs_parser = DocsParser()

# ─── 支持的文件格式 ─────────────────────────────────
SUPPORTED_FORMATS = {
    "pdf": ["pdf"],
    "document": ["doc", "docx", "xls", "xlsx"],
    "image": ["jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff"],
    "audio": ["mp3", "wav", "ogg", "flac", "m4a", "aac"],
    "video": ["mp4", "avi", "mov", "mkv", "wmv", "flv"],
}

# 扁平化格式映射（扩展名 → 类型）
EXT_TO_TYPE = {}
for _type, exts in SUPPORTED_FORMATS.items():
    for ext in exts:
        EXT_TO_TYPE[ext] = _type


def _get_file_ext(file_path: str) -> str:
    """获取文件扩展名（小写，无点）"""
    return file_path.rsplit(".", 1)[-1].lower() if "." in file_path else ""


def _token_matches(token: str) -> bool:
    """常量时间比较，防时序攻击"""
    return hmac.compare_digest(token.encode(), config.API_TOKEN.encode())


# ═══════════════════ 认证中间件 ═══════════════════


@app.middleware("http")
async def auth_middleware(request: Request, call_next):
    """除 /health 外，所有请求校验 X-API-Token"""
    if request.url.path == "/health":
        return await call_next(request)

    token = request.headers.get("X-API-Token")
    if token is None:
        return JSONResponse(
            status_code=401,
            content={"detail": "缺少认证令牌", "error_code": "UNAUTHORIZED"},
        )
    if not _token_matches(token):
        return JSONResponse(
            status_code=403,
            content={"detail": "认证令牌无效", "error_code": "FORBIDDEN"},
        )

    return await call_next(request)


# ═══════════════════ 解析逻辑 ═══════════════════


async def _detect_and_parse(file_path: str, options: dict) -> ParseResult:
    """根据文件扩展名选择解析器"""
    ext = _get_file_ext(file_path)
    file_type = EXT_TO_TYPE.get(ext)

    if file_type is None:
        raise HTTPException(
            status_code=400,
            detail=f"不支持的文件类型: .{ext}",
        )

    if not os.path.exists(file_path):
        raise HTTPException(status_code=400, detail=f"文件不存在: {file_path}")

    if file_type == "pdf":
        return await pdf_parser.parse(file_path, options)
    elif file_type == "document":
        return await docs_parser.parse(file_path, options)
    elif file_type == "image":
        return await image_parser.parse(file_path, options)
    elif file_type == "audio":
        return await audio_parser.parse(file_path, options)
    elif file_type == "video":
        return await video_parser.parse(file_path, options)

    raise HTTPException(status_code=500, detail=f"解析器未实现: {file_type}")


# ═══════════════════ API 路由 ═══════════════════


@app.post("/parse", response_model=ParseResult)
async def parse_file(req: ParseRequest):
    """
    解析指定路径的文件，返回提取的纯文本内容。

    支持文件类型：PDF / Word / Excel / JPG / PNG / MP3 / WAV / MP4 / AVI ...
    """
    return await _detect_and_parse(req.file_path, req.options or {})


@app.post("/parse/batch", response_model=BatchParseResult)
async def parse_batch(req: BatchParseRequest):
    """
    批量解析多个文件。
    """
    results = []
    failed = []

    for item in req.files:
        try:
            result = await _detect_and_parse(item.file_path, item.options or {})
            results.append(result)
        except Exception as e:
            failed.append({"file_path": item.file_path, "error": str(e)})

    return BatchParseResult(results=results, failed=failed)


@app.get("/health", response_model=HealthResponse)
async def health():
    """健康检查（豁免认证）"""
    whisper_loaded = AudioParser._model is not None
    return HealthResponse(
        status="ok",
        version="1.0.0",
        vlm_mode="rust",  # VLM 描述由 Rust 网关调用
        whisper_loaded=whisper_loaded,
    )


@app.get("/formats")
async def formats():
    """查询支持的文件格式列表"""
    return SUPPORTED_FORMATS


# ═══════════════════ 全局异常处理 ═══════════════════


@app.exception_handler(Exception)
async def global_exception_handler(request, exc):
    if isinstance(exc, HTTPException):
        return JSONResponse(
            status_code=exc.status_code,
            content={"detail": exc.detail, "error_code": "PARSE_ERROR"},
        )
    return JSONResponse(
        status_code=500,
        content={
            "detail": f"解析过程异常: {str(exc)}",
            "error_code": "PARSE_FAILED",
        },
    )


# ═══════════════════ 直接运行 ═══════════════════

if __name__ == "__main__":
    import uvicorn

    uvicorn.run(
        "main:app",
        host=config.PARSER_HOST,
        port=config.PARSER_PORT,
        reload=True,
    )