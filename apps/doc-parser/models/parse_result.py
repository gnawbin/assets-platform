"""统一解析结果数据模型"""

from pydantic import BaseModel, Field
from typing import Optional


class ParseResult(BaseModel):
    """解析器的统一输出结构，所有模态都返回此格式"""

    file_name: str = Field(..., description="原文件名")
    file_type: str = Field(..., description="文件类型: pdf / image / audio / video")
    raw_text: str = Field(..., description="提取或生成的纯文本内容")
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