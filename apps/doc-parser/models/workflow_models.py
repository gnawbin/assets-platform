"""AI 工作流编排数据模型

设计参考：docs/知识库模块/AI工作流编排器设计方案.md 第 3 节（工作流 JSON 标准）

节点类型：
- trigger:   触发节点（file_upload / manual / scheduled / webhook）
- skill:     Skill 节点（复用 skill_registry，当前占位）
- llm:       LLM 节点（走 LangChain ChatOpenAI）
- condition: 条件分支节点（yes/no 两条出边）
- workflow:  内置工作流操作节点（当前占位）
- code:      代码节点（沙箱执行）
"""

from typing import Any, Literal, Optional

from pydantic import BaseModel, Field

NodeType = Literal["trigger", "skill", "llm", "condition", "workflow", "code"]


class TriggerConfig(BaseModel):
    """trigger 节点配置"""

    trigger_type: str = Field("manual", description="file_upload / manual / scheduled / webhook")
    accept: Optional[str] = Field(None, description="接受的扩展名，如 .pdf,.docx")
    max_size_mb: Optional[int] = Field(None, description="最大文件大小（MB）")


class SkillConfig(BaseModel):
    """skill 节点配置（复用 skill_registry，当前占位）"""

    params: dict[str, Any] = Field(default_factory=dict, description="Skill 参数")


class LLMConfig(BaseModel):
    """llm 节点配置"""

    prompt: str = Field(..., description="发送给 LLM 的提示词模板")
    model: Optional[str] = Field(None, description="模型名，null = 默认")
    temperature: Optional[float] = Field(None, description="温度")
    max_tokens: Optional[int] = Field(None, description="最大输出 tokens")
    output_schema: Optional[dict[str, Any]] = Field(None, description="结构化输出 schema")


class ConditionConfig(BaseModel):
    """condition 节点配置"""

    field: str = Field(..., description="要判断的字段路径")
    operator: str = Field(..., description="> / < / >= / <= / == / != / contains / is_empty")
    value: Any = Field(None, description="比较值")
    yes_label: Optional[str] = Field("yes", description="成立分支的边 label")
    no_label: Optional[str] = Field("no", description="不成立分支的边 label")


class WorkflowNodeConfig(BaseModel):
    """workflow 操作节点配置（当前占位）"""

    wf_type: str = Field("push-approval", description="操作类型")
    params: dict[str, Any] = Field(default_factory=dict, description="操作参数")


class CodeConfig(BaseModel):
    """code 节点配置"""

    language: str = Field("python", description="代码语言（当前仅支持 python 表达式）")
    code: str = Field(..., description="代码：返回值的表达式，可访问 input 变量")


class WorkflowNode(BaseModel):
    """工作流节点（按 type 判别具体配置）"""

    id: str = Field(..., description="节点唯一 ID")
    type: NodeType = Field(..., description="节点类型")
    label: str = Field("", description="节点显示名称")
    config: dict[str, Any] = Field(default_factory=dict, description="节点配置（原始 dict）")

    # ── 便捷访问（惰性解析） ────────────────────────

    def trigger_config(self) -> TriggerConfig:
        return TriggerConfig(**self.config)

    def skill_config(self) -> SkillConfig:
        return SkillConfig(**self.config)

    def llm_config(self) -> LLMConfig:
        return LLMConfig(**self.config)

    def condition_config(self) -> ConditionConfig:
        return ConditionConfig(**self.config)

    def workflow_config(self) -> WorkflowNodeConfig:
        return WorkflowNodeConfig(**self.config)

    def code_config(self) -> CodeConfig:
        return CodeConfig(**self.config)


class WorkflowEdge(BaseModel):
    """节点间连线"""

    id: str = Field("", description="边唯一 ID")
    source: str = Field(..., description="源节点 ID")
    target: str = Field(..., description="目标节点 ID")
    label: Optional[str] = Field(None, description="分支 label（condition 节点用 yes/no）")


class WorkflowConfig(BaseModel):
    """工作流全局配置"""

    max_execution_time: Optional[int] = Field(300, description="最大执行时间（秒）")
    retry_on_failure: Optional[bool] = Field(False, description="失败是否重试")
    max_retries: Optional[int] = Field(2, description="最大重试次数")


class WorkflowDefinition(BaseModel):
    """工作流定义（对应 workflow 表 definition JSONB 列）"""

    name: str = Field(..., description="工作流名称")
    description: Optional[str] = Field(None, description="描述")
    version: Optional[str] = Field("1.0.0", description="版本号")
    nodes: list[WorkflowNode] = Field(..., description="节点列表")
    edges: list[WorkflowEdge] = Field(..., description="边列表")
    variables: Optional[dict[str, Any]] = Field(None, description="变量映射")
    config: WorkflowConfig = Field(default_factory=WorkflowConfig, description="全局配置")


class WorkflowExecuteRequest(BaseModel):
    """执行工作流请求"""

    workflow: WorkflowDefinition = Field(..., description="完整工作流定义（内联传入，暂不查库）")
    input_data: dict[str, Any] = Field(default_factory=dict, description="用户输入数据")


class WorkflowNodeResult(BaseModel):
    """单个节点执行结果"""

    node_id: str = Field(..., description="节点 ID")
    status: str = Field(..., description="success / failed / skipped")
    output: Any = Field(None, description="节点输出")
    duration_ms: int = Field(0, description="耗时（毫秒）")
    error: Optional[str] = Field(None, description="错误信息（失败时）")


class WorkflowExecuteResponse(BaseModel):
    """执行工作流响应"""

    success: bool = Field(..., description="是否成功")
    result: Any = Field(None, description="最终输出")
    node_results: list[WorkflowNodeResult] = Field(
        default_factory=list, description="每个节点的执行详情"
    )
    total_duration_ms: int = Field(0, description="总耗时（毫秒）")
    error: Optional[str] = Field(None, description="整体错误信息")