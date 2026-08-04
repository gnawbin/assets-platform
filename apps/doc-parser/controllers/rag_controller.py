"""RAG API 路由：POST /search（向量检索切片）、POST /ask（检索 + LLM 生成）

设计参考：docs/知识库模块/视频全量转文本与向量化记忆设计方案.md 第 6、8 章
"""

import httpx
from fastapi import APIRouter

from models import (
    AskRequest,
    AskResponse,
    SearchRequest,
    SearchResultItem,
)
from services.vector_store import vector_store
import config

router = APIRouter(prefix="", tags=["rag"])

# ─── Prompt 模板 ─────────────────────────────────────

SYSTEM_PROMPT = (
    "你是知识库助手。基于以下视频内容切片回答问题，"
    "并注明内容所在的视频时间点。若切片信息不足，请如实说明。"
)


async def _build_context(results: list[dict]) -> str:
    """将检索切片拼接为带时间戳的上下文"""
    lines = []
    for r in results:
        start = _fmt_sec(r.get("start_sec", 0))
        end = _fmt_sec(r.get("end_sec", 0))
        typ = r.get("type", "mixed")
        lines.append(f"[{start}-{end} {typ}] {r.get('content', '')}")
    return "\n".join(lines)


async def _ask_ollama(prompt: str) -> str:
    """调用 Ollama OpenAI 兼容 /v1/chat/completions 生成回答"""
    url = f"{config.OLLAMA_BASE_URL}/v1/chat/completions"
    payload = {
        "model": "qwen2.5:7b",  # 默认模型，可由环境变量扩展
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": prompt},
        ],
        "temperature": 0.3,
        "stream": False,
    }
    async with httpx.AsyncClient(timeout=60) as client:
        resp = await client.post(url, json=payload)
        resp.raise_for_status()
        data = resp.json()
        return data["choices"][0]["message"]["content"].strip()


def _fmt_sec(sec) -> str:
    """秒 → mm:ss"""
    sec = max(0, int(float(sec or 0)))
    return f"{sec // 60:02d}:{sec % 60:02d}"


# ─── 路由 ────────────────────────────────────────────


@router.post("/search", response_model=list[SearchResultItem])
async def search(req: SearchRequest):
    """向量检索视频切片（带时间戳、相似度）"""
    results = await vector_store.search(
        query=req.query,
        top_k=req.top_k,
        video_id=req.video_id,
        permission_level=req.permission_level,
    )
    return [SearchResultItem(**r) for r in results]


@router.post("/ask", response_model=AskResponse)
async def ask(req: AskRequest):
    """检索相关切片 → 拼接 Prompt → LLM 生成带时间引用的回答"""
    results = await vector_store.search(
        query=req.query,
        top_k=req.top_k,
        video_id=req.video_id,
        permission_level=req.permission_level,
    )

    if not results:
        return AskResponse(
            answer="未检索到与问题相关的视频内容切片。",
            references=[],
        )

    context = await _build_context(results)
    prompt = (
        f"【上下文】\n{context}\n\n"
        f"【问题】\n{req.query}\n\n"
        f"请基于以上视频内容切片回答，并注明内容所在的视频时间点。"
    )

    try:
        answer = await _ask_ollama(prompt)
    except Exception as e:
        # LLM 不可用时降级：拼接检索结果作为回答（保证接口可用）
        answer = (
            f"（LLM 生成失败: {e}）\n\n检索到的相关视频切片如下：\n{context}"
        )

    return AskResponse(
        answer=answer,
        references=[
            SearchResultItem(
                video_id=r["video_id"],
                chunk_index=r["chunk_index"],
                start_sec=r["start_sec"],
                end_sec=r["end_sec"],
                type=r["type"],
                content=r["content"],
                score=r["score"],
                file_name=r.get("file_name"),
            )
            for r in results
        ],
    )