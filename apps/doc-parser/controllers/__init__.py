"""API 路由层"""
from .parse_controller import router as parse_router
from .system_controller import router as system_router

__all__ = ["parse_router", "system_router"]