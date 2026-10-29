//! 运维连接连通性探针（只读诊断工具）
//!
//! 全部行为都只发只读请求：Docker 走 `/_ping`、`/version`、`list_*`；
//! Kubernetes 走 `get`/`list`。探针不会 exec、不会起停、不会改任何远端状态。
//!
//! ```bash
//! # 本机 Docker
//! cargo run -p ops --example ops_probe --features docker
//!
//! # 远程 Swarm（mTLS）
//! OPS_PROVIDER=swarm OPS_ENDPOINT=https://10.0.0.5:2376 \
//! OPS_TLS_CA=./ca.pem OPS_TLS_CERT=./cert.pem OPS_TLS_KEY=./key.pem \
//! cargo run -p ops --example ops_probe --features docker -- --collect
//!
//! # Kubernetes（本机 kubeconfig）
//! OPS_PROVIDER=k8s OPS_KUBECONFIG=~/.kube/config OPS_KUBE_CONTEXT=prod \
//! cargo run -p ops --example ops_probe --features k8s
//! ```
//!
//! 退出码：`0` 探测通过；`1` 配置错误或探测失败（便于接入 CI / 巡检脚本）。

use std::collections::BTreeMap;
use std::sync::Arc;

use ops::connector::{AuthKind, ConnectionConfig, ConnectionHealth, Connector};
use ops::model::Provider;
use tracing_subscriber::EnvFilter;

/// 采样预览条数（避免刷屏）
const PREVIEW_LIMIT: usize = 10;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("ops=info")),
        )
        .with_target(false)
        .init();

    if let Err(message) = run().await {
        eprintln!("✗ {message}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let config = build_config()?;
    config
        .validate()
        .map_err(|err| format!("配置校验失败：{err}（{}）", err.classify().hint()))?;
    print_plan(&config);

    let connector = build_connector(config).await?;
    let health = connector
        .health()
        .await
        .map_err(|err| format!("探测失败：{err}（{}）", err.classify().hint()))?;

    print_health(&health);
    if !health.ok {
        return Err(format!(
            "连通性探测未通过（{}，耗时 {} ms）",
            health.failure.code(),
            health.latency_ms
        ));
    }

    if wants_collect() {
        collect_and_print(connector.as_ref()).await?;
    } else {
        println!("\n（加 `--collect` 或 `OPS_COLLECT=1` 可顺带跑一次全量只读采集）");
    }
    Ok(())
}

/// 打印将要使用的连接方案（凭证只打印类型，不打印内容）
fn print_plan(config: &ConnectionConfig) {
    println!("== ops_probe ==");
    println!("connection : {}", config.name);
    println!(
        "provider   : {}（{}）",
        config.provider.as_str(),
        config.provider.label()
    );
    println!(
        "endpoint   : {}",
        if config.endpoint.is_empty() {
            "<由 kubeconfig 决定>"
        } else {
            config.endpoint.as_str()
        }
    );
    println!("auth       : {}", config.auth.describe());
    println!(
        "tier       : {:?}   超时 {}s   单类上限 {}",
        config.tier(),
        config.timeout_secs(),
        config.max_items
    );
}

/// 打印探测结论
fn print_health(health: &ConnectionHealth) {
    println!("\n-- health --");
    println!("{}", health.summary());
    println!("latency    : {} ms", health.latency_ms);
    if let Some(version) = health.server_version.as_deref() {
        println!("server     : {version}");
    }
    if let Some(api) = health.api_version.as_deref() {
        println!("api        : {api}");
    }
    println!(
        "authn/authz: {} / {}",
        if health.authenticated { "ok" } else { "fail" },
        if health.authorized { "ok" } else { "fail" }
    );
    for warning in &health.warnings {
        println!("warn       : {warning}");
    }
    if let Some(error) = health.error.as_deref() {
        println!("error      : {error}");
        println!(
            "failure    : {}（{}）→ {}",
            health.failure.code(),
            health.failure.label(),
            health.failure.hint()
        );
    }
}

/// 跑一次全量采集并打印统计（只展示聚合结果与少量样本）
async fn collect_and_print(connector: &dyn Connector) -> Result<(), String> {
    println!("\n-- collect --");
    let snapshot = connector
        .collect()
        .await
        .map_err(|err| format!("采集失败：{err}（{}）", err.classify().hint()))?;

    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for record in &snapshot.resources {
        *counts.entry(record.key.kind.as_str()).or_default() += 1;
    }

    println!(
        "resources  : {} 条，耗时 {} ms",
        snapshot.resources.len(),
        snapshot.elapsed_ms
    );
    println!(
        "complete   : {}",
        if snapshot.is_complete() {
            "是"
        } else {
            "否（见下方 partial）"
        }
    );
    for (kind, count) in counts {
        println!("  {kind:<22} {count}");
    }
    for error in &snapshot.partial_errors {
        println!("  partial: {error}");
    }

    if !snapshot.resources.is_empty() {
        println!("  预览（前 {PREVIEW_LIMIT} 条）：");
        for record in snapshot.resources.iter().take(PREVIEW_LIMIT) {
            println!(
                "    · {:<22} {:<28} {}",
                record.key.kind.as_str(),
                record.display_name,
                record.status.label()
            );
        }
    }
    Ok(())
}

/// 是否附带采集
fn wants_collect() -> bool {
    std::env::args().any(|arg| arg == "--collect") || env_flag("OPS_COLLECT")
}

/// 读取环境变量（空字符串视为未设置）
fn env_opt(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 读取布尔环境变量
fn env_flag(name: &str) -> bool {
    matches!(
        env_opt(name).as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "on")
    )
}

/// 读取数值环境变量
fn env_u64(name: &str) -> Option<u64> {
    env_opt(name).map(|value| value.parse::<u64>().unwrap_or(0))
}

/// 读取 `usize` 型上限环境变量（非法值视为未设置）
fn env_usize(name: &str) -> Option<usize> {
    env_opt(name).and_then(|value| value.parse::<usize>().ok())
}

/// 从环境变量组装连接配置（凭证只从环境读取，不落命令行历史）
fn build_config() -> Result<ConnectionConfig, String> {
    let provider = match env_opt("OPS_PROVIDER").as_deref() {
        None | Some("docker") => Provider::Docker,
        Some("swarm") => Provider::Swarm,
        Some("k8s" | "kubernetes") => Provider::Kubernetes,
        Some(other) => {
            return Err(format!(
                "未知 OPS_PROVIDER：{other}（可选 docker / swarm / k8s）"
            ));
        }
    };
    let name = env_opt("OPS_NAME").unwrap_or_else(|| format!("probe-{}", provider.as_str()));
    let endpoint = env_opt("OPS_ENDPOINT").unwrap_or_default();
    let auth = env_opt("OPS_AUTH").unwrap_or_default().to_ascii_lowercase();

    let mut config = match provider {
        Provider::Docker | Provider::Swarm => docker_config(provider, name, &endpoint, &auth)?,
        Provider::Kubernetes => k8s_config(name, &endpoint, &auth)?,
    };

    if let Some(secs) = env_u64("OPS_TIMEOUT_SECS") {
        config = config.with_timeout_secs(secs);
    }
    if let Some(limit) = env_usize("OPS_MAX_ITEMS") {
        config = config.with_max_items(limit);
    }
    if let Some(version) = env_opt("OPS_DOCKER_API_VERSION") {
        config = config.with_docker_api_version(version);
    }
    if let Some(namespace) = env_opt("OPS_NAMESPACE") {
        config = config.with_namespace(namespace);
    }
    Ok(config)
}

/// Docker / Swarm 连接方案
fn docker_config(
    provider: Provider,
    name: String,
    endpoint: &str,
    auth: &str,
) -> Result<ConnectionConfig, String> {
    let ca = env_opt("OPS_TLS_CA");
    let cert = env_opt("OPS_TLS_CERT");
    let key = env_opt("OPS_TLS_KEY");
    let tls_ready = ca.is_some() && cert.is_some() && key.is_some();

    let mode = match auth {
        "" | "auto" => {
            if tls_ready {
                "tls"
            } else if endpoint.is_empty() {
                "local"
            } else {
                "tcp"
            }
        }
        "local" | "socket" => "local",
        "tcp" | "plain" | "plain_tcp" => "tcp",
        "tls" | "mutual_tls" | "mtls" => "tls",
        other => {
            return Err(format!(
                "未知 OPS_AUTH：{other}（Docker / Swarm 可选 local / tcp / tls）"
            ));
        }
    };

    match mode {
        "local" => Ok(match provider {
            Provider::Swarm => ConnectionConfig::local_swarm(name),
            _ => ConnectionConfig::local_docker(name),
        }),
        "tcp" => {
            if endpoint.is_empty() {
                return Err("远程 Docker 需要 OPS_ENDPOINT，例如 tcp://10.0.0.5:2375".to_string());
            }
            Ok(match provider {
                Provider::Swarm => ConnectionConfig::swarm_tcp(name, endpoint),
                _ => ConnectionConfig::docker_tcp(name, endpoint),
            })
        }
        _ => {
            if endpoint.is_empty() {
                return Err("mTLS 端点需要 OPS_ENDPOINT，例如 https://10.0.0.5:2376".to_string());
            }
            let ca = ca.ok_or("mTLS 需要 OPS_TLS_CA（CA 证书路径）")?;
            let cert = cert.ok_or("mTLS 需要 OPS_TLS_CERT（客户端证书路径）")?;
            let key = key.ok_or("mTLS 需要 OPS_TLS_KEY（客户端私钥路径）")?;
            Ok(match provider {
                Provider::Swarm => ConnectionConfig::swarm_tls(name, endpoint, ca, cert, key),
                _ => ConnectionConfig::docker_tls(name, endpoint, ca, cert, key),
            })
        }
    }
}

/// Kubernetes 连接方案
fn k8s_config(name: String, endpoint: &str, auth: &str) -> Result<ConnectionConfig, String> {
    let kubeconfig = env_opt("OPS_KUBECONFIG");
    let context = env_opt("OPS_KUBE_CONTEXT");
    let token = env_opt("OPS_TOKEN");

    match auth {
        "kubeconfig" | "file" => {
            let path = kubeconfig.ok_or("OPS_AUTH=kubeconfig 需要 OPS_KUBECONFIG（文件路径）")?;
            Ok(ConnectionConfig::kubeconfig(name, path, context))
        }
        "inline" => {
            let path = kubeconfig
                .ok_or("OPS_AUTH=inline 需要 OPS_KUBECONFIG 指向要内联上传的 kubeconfig 文件")?;
            let yaml = std::fs::read_to_string(&path)
                .map_err(|err| format!("读取 kubeconfig 失败（{path}）：{err}"))?;
            Ok(ConnectionConfig::new(
                name,
                Provider::Kubernetes,
                endpoint,
                AuthKind::KubeconfigInline { yaml, context },
            ))
        }
        "token" | "direct" => direct_k8s_config(name, endpoint, token),
        "" | "auto" => {
            if let Some(path) = kubeconfig {
                Ok(ConnectionConfig::kubeconfig(name, path, context))
            } else if token.is_some() {
                direct_k8s_config(name, endpoint, token)
            } else {
                Err("K8s 探测需要 OPS_KUBECONFIG（路径）或 OPS_TOKEN（Bearer Token）".to_string())
            }
        }
        other => Err(format!(
            "未知 OPS_AUTH：{other}（K8s 可选 kubeconfig / inline / token）"
        )),
    }
}

/// 直连 APIServer（Bearer Token + 可选 CA PEM）
fn direct_k8s_config(
    name: String,
    endpoint: &str,
    token: Option<String>,
) -> Result<ConnectionConfig, String> {
    if endpoint.is_empty() {
        return Err("直连 APIServer 需要 OPS_ENDPOINT，例如 https://10.0.0.10:6443".to_string());
    }
    let token = token.ok_or("直连 APIServer 需要 OPS_TOKEN（ServiceAccount Token）")?;
    let ca_pem = match env_opt("OPS_CA_FILE") {
        Some(path) => Some(
            std::fs::read_to_string(&path)
                .map_err(|err| format!("读取 CA 证书失败（{path}）：{err}"))?,
        ),
        None => env_opt("OPS_CA_PEM"),
    };
    Ok(ConnectionConfig::kubernetes_token(
        name,
        endpoint,
        token,
        ca_pem,
        env_flag("OPS_INSECURE_SKIP_TLS_VERIFY"),
    ))
}

/// 按 provider 构建连接器（未启用的 feature 给出明确提示，而不是链接错误）
async fn build_connector(config: ConnectionConfig) -> Result<Arc<dyn Connector>, String> {
    match config.provider {
        Provider::Docker | Provider::Swarm => build_docker(config),
        Provider::Kubernetes => build_k8s(config).await,
    }
}

#[cfg(feature = "docker")]
fn build_docker(config: ConnectionConfig) -> Result<Arc<dyn Connector>, String> {
    let connector = ops::connector::docker::DockerConnector::connect(config)
        .map_err(|err| format!("连接器初始化失败：{err}（{}）", err.classify().hint()))?;
    Ok(Arc::new(connector))
}

#[cfg(not(feature = "docker"))]
fn build_docker(_config: ConnectionConfig) -> Result<Arc<dyn Connector>, String> {
    Err(
        "本构建未启用 `docker` feature：cargo run -p ops --example ops_probe --features docker"
            .to_string(),
    )
}

#[cfg(feature = "k8s")]
async fn build_k8s(config: ConnectionConfig) -> Result<Arc<dyn Connector>, String> {
    let connector = ops::connector::k8s::K8sConnector::connect(config)
        .await
        .map_err(|err| format!("连接器初始化失败：{err}（{}）", err.classify().hint()))?;
    Ok(Arc::new(connector))
}

#[cfg(not(feature = "k8s"))]
async fn build_k8s(_config: ConnectionConfig) -> Result<Arc<dyn Connector>, String> {
    Err(
        "本构建未启用 `k8s` feature：cargo run -p ops --example ops_probe --features k8s"
            .to_string(),
    )
}
