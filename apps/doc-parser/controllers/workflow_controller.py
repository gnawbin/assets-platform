"""工作流 API 路由：POST /workflow/execute（LangGraph 执行）

设计参考：docs/知识库模块/AI工作流编排器设计方案.md 第 6、7 章
"""

from fastapi import APIRouter

from models.workflow_models import (
    WorkflowExecuteRequest,
    WorkflowExecuteResponse,
    WorkflowNodeResult,
)
from services.workflow_executor import execute_workflow

router = APIRouter(prefix="/workflow", tags=["workflow"])


@router.post("/execute", response_model=WorkflowExecuteResponse)
async def execute(req: WorkflowExecuteRequest):
    """执行工作流（JSON 定义 → LangGraph StateGraph → 执行）

    - 完整工作流定义内联传入（暂不查 workflow 表）
    - 返回每个节点的执行结果 + 总耗时
    """
    result = execute_workflow(req.workflow, req.input_data)

    return WorkflowExecuteResponse(
        success=result["success"],
        result=result["result"],
        node_results=[
            WorkflowNodeResult(
                node_id=r["node_id"],
                status=r["status"],
                output=r["output"],
                duration_ms=r["duration_ms"],
                error=r["error"],
            )
            for r in result["node_results"]
        ],
        total_duration_ms=result["total_duration_ms"],
        error=result["error"],
    )