# BPMN 工作流编辑器迁移方案

> 状态：已实施（P0 / P1 / P2）
> 范围：`apps/web` 工作流编排器从 `@xyflow/react` 迁移到 `bpmn-js`
> 目标：可视化编辑标准 BPMN 2.0，同时保留后端执行所需的 `WorkflowDefinition` IR

## 1. 背景与目标

原编辑器基于 xyflow 自定义节点渲染，导出的 JSON 只包含私有 `nodes/edges`，
无法与 bpmn.io、Camunda Modeler 等标准工具互通，也不利于后续接入 BPMN 引擎。

迁移后：

- 画布、文件格式、CMMN 交互全部使用 **标准 BPMN 2.0**；
- 后端执行契约不变，仍为 `WorkflowDefinition.nodes/edges`；
- 执行参数通过 BPMN 规范的 `bpmn:extensionElements` 承载，不新增私有节点类型；
- 支持 `.bpmn` / `.json` 导入导出，导出文件可在 bpmn.io 直接打开。

## 2. 依赖变更

| 依赖 | 变更 |
|------|------|
| `@xyflow/react` | 移除 |
| `bpmn-js` | 新增（18.31.1） |
| `bpmn-moddle` | 新增（10.3.1，ESM-only） |

bpmn-js 访问 `window/document/SVG`，因此画布组件必须 `dynamic(..., { ssr: false })`。
`bpmn-moddle@10` 为 ESM-only，单元测试只覆盖纯映射函数，避免把 bpmn-js 拉进
Jest 环境。测试转换使用 Next.js 内置的 `next/jest`（SWC），不依赖 TypeScript 的
JS Compiler API。

## 3. 目录结构

```
apps/web/src/components/workflow/
├─ WorkflowEditor.tsx              # 外壳：加载 / 保存 / 执行 / 导入导出
├─ NodePalettePanel.tsx            # 业务模板 + Skill 库（拖拽生成标准元素）
├─ NodeConfigPanel.tsx             # 标准 BPMN 属性 + 执行参数
└─ bpmn/
   ├─ BpmnCanvas.tsx               # bpmn-js 实例生命周期、拖放、事件桥接
   ├─ EngineFieldsPanel.tsx        # trigger/skill/llm/condition/code/workflow 参数
   ├─ types.ts                     # SelectedBpmnElement / ElementPatch / BpmnCanvasApi
   ├─ bpmn.css                     # 画布与语义着色
   ├─ mapping/
   │  ├─ nodeKinds.ts              # 业务模板 ↔ BPMN 类型
   │  ├─ helpers.ts                # 纯函数：条件表达式、校验、IR 排序
   │  ├─ bpmnToDefinition.ts       # BPMN → IR
   │  └─ definitionToBpmn.ts       # IR → BPMN（旧数据兼容）
   ├─ modeler/
   │  ├─ createModeler.ts          # Modeler + 自定义模块
   │  ├─ modelerApi.ts             # 命令式 API（撤销/缩放/属性更新/投影）
   │  ├─ createElements.ts         # 按模板创建标准 BPMN 业务对象
   │  ├─ AssetsPaletteProvider.ts  # 左侧“业务模板”调色板分组
   │  └─ AssetsStylingModule.ts    # 按执行语义着色（marker）
   ├─ moddle/assetsModdle.ts       # assets:EngineConfig 扩展描述
   └─ template/blankDiagram.ts     # 空白 BPMN 文档
```

## 4. IR ↔ BPMN 映射

| IR 节点 | BPMN 元素 | 执行参数的落点 |
|---------|-----------|----------------|
| `trigger` | `bpmn:StartEvent` | `triggerType/accept/maxSizeMb`；`scheduled→timer`、`webhook→message` |
| `skill` | `bpmn:ServiceTask` | `implementation="##Skill"` + `assets:skillId/skillName/params` |
| `llm` | `bpmn:ServiceTask` | `implementation="##LLM"` + `assets:prompt/model/temperature/maxTokens/outputSchema` |
| `condition` | `bpmn:ExclusiveGateway` | 出线 `name="yes"/"no"`；yes 带 `conditionExpression`，no 为 `default` |
| `code` | `bpmn:ScriptTask` | `scriptFormat` + `script` |
| `workflow` | `bpmn:CallActivity` | `calledElement` + `assets:wfType` |
| `output` | `bpmn:EndEvent` | 仅保留在 XML，不投影为 IR |

节点 `id` 直接复用 BPMN 元素 id，保证执行结果（`node_results[].node_id`）与画布
元素一一对应。

### assets 扩展

命名空间：`https://assets-platform.local/schema/bpmn/assets`
类型：`assets:EngineConfig`（序列化为 `<assets:engineConfig ... />`，全部为属性）

```xml
<bpmn:extensionElements>
  <assets:engineConfig kind="skill" skillId="doc-parse" />
</bpmn:extensionElements>
```

`assets` 只承载执行参数；节点语义完全由标准 BPMN 元素类型表达。

## 5. 数据流

### 5.1 加载

1. `getWorkflow(id)` 返回 `WorkflowDefinition`；
2. 若有 `bpmn_xml` → 直接导入；
3. 否则用 `definitionToBpmn` 生成标准 XML（旧数据兼容）；
4. 新建流程使用 `blankDiagram.ts`。

### 5.2 保存

1. `api.saveXml()` 得到标准 BPMN XML；
2. `api.buildDefinition()` 投影出 IR（同时收集校验问题）；
3. `saveWorkflow` 双写 `definition`（含 `bpmn_xml`）、`bpmnXml`、`node_types`。

### 5.3 执行

1. 先投影并校验，`error` 级别阻断执行；
2. 有未保存修改时先静默保存；
3. `executeWorkflow({ workflowId, definition, inputData, title })`；
4. 若后端返回 `node_results`，在画布上按 id 着色（成功/失败/运行中）。

## 6. 校验规则

| 级别 | 规则 |
|------|------|
| error | 缺少 `bpmn:Process` / 缺少开始事件 / 条件网关少于两条出线 / Skill 未选 / LLM 无 Prompt / 子流程无类型 |
| warning | 多个开始事件 / 无结束事件 / 无可执行语义的标准元素 / 条件出线缺少 `yes` 标签 / 后端占位执行器（skill、workflow） |

不可执行的 BPMN 元素（UserTask、ParallelGateway 等）会保留在 XML 中，但在投影
时被排除并给出 warning。

## 7. UI 行为

- 左侧 `NodePalettePanel`：业务模板 + Skill 库，可拖拽或点击插入；
- bpmn-js 原生调色板、键盘快捷键、撤销/重做、缩放、fit-viewport 全部保留；
- 右侧属性面板：标准属性（name/documentation/元素类型/事件定义/implementation/
  calledElement/脚本）+ 执行参数；
- 工具栏：保存、执行、撤销/重做、缩放、适应画布、导出 `.bpmn`/`.json`、导入。

## 8. 已知限制

1. 后端 `workflow_executor.py` 中 `skill`、`workflow` 节点仍是占位实现；
2. 后端没有 `output` 节点类型，`EndEvent` 只作为画布终点与上游节点的终止标记；
3. Pool/Lane、并行/包容网关可编辑与导出，但执行投影只支持单一可执行 Process 与
   ExclusiveGateway；
4. 单元测试使用 `next/jest` + SWC 转换（不依赖 TypeScript JS Compiler API），类型检查由
   `tsc --noEmit` 负责；上传模块的旧测试仍使用单阶段 mock，与当前两阶段上传实现不一致，
   待单独修复。

## 9. 验证清单

- [x] `pnpm --filter @assets/web exec tsc --noEmit`
- [x] `pnpm --filter @assets/web build`
- [x] `pnpm --filter @assets/web test -- --runInBand`：BPMN 映射 16 个用例通过
- [x] 无头 Chrome 加载 mock 工作流：画布渲染 11 个 shape / 5 条连线，选中节点后右侧显示标准属性与执行参数
- [x] 导出 `.bpmn` 并检查 XML：`assets:engineConfig`、`implementation="##Skill"`、`name="yes"` + `conditionExpression`、`name="no"` default flow
- [ ] 在 bpmn.io / Camunda Modeler 在线打开导出的 `.bpmn`
- [ ] 真实后端环境下保存 → 刷新 → 执行全链路
