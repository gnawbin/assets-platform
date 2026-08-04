"""ParseService 单元测试（不依赖 HTTP）"""

import os
import tempfile

import pytest
from fastapi import HTTPException

# 必须在导入 config 之前设置认证 token（config.py 在 import 时读取环境变量）
os.environ["DOC_PARSER_TOKEN"] = "test-token-123456"

from services import ParseService


@pytest.fixture(scope="module")
def service():
    return ParseService()


def test_supported_formats_contains_all_types(service):
    """支持的文件格式映射完整"""
    data = service.SUPPORTED_FORMATS
    assert "pdf" in data
    assert "document" in data
    assert "image" in data
    assert "audio" in data
    assert "video" in data
    assert "jpg" in data["image"]
    assert "docx" in data["document"]


def test_get_file_ext_lowercase(service):
    """扩展名提取为小写无点"""
    assert service._get_file_ext("/tmp/Photo.JPG") == "jpg"
    assert service._get_file_ext("/tmp/noext") == ""
    assert service._get_file_ext("/tmp/file.tar.gz") == "gz"


@pytest.mark.asyncio
async def test_unsupported_format_raises(service):
    """不支持的文件类型 → HTTPException 400"""
    with pytest.raises(HTTPException) as exc_info:
        await service.detect_and_parse("/tmp/test.exe")
    assert exc_info.value.status_code == 400
    assert "不支持的文件类型" in exc_info.value.detail


@pytest.mark.asyncio
async def test_file_not_found_raises(service):
    """文件不存在 → HTTPException 400"""
    with pytest.raises(HTTPException) as exc_info:
        await service.detect_and_parse("/nonexistent/file.pdf")
    assert exc_info.value.status_code == 400
    assert "文件不存在" in exc_info.value.detail


@pytest.mark.asyncio
async def test_empty_file_path_raises(service):
    """空 file_path → HTTPException 400"""
    with pytest.raises(HTTPException):
        await service.detect_and_parse("")


@pytest.mark.asyncio
async def test_parse_batch_collects_failures(service):
    """批量解析：失败单独收集，不影响其他"""
    result = await service.parse_batch(
        [
            ("/tmp/exists.pdf", {}),
            ("/tmp/photo.jpg", {}),
        ]
    )
    assert len(result.results) == 0
    assert len(result.failed) == 2
    assert result.failed[0]["file_path"] == "/tmp/exists.pdf"
    assert "error" in result.failed[0]


@pytest.mark.asyncio
async def test_parse_batch_success_with_real_pdf(service):
    """批量解析：真实 PDF 应成功进入 results"""
    pdf_bytes = _create_minimal_pdf()
    with tempfile.NamedTemporaryFile(suffix=".pdf", delete=False) as f:
        f.write(pdf_bytes)
        tmp_path = f.name

    try:
        result = await service.parse_batch([(tmp_path, {})])
        assert len(result.results) == 1
        assert len(result.failed) == 0
        assert result.results[0].file_type == "pdf"
        assert result.results[0].raw_text is not None
    finally:
        os.unlink(tmp_path)


def _create_minimal_pdf() -> bytes:
    """构造一个最小的 PDF（含 "Hello World" 文本）"""
    header = b"%PDF-1.4\n"
    obj1 = b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n"
    obj2 = b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n"
    obj3 = (
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R "
        b"/MediaBox [0 0 612 792] /Contents 4 0 R "
        b"/Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n"
    )
    stream_data = b"BT /F1 12 Tf 100 700 Td (Hello World) Tj ET\n"
    obj4 = (
        b"4 0 obj\n<< /Length 44 >>\nstream\n"
        + stream_data
        + b"\nendstream\nendobj\n"
    )
    obj5 = b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n"

    parts = [header, obj1, obj2, obj3, obj4, obj5]
    offsets = []
    pos = 0
    for p in parts:
        offsets.append(pos)
        pos += len(p)
    offset_xref = pos

    xref = b"xref\n0 6\n0000000000 65535 f \n"
    for off in offsets:
        xref += f"{off:010d} 00000 n \n".encode()
    trailer = f"trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{offset_xref}\n%%EOF\n".encode()

    result = b"".join(parts) + xref + trailer
    return result
