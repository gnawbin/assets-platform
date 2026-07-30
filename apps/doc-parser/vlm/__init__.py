"""
VLM 统一入口。

自动故障切换逻辑：
1. 以配置的 VLM_MODE 为准
2. 当前模式调用失败 → 自动切换到另一种模式
3. 都失败 → 返回降级描述
"""

from vlm.ollama_client import OllamaClient
from vlm.cloud_client import CloudVLMClient
import config


async def describe_image(image_path: str, prompt: str = None) -> str:
    """统一的 VLM 调用入口（带自动故障切换）"""
    prompt = prompt or (
        "请详细描述这张图片的内容。"
        "如果包含文字，请完整提取；"
        "如果包含图表/表格，请描述结构和数据；"
        "如果包含人物/场景，请描述细节。"
    )

    modes_to_try = [
        config.VLM_MODE,
        "cloud" if config.VLM_MODE == "ollama" else "ollama",
    ]

    for mode in modes_to_try:
        try:
            if mode == "ollama":
                client = OllamaClient()
            else:
                client = CloudVLMClient()
            return await client.describe(image_path, prompt)
        except Exception as e:
            print(f"[VLM] {mode} 调用失败: {e}")
            continue

    # 都失败 → 降级
    return f"[图片描述生成失败: {image_path}]"


__all__ = ["describe_image"]