"""本地 Ollama VLM 客户端"""

import base64
import httpx
from vlm.base import VLMProvider
import config


class OllamaClient(VLMProvider):
    """
    Ollama 本地 VLM 客户端。
    支持模型：llava, llava:13b, llama3.2-vision, bakllava 等
    """

    def __init__(self):
        self.base_url = config.OLLAMA_BASE_URL.rstrip("/")
        self.model = config.OLLAMA_VLM_MODEL

    async def describe(self, image_path: str, prompt: str = None) -> str:
        prompt = prompt or "请详细描述这张图片的内容。"

        with open(image_path, "rb") as f:
            b64 = base64.b64encode(f.read()).decode()

        payload = {
            "model": self.model,
            "prompt": prompt,
            "images": [b64],
            "stream": False,
            "options": {"temperature": 0.1},
        }

        async with httpx.AsyncClient(timeout=60) as client:
            resp = await client.post(f"{self.base_url}/api/generate", json=payload)
            resp.raise_for_status()
            return resp.json()["response"]