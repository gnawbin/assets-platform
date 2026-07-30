"""云端 VLM 客户端（经 Rust llm_gateway_service 中转）"""

import base64
import httpx
from vlm.base import VLMProvider
import config


class CloudVLMClient(VLMProvider):
    """
    云端 VLM 客户端。

    不直接存储 API Key，所有请求通过 Rust 后端的 llm_gateway_service 转发。
    Rust 侧负责：厂商路由、负载均衡、API Key 管理、熔断重试。
    Python 只做：图片 base64 → 构造多模态消息 → HTTP 转发 → 获取回复。
    """

    def __init__(self):
        self.gateway_url = config.RUST_GATEWAY_URL.rstrip("/")

    async def describe(self, image_path: str, prompt: str = None) -> str:
        prompt = prompt or "请详细描述这张图片的内容。"

        with open(image_path, "rb") as f:
            b64 = base64.b64encode(f.read()).decode()

        # 构造 OpenAI 兼容的多模态消息
        messages = [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": prompt},
                    {
                        "type": "image_url",
                        "image_url": {"url": f"data:image/jpeg;base64,{b64}"},
                    },
                ],
            }
        ]

        async with httpx.AsyncClient(timeout=120) as client:
            resp = await client.post(
                f"{self.gateway_url}/api/llm/chat",
                json={
                    "messages": messages,
                    "model_type": "vision",
                    "stream": False,
                    "temperature": 0.1,
                },
            )
            resp.raise_for_status()
            return resp.json()["content"]