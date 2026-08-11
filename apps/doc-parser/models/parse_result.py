"""统一解析结果数据模型"""

from pydantic import BaseModel, Field
from typing import Optional


class ParseResult(BaseModel):
    """解析器的统一输出结构，所有模态都返回此格式"""

    file_name: str = Field(..., description="原文件名")
    file_type: str = Field(..., description="文件类型: pdf / document / image / audio / video")
    raw_text: str = Field(..., description="提取或生成的纯文本内容")
    images: list[str] = Field(
        default_factory=list,
        description="需要 VLM 语义描述的本地图片路径（图片原图 / 视频抽帧）",
    )
    metadata: dict = Field(default_factory=dict, description="可选元数据")


class ParseRequest(BaseModel):
    """解析请求参数"""

    file_path: str = Field(..., description="待解析文件的绝对路径")
    options: dict = Field(default_factory=dict, description="解析选项")


class BatchParseRequest(BaseModel):
    """批量解析请求"""

    files: list[ParseRequest] = Field(..., description="文件列表")


class BatchParseResult(BaseModel):
    """批量解析响应"""

    results: list[ParseResult] = Field(default_factory=list)
    failed: list[dict] = Field(default_factory=list)


class HealthResponse(BaseModel):
    """健康检查响应"""

    status: str = "ok"
    version: str = "1.0.0"
    vlm_mode: str = ""
    whisper_loaded: bool = False


# ───────────────────────────────────────────────────
# RAG 检索 / 问答模型（视频全量转文本与向量化记忆）
# ───────────────────────────────────────────────────


class SearchRequest(BaseModel):
    """向量检索请求"""

    query: str = Field(..., description="检索文本")
    top_k: int = Field(10, ge=1, le=50, description="返回条数（默认 10）")
    video_id: Optional[str] = Field(None, description="可选，限定某视频检索")
    permission_level: Optional[str] = Field(None, description="可选，权限等级过滤")


class SearchResultItem(BaseModel):
    """视频切片检索结果"""

    video_id: str = Field(..., description="视频唯一 ID")
    chunk_index: int = Field(..., description="切片序号")
    start_sec: float = Field(..., description="起始时间（秒）")
    end_sec: float = Field(..., description="结束时间（秒）")
    type: str = Field(..., description="切片类型: voice / ocr / mixed")
    content: str = Field(..., description="切片文本")
    score: float = Field(..., description="相似度（0~1，越大越相关）")
    file_name: Optional[str] = Field(None, description="原视频文件名")


class AskRequest(BaseModel):
    """RAG 问答请求"""

    query: str = Field(..., description="用户问题")
    top_k: int = Field(8, ge=1, le=50, description="检索切片数（默认 8）")
    video_id: Optional[str] = Field(None, description="可选，限定某视频")
    permission_level: Optional[str] = Field(None, description="可选，权限等级过滤")


class AskResponse(BaseModel):
    """RAG 问答响应"""

    answer: str = Field(..., description="LLM 生成的回答（引用视频时间点）")
    references: list[SearchResultItem] = Field(
        default_factory=list, description="引用的视频切片"
    )
