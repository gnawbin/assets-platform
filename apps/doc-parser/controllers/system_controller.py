"""系统 API 路由：GET /health、GET /formats"""

from fastapi import APIRouter

from models import HealthResponse
from parsers.audio_parser import AudioParser
from services import ParseService

router = APIRouter(tags=["system"])

parse_service = ParseService()


@router.get("/health", response_model=HealthResponse)
async def health():
    """健康检查（豁免认证）"""
    whisper_loaded = AudioParser._model is not None
    return HealthResponse(
        status="ok",
        version="1.0.0",
        vlm_mode="rust",  # VLM 描述由 Rust 网关调用
        whisper_loaded=whisper_loaded,
    )


@router.get("/formats")
async def formats():
    """查询支持的文件格式列表"""
    return parse_service.SUPPORTED_FORMATS