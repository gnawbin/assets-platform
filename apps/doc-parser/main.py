"""
doc-parser — 多模态文档解析服务

一个专注于「非文本 → 纯文本」转换的轻量解析引擎。
通过 HTTP API 为 Tauri/Rust 后端提供文档解析能力。

认证：除 /health 及 Swagger/ReDoc 文档（/docs、/redoc、/openapi.json 等）外，
所有请求需携带 X-API-Token 请求头
（token 由 Tauri 启动时通过环境变量 DOC_PARSER_TOKEN 注入）

启动方式：
    python -m uvicorn main:app --host 127.0.0.1 --port 8321 --reload
"""

import hmac

import config
from fastapi import FastAPI, HTTPException, Request
from fastapi.openapi.utils import get_openapi
from fastapi.responses import JSONResponse

from controllers import parse_router, system_router, rag_router, workflow_router

app = FastAPI(
    title="doc-parser",
    description="多模态文档解析服务：PDF / Word / Excel / 图片 / 音频 / 视频 → 纯文本",
    version="1.0.0",
)


# ═══════════════════ 公开路径（豁免认证） ═══════════════════
# 仅本机监听（127.0.0.1）：健康检查 + Swagger/ReDoc/OpenAPI 文档。
# 文档端点只暴露接口结构，真正的解析/检索/工作流端点仍受 X-API-Token 保护。
PUBLIC_PATHS = {
    "/health",
    "/docs",
    "/redoc",
    "/openapi.json",
    "/docs/oauth2-redirect",
}


def _token_matches(token: str) -> bool:
    """常量时间比较，防时序攻击"""
    return hmac.compare_digest(token.encode(), config.API_TOKEN.encode())


# ═══════════════════ 认证中间件 ═══════════════════


@app.middleware("http")
async def auth_middleware(request: Request, call_next):
    """除 PUBLIC_PATHS（/health、/docs、/openapi.json 等）外，所有请求校验 X-API-Token"""
    if request.url.path in PUBLIC_PATHS:
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
app.include_router(rag_router)
app.include_router(workflow_router)


# ═══════════════════ Swagger 安全声明 ═══════════════════
# 仅用于文档展示（Swagger 右上角出现 Authorize 按钮，填入 token 后可直接调试接口），
# 实际鉴权仍由上方 auth_middleware 强制，业务端点不可绕过。


def _custom_openapi():
    if app.openapi_schema:
        return app.openapi_schema
    schema = get_openapi(
        title="doc-parser",
        version="1.0.0",
        description="多模态文档解析服务：PDF / Word / Excel / 图片 / 音频 / 视频 → 纯文本",
        routes=app.routes,
    )
    schema.setdefault("components", {}).setdefault("securitySchemes", {})[
        "X-API-Token"
    ] = {
        "type": "apiKey",
        "in": "header",
        "name": "X-API-Token",
        "description": "动态认证令牌（Tauri 启动 doc-parser 时经 DOC_PARSER_TOKEN 注入）",
    }
    schema["security"] = [{"X-API-Token": []}]
    app.openapi_schema = schema
    return schema


app.openapi = _custom_openapi


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