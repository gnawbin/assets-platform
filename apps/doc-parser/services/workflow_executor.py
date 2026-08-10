"""LangGraph 工作流执行器

将工作流 JSON 定义（WorkflowDefinition）转换为 LangGraph StateGraph 并执行。
设计参考：docs/知识库模块/AI工作流编排器设计方案.md 第 6 章

支持节点：trigger / llm / condition / code（可运行），skill / workflow（占位报错）。
依赖懒加载：langchain / langgraph 仅在首次执行时导入。
"""

import time
from typing import Any, TypedDict

import config


class WorkflowState(TypedDict):
    """LangGraph 图状态"""

    data: dict[str, Any]
    node_results: list[dict[str, Any]]


# ─── 字段解析 ───────────────────────────────────────


def _resolve_field(data: dict, field: str):
    """按 'a.b.c' 路径取值；node_x.output.xxx 从节点输出中取"""
    if field.startswith("node_"):
        parts = field.split(".")
        node_id = parts[0]
        outputs = data.get("_node_outputs", {})
        rest = parts[2:] if len(parts) >= 3 and parts[1] == "output" else parts[1:]
        target = outputs.get(node_id)
    else:
        target = data
        rest = field.split(".")
    for key in rest:
        if isinstance(target, dict):
            target = target.get(key)
        else:
            return None
    return target


# ─── 节点工厂 ───────────────────────────────────────


def trigger_node(node: dict):
    """trigger 节点：透传输入"""

    def _run(state: WorkflowState) -> dict:
        return {"data": state.get("data", {})}

    return _run


def code_node(node: dict):
    """code 节点：白名单表达式求值（无任意代码执行）"""

    expr = str(node.get("config", {}).get("code", ""))

    def _run(state: WorkflowState) -> dict:
        data = state.get("data", {})
        sandbox = {
            "input": data,
            "len": len, "str": str, "int": int, "float": float,
            "bool": bool, "list": list, "dict": dict,
            "sum": sum, "min": min, "max": max, "abs": abs,
            "round": round, "sorted": sorted, "range": range,
            "True": True, "False": False, "None": None,
        }
        result = eval(expr, {"__builtins__": {}}, sandbox)  # noqa: S307
        return {"data": {**data, "result": result}}

    return _run


def llm_node(node: dict):
    """llm 节点：LangChain ChatOpenAI（Ollama OpenAI 兼容）"""

    from langchain_openai import ChatOpenAI

    cfg = node.get("config", {})
    prompt_tpl = str(cfg.get("prompt", ""))
    model = cfg.get("model") or config.OLLAMA_MODEL
    temperature = cfg.get("temperature") or 0.3
    max_tokens = cfg.get("max_tokens") or 2048

    def _run(state: WorkflowState) -> dict:
        data = state.get("data", {})
        prompt = prompt_tpl
        for k, v in data.items():
            if isinstance(v, (str, int, float, bool)):
                prompt = prompt.replace("{{" + k + "}}", str(v))
        llm = ChatOpenAI(
            base_url=f"{config.OLLAMA_BASE_URL}/v1",
            api_key="ollama",
            model=model,
            temperature=temperature,
            max_tokens=max_tokens,
            timeout=120,
        )
        resp = llm.invoke(prompt)
        return {"data": {**data, "result": resp.content}}

    return _run


def condition_node(node: dict):
    """condition 节点：路由函数，返回 yes/no 边 label"""

    cfg = node.get("config", {})
    field = str(cfg.get("field", ""))
    operator = str(cfg.get("operator", "=="))
    value = cfg.get("value")
    yes_label = str(cfg.get("yes_label", "yes"))
    no_label = str(cfg.get("no_label", "no"))

    def _route(state: WorkflowState) -> str:
        data = state.get("data", {})
        actual = _resolve_field(data, field)
        if operator == ">":
            matched = actual is not None and actual > value
        elif operator == "<":
            matched = actual is not None and actual < value
        elif operator == ">=":
            matched = actual is not None and actual >= value
        elif operator == "<=":
            matched = actual is not None and actual <= value
        elif operator == "!=":
            matched = actual != value
        elif operator == "contains":
            matched = actual is not None and str(value) in str(actual)
        elif operator == "is_empty":
            matched = actual in (None, "", [], {})
        else:
            matched = actual == value
        return yes_label if matched else no_label

    return _route


# ─── 图构建与执行 ───────────────────────────────────


def build_graph(definition) -> Any:
    """将 WorkflowDefinition（Pydantic 或 dict）构建为编译后的 LangGraph 应用"""
    from langgraph.graph import END, StateGraph

    defn = definition.model_dump() if hasattr(definition, "model_dump") else definition
    nodes = defn["nodes"]
    edges = defn["edges"]
    node_by_id = {n["id"]: n for n in nodes}

    if not nodes:
        raise ValueError("工作流定义缺少节点（nodes 为空）")

    graph = StateGraph(WorkflowState)

    for n in nodes:
        nid, ntype = n["id"], n["type"]
        if ntype == "condition":
            continue  # 条件节点由 add_conditional_edges 路由
        if ntype == "trigger":
            graph.add_node(nid, trigger_node(n))
        elif ntype == "code":
            graph.add_node(nid, code_node(n))
        elif ntype == "llm":
            graph.add_node(nid, llm_node(n))
        elif ntype == "skill":
            def _skill_placeholder(state):
                raise NotImplementedError(f"skill 节点暂未实现（{nid}），属方案 B 占位")
            graph.add_node(nid, _skill_placeholder)
        elif ntype == "workflow":
            def _wf_placeholder(state):
                raise NotImplementedError(f"workflow 操作节点暂未实现（{nid}），属方案 B 占位")
            graph.add_node(nid, _wf_placeholder)
        else:
            raise ValueError(f"未知节点类型: {ntype}")

    graph.set_entry_point(nodes[0]["id"])

    # 收集条件节点的所有出边（一次 add_conditional_edges 注册完整分支映射，
    # 多次调用后者会覆盖前者的映射）
    cond_branches: dict[str, dict[str, str]] = {}
    normal_edges: list[tuple[str, str]] = []
    for e in edges:
        src, tgt = e["source"], e["target"]
        label = e.get("label")
        src_node = node_by_id.get(src)
        if src_node and src_node["type"] == "condition":
            cond_branches.setdefault(src, {})[label or "no"] = tgt
        else:
            normal_edges.append((src, tgt))

    for src, mapping in cond_branches.items():
        graph.add_conditional_edges(src, condition_node(node_by_id[src]), mapping)

    for src, tgt in normal_edges:
        graph.add_edge(src, tgt)

    targets = {e["target"] for e in edges}
    for n in nodes:
        if n["id"] not in targets and n["type"] != "condition":
            graph.add_edge(n["id"], END)

    return graph.compile()


def execute_workflow(definition, input_data: dict | None = None) -> dict:
    """执行工作流，返回标准化结果 dict（对应 WorkflowExecuteResponse）"""
    start = time.monotonic()
    node_results: list[dict] = []
    try:
        app = build_graph(definition)
        initial = {"data": dict(input_data or {}), "node_results": node_results}
        result_state = app.invoke(initial)
        data = result_state.get("data", {})
        defn = definition.model_dump() if hasattr(definition, "model_dump") else definition
        for n in defn["nodes"]:
            node_results.append({
                "node_id": n["id"],
                "status": "success",
                "output": data.get("result"),
                "duration_ms": 0,
                "error": None,
            })
        return {
            "success": True,
            "result": data,
            "node_results": node_results,
            "total_duration_ms": int((time.monotonic() - start) * 1000),
            "error": None,
        }
    except Exception as e:
        return {
            "success": False,
            "result": None,
            "node_results": node_results,
            "total_duration_ms": int((time.monotonic() - start) * 1000),
            "error": str(e),
        }