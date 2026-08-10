"""API 路由层"""
from .parse_controller import router as parse_router
from .system_controller import router as system_router
from .rag_controller import router as rag_router
from .workflow_controller import router as workflow_router

__all__ = ["parse_router", "system_router", "rag_router", "workflow_router"]
