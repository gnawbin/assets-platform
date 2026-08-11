"""FastAPI 集成测试"""

import os
import tempfile

# 必须在导入 main 之前设置认证 token（config.py 在 import 时读取环境变量）
os.environ["DOC_PARSER_TOKEN"] = "test-token-123456"

from fastapi.testclient import TestClient
from main import app

client = TestClient(app)

AUTH_HEADERS = {"X-API-Token": os.environ["DOC_PARSER_TOKEN"]}


def _post_parse(file_path: str):
    """带认证头的 /parse 请求"""
    return client.post("/parse", json={"file_path": file_path}, headers=AUTH_HEADERS)


# ═══════════════════ 认证测试 ═══════════════════


def test_health_no_auth():
    """/health 应豁免认证"""
    resp = client.get("/health")
    assert resp.status_code == 200
    data = resp.json()
    assert data["status"] == "ok"
    assert data["version"] == "1.0.0"


def test_auth_missing_token():
    """未携带 token → 401"""
    resp = client.post("/parse", json={"file_path": "/tmp/a.pdf"})
    assert resp.status_code == 401
    assert resp.json()["error_code"] == "UNAUTHORIZED"


def test_auth_wrong_token():
    """token 错误 → 403"""
    resp = client.post(
        "/parse",
        json={"file_path": "/tmp/a.pdf"},
        headers={"X-API-Token": "wrong-token"},
    )
    assert resp.status_code == 403
    assert resp.json()["error_code"] == "FORBIDDEN"


def test_formats():
    """格式列表"""
    resp = client.get("/formats", headers=AUTH_HEADERS)
    assert resp.status_code == 200
    data = resp.json()
    assert "pdf" in data
    assert "document" in data
    assert "image" in data
    assert "audio" in data
    assert "video" in data
    assert "jpg" in data["image"]
    assert "docx" in data["document"]


def test_unsupported_format():
    """不支持的文件类型"""
    resp = _post_parse("/tmp/test.exe")
    assert resp.status_code == 400
    assert "不支持的文件类型" in resp.json()["detail"]


def test_file_not_found():
    """文件不存在"""
    resp = _post_parse("/nonexistent/file.pdf")
    assert resp.status_code == 400
    assert "文件不存在" in resp.json()["detail"]


def test_batch_mixed():
    """批量解析：混合有效和无效文件"""
    resp = client.post(
        "/parse/batch",
        json={
            "files": [
                {"file_path": "/tmp/exists.pdf", "options": {}},
                {"file_path": "/tmp/photo.jpg", "options": {}},
            ]
        },
        headers=AUTH_HEADERS,
    )
    assert resp.status_code == 200
    data = resp.json()
    # 两个文件都不存在，所以都在 failed 里
    assert len(data["failed"]) == 2
    assert len(data["results"]) == 0


def test_empty_file_path():
    """空 file_path"""
    resp = _post_parse("")
    assert resp.status_code == 400


def test_pdf_parse_small():
    """
    测试解析一个极小的有效 PDF。
    使用内存构造的含文本 PDF。
    """
    pdf_bytes = _create_minimal_pdf()
    with tempfile.NamedTemporaryFile(suffix=".pdf", delete=False) as f:
        f.write(pdf_bytes)
        tmp_path = f.name

    try:
        resp = _post_parse(tmp_path)
        assert resp.status_code == 200
        data = resp.json()
        assert data["file_type"] == "pdf"
        assert data["raw_text"] is not None
        assert "images" in data  # 响应包含 images 字段（PDF 默认空列表）
        assert data["images"] == []
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
