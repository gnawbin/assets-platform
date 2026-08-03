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
import config
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import JSONResponse

from controllers import parse_router, system_router

app = FastAPI(
    title="doc-parser",
    description="多模态文档解析服务：PDF / Word / Excel / 图片 / 音频 / 视频 → 纯文本",
    version="1.0.0",
)


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


# ═══════════════════ 路由注册 ═══════════════════

app.include_router(parse_router)
app.include_router(system_router)


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