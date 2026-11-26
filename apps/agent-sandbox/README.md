# agent-runner 沙箱镜像（B4'）

OpenShell 工作负载镜像：**只做 LLM 调用 + 本地工具**，编排 / 状态 / 平台工具（RAG）都在平台侧。
设计文档：`docs/知识库模块/OpenShell沙箱Agent落地设计（Rust版）.md` §9、§10.1。

```
apps/agent-sandbox/
  Dockerfile           多阶段构建（rust builder → debian-slim，非 root，/workspace）
  policies/agent.yaml  OpenShell 策略（文件系统 / 进程 / 出网白名单），进 Git 版本化
```

## 1. 构建

构建上下文是**仓库根目录**（需要完整 Cargo workspace）：

```bash
docker build -f apps/agent-sandbox/Dockerfile -t assets-platform/agent-runner:0.1.0 .
# 或
docker compose --profile build build agent-runner
```

镜像内容校验（不联网、不写文件）：

```bash
docker run --rm assets-platform/agent-runner:0.1.0 agent-runner probe
# {"protocol":"assets-agent-step/v1","version":"0.1.0"}
```

版本矩阵：`agent-runner` 镜像 tag 与 `quay.io/ai-openshell/gateway`、supervisor 镜像一起升级，不要单独漂移。

## 2. 启动 gateway（单机 Docker）

```bash
# supervisor 二进制：必须同时存在于宿主与容器内（same-path 要求）
mkdir -p ~/openshell/supervisor
docker create --name tmp-supervisor quay.io/ai-openshell/supervisor:latest
docker cp tmp-supervisor:/openshell-sandbox ~/openshell/supervisor/openshell-sandbox
docker rm tmp-supervisor && chmod +x ~/openshell/supervisor/openshell-sandbox

export OPENSHELL_SUPERVISOR_DIR="$HOME/openshell/supervisor"
docker compose up -d openshell-gateway

# 注册 + 自检（gateway 启动需要几秒）
openshell gateway add http://127.0.0.1:8080 --local --name local
openshell status      # 期望 Connected
```

生产环境走 Helm（见 §2.1），平台侧只配 URL + token + CA（`SANDBOX_OPENSHELL_*`），本目录的 compose 服务仅用于开发 / 单机。

## 2.1 Helm（K8s 生产环境）

生产**不**用 `docker.sock`、也不要求宿主与容器 same-path：supervisor 由 chart 以 DaemonSet / sidecar
形态注入，平台侧只需 gateway 地址 + token + CA。

| values | 说明 |
|---|---|
| `gateway.tls.enabled=true` | 生产必须开；平台侧 `openshell_gateway_url=https://...` + `openshell_ca_cert=/etc/openshell/ca/ca.crt` |
| `gateway.auth.token` | 用 Secret 注入（平台侧 `openshell_token`；不要把明文写进 values 仓库） |
| `gateway.replicaCount` | 建议 2+（gateway 无状态，前面挂 ClusterIP Service） |
| `supervisor.*` | chart 内的 supervisor DaemonSet：无需宿主 `docker cp`、无需 same-path |
| `agentRunner.repository/tag` | 与 `quay.io/ai-openshell/gateway`、supervisor 镜像同 tag 升级 |

```bash
# 1) gateway（示例，chart 名以实际发布为准）
helm upgrade --install openshell <chart> -n openshell --create-namespace \
  --set gateway.tls.enabled=true \
  --set gateway.replicaCount=2 \
  --set agentRunner.repository=registry.example.com/assets-platform/agent-runner \
  --set agentRunner.tag=0.1.0

# 2) 平台侧 .env.toml（[sandbox]）
# mode = "openshell"
# openshell_gateway_url = "https://openshell.openshell.svc.cluster.local:9443"
# openshell_ca_cert = "/etc/openshell/ca/ca.crt"     # 由 Secret 挂载
# openshell_workspace = "default"
# openshell_image = "registry.example.com/assets-platform/agent-runner:0.1.0"
# openshell_command = "sleep,infinity"

# 3) 全局策略（host glob 覆盖任意 namespace 下的平台 Service，见 §3）
kubectl -n openshell exec deploy/openshell-gateway -- \
  openshell policy set --global --policy /etc/openshell/policies/agent.yaml
```

与单机 compose 的差异：① 不需要 `docker.sock` 与宿主 supervisor 二进制；② 出网白名单用
`host: "**.svc.cluster.local"` + `allowed_ips`（Service CIDR），否则 ClusterIP 命中 SSRF 内网拦截
（`403 policy denied`）。

## 3. 应用策略

`policies/agent.yaml` 是**唯一**的出网 / 文件 / 进程策略来源；平台侧 `SandboxSpec.policy_yaml` 目前显式
返回 `Unsupported`（proto 的 policy 是结构化字段），所以策略只能从 gateway 侧生效：

```bash
# 方式 A：全局策略（对所有沙箱生效，推荐用于集群）
openshell policy set --global --policy apps/agent-sandbox/policies/agent.yaml

# 方式 B：手工建箱时按沙箱应用
openshell sandbox create \
  --name smoke-agent \
  --image assets-platform/agent-runner:0.1.0 \
  --policy apps/agent-sandbox/policies/agent.yaml \
  -- sleep infinity
```

策略要点：

| 段 | 值 | 说明 |
|---|---|---|
| `filesystem_policy.include_workdir` | `true` | `/workspace` 读写（step.json 进出） |
| `read_only` | `/bin` `/usr` `/lib` `/etc` `/proc` `/dev/urandom` `/var/log` | DNS / 动态库 / CA |
| `read_write` | `/tmp` `/dev/null` | 临时文件 |
| `process.run_as_user/group` | `10001` | 与镜像 `USER 10001:10001` 一致 |
| `network_policies.platform_llm` | `host: "**.svc.cluster.local"`（`rest` + `read-write`）+ `allowed_ips` | 内部 LLM 端点；`binaries` 只放行 `/usr/local/bin/agent-runner` |

`host` 支持 glob（`.` 为分隔符）：`*` 匹配单个 label，`**` 跨 label，因此 `**.svc.cluster.local`
可覆盖任意 namespace 的平台 Service，**无需按 namespace 各写一份策略**。

`allowed_ips` **必填**：host 解析到 ClusterIP（`10.x` / `172.x`）时，空列表会走默认 SSRF 内网拦截，
沙箱内 LLM 调用直接 `403 policy denied`。取值 = 平台 Service 的 CIDR：

```bash
kubectl -n <ns> get svc <assets-server-svc> -o jsonpath='{.spec.clusterIP}'   # 例 10.96.12.34
# → allowed_ips: ["10.96.0.0/12"]（收敛到 service CIDR；loopback / link-local 恒被拒）
```

`sandbox_exec` 起的 `sh` / `curl` / `python` **不在** binaries 白名单，默认拒绝
（这是预期行为，用于验证策略确实生效）。

## 4. 平台侧配置

`.env.toml` 的 `[sandbox]` 段（扁平标量，映射为 `SANDBOX_*`）：

```toml
mode = "openshell"
openshell_gateway_url = "http://127.0.0.1:8080"
openshell_workspace = "default"
openshell_image = "assets-platform/agent-runner:0.1.0"
# 主进程保持存活；step 由平台 exec 调起，不要写 "agent-runner,serve"（serve 尚未实现）
openshell_command = "sleep,infinity"
openshell_ready_timeout_secs = 120

agent_in_sandbox = true
agent_llm_base_url = "http://assets-server:3001/internal/llm/v1"
agent_token_env = "AGENT_TOKEN"
# 租户级 API Key（scope 必须含 sandbox:llm）；留空则沙箱内 LLM 调用 401
agent_token = "<api-key>"
agent_step_timeout_secs = 120
agent_max_steps = 6
agent_exec_timeout_secs = 60
# 逐步写 agent_step 审计（审批申请 / 结论不受该开关影响）
agent_audit_steps = true
```

token 注入路径：`SandboxAgentRunner` 启动时读 `SANDBOX_AGENT_TOKEN` → `ServiceSandboxIo::prepare`
登记到 `sandbox_service` 的会话环境 → 建箱时写进 `SandboxSpec.env` → 沙箱内 `agent-runner` 从
`step.llm.token_env`（`AGENT_TOKEN`）读取并发给内部 LLM 端点。

> 已存在的沙箱不会重建：改 token 后要么等 TTL 回收，要么 `DELETE /api/sandbox/sessions/...` 后重建。

### 4.1 步数 / 审计语义（B6'）

- 有效步数上限 = `min(节点 config.max_iterations, agent_max_steps)`（`max_iterations` 先 clamp 到 1..=30）；
- 达到上限 → `Completed {"answer":"Agent 达到最大步数 N，已停止。","steps":N,"limit":N}`，
  **不再 `Failed`**，避免平台按服务任务失败重试、反复建箱（v2.1 行为变更）；
- 审批恢复时 `step` 被 clamp 到 `0..=limit`，收紧配置后不会越界；
- token 用量：每步 `StepOutput.usage` 累加进最终 `state.tokens = {input, output}`；
- 审计（`sys_oper_log`，`oper_module=sandbox`）：`agent_step`（逐步，可关）、`agent_approval`
  （`requested` 由平台在中断落库后写、`approved`/`rejected` 在人工审批完成时写）、`agent_limit`。
  只记 tool 名 / kind / 状态 / 耗时 / token / 审批意见（截断 200 字），**不写** arguments、提示词、LLM 原文。

## 5. 验收（B4'）

1. `docker run --rm assets-platform/agent-runner:0.1.0 agent-runner probe` 返回协议 JSON；
2. `openshell status` = Connected，`openshell sandbox create --image assets-platform/agent-runner:0.1.0 ...` 成功；
3. 平台 `mode=openshell` + `agent_in_sandbox=true`，跑一个带 Agent 节点的工作流：
   `/workspace/in/step.json` 写入 → `agent-runner step` 冷启动 → `llm_call_record` 出现 `call_type='sandbox_chat'` 行；
4. `sys_oper_log`（或 OCSF 日志）里能按 sandbox 名称对上 `session_open` / `process_start`；
5. 达到 `agent_max_steps`：执行记录为 `success`，输出含「Agent 达到最大步数」，且 `sys_oper_log` 有 `agent_limit`；
6. 审批流：申请时一行 `agent_approval`（`status=requested`），审批后一行（`status=approved` / `rejected`），
   两行 `client_info` 带同一个 `scope=wf:<execution_id>`，且 `oper_content` 中**没有** tool arguments。

> 本节的 1–6 需在 Linux + Docker/K8s 环境执行，开发机无法闭环；进度跟踪：根目录 `TODO.md` #51（端到端）、
> #52（`allowed_ips` 收窄）、#53（发布说明 / 告警口径）。

## 6. 排障

| 现象 | 原因 | 处理 |
|---|---|---|
| 沙箱建不起来：`no such image` | 镜像只在开发机 build，gateway 所在节点看不到 | 推到 registry 并让 Pod 能拉取（`imagePullSecret`）；compose 下须在**宿主** docker 构建（gateway 通过 socket 建箱） |
| `open /…/openshell-sandbox: no such file` | supervisor 二进制不在宿主，或两侧路径不一致 | 仅单机 compose 需要：重跑 §2 的 `docker cp`，两侧统一绝对路径；K8s 用 §2.1 chart 内的 supervisor |
| `permission denied /var/run/docker.sock` | gateway 容器无 docker 组 | 仅单机 compose：Linux 加 `group_add: docker`；Docker Desktop 改成 socket 的 GID。K8s 不需要 socket |
| 沙箱内 LLM 调用 401 | `agent_token` 为空 / 无 `sandbox:llm` scope / 沙箱在改配置前就建好 | 配 token 并重建会话 |
| 沙箱内 LLM 调用 `403 policy denied`（host 不匹配） | 策略 `host` glob 与实际 Service 域名不符，或请求不是 agent-runner 发出 | K8s 用 `**.svc.cluster.local`；compose 用服务名；改完 `openshell policy set` 再重试 |
| 沙箱内 LLM 调用 `403 policy denied`（SSRF 内网拦截） | `allowed_ips` 为空，而 host 解析到 ClusterIP / bridge 内网 IP | 在 endpoint 上加 `allowed_ips` = service CIDR（K8s `10.96.0.0/12` 之类，compose `172.16.0.0/12`），实测确认网段后收窄 |
| `agent-runner: command not found` | 平台 `openshell_command` 写成 `agent-runner,serve` 之类未实现形态 | 用 `sleep,infinity` 作为主进程，step 由 exec 调起 |
| 平台日志 `SANDBOX_AGENT_TOKEN 未配置` | 只配了 `agent_in_sandbox` | 补 `agent_token`（否则沙箱内 LLM 必 401） |
| 平台日志 `沙箱 Agent 审计未写入` | 写库连接不可用 / `sys_oper_log` 表缺失 | 检查 `database` 写池与 `knowledge_module_migration.sql`；审计失败不影响 Agent 执行 |

## 7. 已知限制

- `agent-runner serve`（常驻 / 流式）**未实现**：当前主进程是 `sleep infinity`，一次 exec = 一步；
- `SandboxSpec.policy_yaml` 仍返回 `Unsupported`，策略只从 gateway 侧生效；
- 镜像不含 python：`sandbox_exec` 里跑 Python 需要换底镜像或另行安装（与 B-Ⅰ 工具语义解耦）；
- 单机 compose 关闭 TLS 仅适用于 `127.0.0.1`；跨机必须启用 mTLS 或 Helm。
