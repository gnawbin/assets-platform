"""工作流执行器测试（LangGraph）

覆盖：
- 最小 DAG（trigger → code）执行成功
- 条件分支（yes/no）
- skill 节点占位报错
- 空节点工作流报错
- API 层 /workflow/execute（认证 + 响应结构）
"""

import os

# 与 test_api.py 保持一致（config 在 import 时读取环境变量）
os.environ["DOC_PARSER_TOKEN"] = "test-token-123456"

import pytest
from fastapi.testclient import TestClient

from services.workflow_executor import execute_workflow

pytestmark = pytest.mark.workflow


# ═══════════════════ 执行器测试 ═══════════════════


def test_trigger_code_success():
    """最小 DAG：trigger → code，code 表达式计算结果"""
    wf = {
        "name": "calc",
        "nodes": [
            {"id": "n_trigger", "type": "trigger", "label": "开始", "config": {}},
            {"id": "n_code", "type": "code", "label": "计算", "config": {"code": "input['amount'] * 2"}},
        ],
        "edges": [
            {"source": "n_trigger", "target": "n_code"},
        ],
    }
    result = execute_workflow(wf, {"amount": 21})

    assert result["success"] is True
    assert result["error"] is None
    assert result["result"]["result"] == 42
    # 每个节点都有执行记录
    assert len(result["node_results"]) == 2


def test_condition_yes_branch():
    """条件分支：amount > 100 走 yes 分支"""
    wf = {
        "name": "cond",
        "nodes": [
            {"id": "n_trigger", "type": "trigger", "config": {}},
            {
                "id": "n_cond",
                "type": "condition",
                "config": {
                    "field": "amount",
                    "operator": ">",
                    "value": 100,
                    "yes_label": "yes",
                    "no_label": "no",
                },
            },
            {"id": "n_yes", "type": "code", "config": {"code": "'big'"}},
            {"id": "n_no", "type": "code", "config": {"code": "'small'"}},
        ],
        "edges": [
            {"source": "n_trigger", "target": "n_cond"},
            {"source": "n_cond", "target": "n_yes", "label": "yes"},
            {"source": "n_cond", "target": "n_no", "label": "no"},
        ],
    }

    result = execute_workflow(wf, {"amount": 500})

    assert result["success"] is True
    assert result["result"]["result"] == "big"


def test_condition_no_branch():
    """条件分支：amount <= 100 走 no 分支"""
    wf = {
        "name": "cond",
        "nodes": [
            {"id": "n_trigger", "type": "trigger", "config": {}},
            {
                "id": "n_cond",
                "type": "condition",
                "config": {
                    "field": "amount",
                    "operator": ">",
                    "value": 100,
                    "yes_label": "yes",
                    "no_label": "no",
                },
            },
            {"id": "n_yes", "type": "code", "config": {"code": "'big'"}},
            {"id": "n_no", "type": "code", "config": {"code": "'small'"}},
        ],
        "edges": [
            {"source": "n_trigger", "target": "n_cond"},
            {"source": "n_cond", "target": "n_yes", "label": "yes"},
            {"source": "n_cond", "target": "n_no", "label": "no"},
        ],
    }

    result = execute_workflow(wf, {"amount": 50})

    assert result["success"] is True
    assert result["result"]["result"] == "small"


def test_skill_placeholder_error():
    """skill 节点为占位，执行时应返回未实现错误"""
    wf = {
        "name": "skill",
        "nodes": [
            {"id": "n_skill", "type": "skill", "label": "文档解析", "config": {}},
        ],
        "edges": [],
    }

    result = execute_workflow(wf, {})

    assert result["success"] is False
    assert result["error"] is not None
    assert "skill" in result["error"]


def test_empty_nodes_error():
    """空节点工作流应报错"""
    wf = {"name": "empty", "nodes": [], "edges": []}

    result = execute_workflow(wf, {})

    assert result["success"] is False
    assert "nodes" in result["error"]


# ═══════════════════ API 测试 ═══════════════════


def test_workflow_execute_api_auth():
    """未携带 token → 401"""
    from main import app

    client = TestClient(app)
    resp = client.post("/workflow/execute", json={})
    assert resp.status_code == 401
    assert resp.json()["error_code"] == "UNAUTHORIZED"


def test_workflow_execute_api_success():
    """带认证执行最小工作流 → 200 + 响应结构"""
    from main import app

    client = TestClient(app)
    headers = {"X-API-Token": "test-token-123456"}
    payload = {
        "workflow": {
            "name": "api-calc",
            "nodes": [
                {"id": "n_trigger", "type": "trigger", "config": {}},
                {"id": "n_code", "type": "code", "config": {"code": "input['x'] + 1"}},
            ],
            "edges": [{"source": "n_trigger", "target": "n_code"}],
        },
        "input_data": {"x": 41},
    }

    resp = client.post("/workflow/execute", json=payload, headers=headers)

    assert resp.status_code == 200
    data = resp.json()
    assert data["success"] is True
    assert data["error"] is None
    assert data["result"]["result"] == 42
    assert isinstance(data["node_results"], list)
    assert len(data["node_results"]) == 2
    assert data["total_duration_ms"] >= 0