"""VLM 提供者抽象接口"""

from abc import ABC, abstractmethod


class VLMProvider(ABC):
    """VLM 提供者抽象：所有实现必须支持 describe(image_path, prompt) → text"""

    @abstractmethod
    async def describe(self, image_path: str, prompt: str = None) -> str:
        """描述图片内容，返回文字"""
        ...