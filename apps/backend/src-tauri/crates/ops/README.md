# ops —— 运维只读采集连接层

`ops` 是「IT 资产与运维排查一体化系统」中**采集发现引擎**的连接层：把 Docker / Swarm /
Kubernetes 的资源归一化成 `ResourceRecord`，交给后续的对账器、调度器、操作器。

本 crate **只读**：

- Docker 只调 `/_ping`、`/version`、`/info`、`list_containers`、`list_images`、
  `list_networks`、`list_volumes`、`list_nodes`、`list_services`、`list_tasks`；
- Kubernetes 只调 `ApiserverVersion` 与 `get` / `list`；
- 不 exec、不起停、不删除、不改配置；
- 不直连 K8s 节点的 `docker.sock`；
- `ConfigMap` / `Secret` 只保留键名与类型，**绝不读取 Data 内容**。

## 特性开关

默认全关，避免 bollard / kube 拖慢 Tauri 主构建：

| feature  | 内容 | 依赖 |
| -------- | ---- | ---- |
| `docker` | 单机 Docker + Swarm（Node / Service / Task） | `bollard`、`time` |
| `k8s`    | Kubernetes（APIServer 只读） | `kube`、`k8s-openapi`、`base64` |

```bash
cargo check -p ops --features docker,k8s
cargo test  -p ops --features docker,k8s
```

> `k8s-openapi` 目前按 `v1_32` 编译。若目标集群低于 1.32，请同步调整根
> `Cargo.toml` 中 `k8s-openapi` 的 feature。

## 快速上手

```rust
use ops::connector::docker::DockerConnector;
use ops::connector::Connector;

#[tokio::main]
async fn main() -> ops::OpsResult<()> {
    // 1. 建连接（bollard 懒连接，这一步不发请求）
    let docker = DockerConnector::local_default()?;

    // 2. 探测连通性 + 只读权限（返回中文结论，可直接进界面 toast）
    let health = docker.health().await?;
    println!("{}", health.summary());

    // 3. 采集一次快照
    let snapshot = docker.collect().await?;
    println!(
        "{} 条资源，完整={}，部分失败={}",
        snapshot.resources.len(),
        snapshot.is_complete(),
        snapshot.partial_errors.len()
    );
    Ok(())
}
```

多个连接放进注册表统一调度（`with_defaults()` 已按 feature 注册本机 Docker，所以这里用 `new()`
避免同名重复注册）：

```rust
use std::sync::Arc;

use ops::connector::docker::DockerConnector;
use ops::connector::{Connector, ConnectorRegistry};

#[tokio::main]
async fn main() -> ops::OpsResult<()> {
    let mut registry = ConnectorRegistry::new();
    registry.register(Arc::new(DockerConnector::local_default()?))?;

    for key in registry.keys() {
        println!("已注册：{key}");
    }
    // 顺序探测，避免同时打爆远端 / 触发风控
    for health in registry.health_all().await {
        println!("{} {}", health.ok, health.summary());
    }
    Ok(())
}
```

`ConnectionConfig` 提供一组构造函数，端点会按 provider / 认证方式自动补 scheme：

| 场景 | 构造函数 | 说明 |
| ---- | -------- | ---- |
| 本机 Docker | `ConnectionConfig::local_docker(name)` | Linux socket / Windows npipe |
| 远程 Docker（明文） | `docker_tcp(name, "tcp://10.0.0.5:2375")` | 仅限隔离内网，界面需提示风险 |
| 远程 Docker（mTLS） | `docker_tls(name, "https://10.0.0.5:2376", ca, cert, key)` | 证书路径 |
| 本机 Swarm | `local_swarm(name)` | 同一 socket，额外采集集群对象 |
| 远程 Swarm | `swarm_tcp` / `swarm_tls` | 必须是 Manager 节点 |
| 本机 kubeconfig | `kubeconfig(name, path, context)` | `context` 为 `None` 时用 `current-context` |
| 直连 APIServer | `kubernetes_token(name, endpoint, token, ca_pem, insecure)` | 内部会合成 kubeconfig |

`ConnectionTier`（`Local` / `Intranet` / `Remote`）由端点推断，用于界面风险提示；
`ConnectionHealth.failure` 给出 `FailureClass`，把错误分成
`none / dns / network / timeout / tls / auth / forbidden / version_mismatch / api / config` 十类，
每类都有稳定编码 `code()`（落库 / 前端判定用）、中文 `label()` 和排查建议 `hint()`。

## 安全约定

- 凭证（Token、客户端私钥、kubeconfig 原文）只在 crate 内使用；
  `AuthKind` 手写了 `Debug`，敏感字段输出 `<已隐藏>`，不会误入日志。
- `ConnectionHealth` / `ResourceSnapshot` 里不出现凭证字段。
- 采集失败按「类」记录到 `partial_errors`，不会把整次采集打成空快照
  （避免对账器把在线资源误判为离线）。

## ops_probe 探针

`examples/ops_probe.rs` 是一个只读连通性探针，零参数即可跑本机 Docker：

```bash
# 本机 Docker
cargo run -p ops --example ops_probe --features docker

# 远程 Swarm（mTLS）+ 顺带采集
OPS_PROVIDER=swarm \
OPS_ENDPOINT=https://10.0.0.5:2376 \
OPS_TLS_CA=./ca.pem OPS_TLS_CERT=./cert.pem OPS_TLS_KEY=./key.pem \
cargo run -p ops --example ops_probe --features docker -- --collect

# Kubernetes（本机 kubeconfig）
OPS_PROVIDER=k8s OPS_KUBECONFIG=$HOME/.kube/config OPS_KUBE_CONTEXT=prod \
cargo run -p ops --example ops_probe --features k8s
```

退出码：`0` 通过；`1` 配置错误或探测失败，可直接用于 CI / 巡检脚本。

### 环境变量

| 变量 | 适用 | 说明 |
| ---- | ---- | ---- |
| `OPS_PROVIDER` | 全部 | `docker`（默认）/ `swarm` / `k8s` |
| `OPS_NAME` | 全部 | 连接名称，默认 `probe-<provider>` |
| `OPS_ENDPOINT` | 远程 | `tcp://host:2375`、`https://host:2376`、`https://apiserver:6443` |
| `OPS_AUTH` | 全部 | Docker：`local` / `tcp` / `tls`；K8s：`kubeconfig` / `inline` / `token`；留空自动推断 |
| `OPS_TLS_CA` / `OPS_TLS_CERT` / `OPS_TLS_KEY` | Docker mTLS | 证书文件路径 |
| `OPS_KUBECONFIG` | K8s | kubeconfig 路径（`OPS_AUTH=inline` 时读入内存后内联） |
| `OPS_KUBE_CONTEXT` | K8s | 指定 context，留空用 `current-context` |
| `OPS_TOKEN` | K8s 直连 | ServiceAccount Token |
| `OPS_CA_PEM` / `OPS_CA_FILE` | K8s 直连 | CA PEM 原文 / CA 文件路径 |
| `OPS_INSECURE_SKIP_TLS_VERIFY` | K8s 直连 | `1` 跳过证书校验（自签集群录入期临时用） |
| `OPS_NAMESPACE` | K8s | 只采集该命名空间；留空采集全集群 |
| `OPS_TIMEOUT_SECS` | 全部 | 单次请求超时，默认 15（`ConnectionConfig` 夹到最小 1 秒） |
| `OPS_MAX_ITEMS` | 全部 | 单类资源采集上限，默认 20000（`>=1`） |
| `OPS_DOCKER_API_VERSION` | Docker | 例如 `1.43`，留空用 bollard 默认 |
| `OPS_COLLECT` | 全部 | `1` 等价于 `--collect` |
| `RUST_LOG` | 全部 | 日志过滤，例如 `ops=debug` |

## 测试

全部用例都可离线跑：Docker 侧用 `127.0.0.1:0` 起的假 daemon（HTTP/1.1，按路径后缀路由），
K8s 侧校验合成的 kubeconfig 可被 `kube::config::Kubeconfig` 解析、错误分类与状态映射。

```bash
cargo test -p ops                       # 27 个用例（model / error / 注册表基线）
cargo test -p ops --features docker     # 33 个用例
cargo test -p ops --features k8s        # 34 个用例
cargo test -p ops --features docker,k8s # 40 个用例（feature 叠加）
```

## 后续迭代（不属于本 crate）

- 对账器、调度器、操作器（起停 / 变更）在后续 crate 中实现；
- watch 增量订阅本轮全部关闭（`Connector::supports_watch()` 恒为 `false`）。
