//! 日志初始化模块
//!
//! 基于 tracing + tracing-subscriber + OpenTelemetry 的统一日志管理
//! - 开发环境：控制台文本输出，debug 级别
//! - 生产环境：JSON 格式输出到文件 + 控制台，info 级别
//! - 支持 RUST_LOG 环境变量动态控制日志级别
//! - 支持通过 OTEL_ENABLED 环境变量开关 OpenTelemetry 导出
//! - 支持通过 OTEL_EXPORTER_OTLP_PROTOCOL 选择 gRPC 或 HTTP 协议

use anyhow::{anyhow, Result};
use once_cell::sync::OnceCell;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use std::path::PathBuf;
use std::sync::Mutex;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

// ======================== 环境变量常量 ========================

/// 是否启用 OpenTelemetry 导出（默认 true）
const ENV_OTEL_ENABLED: &str = "OTEL_ENABLED";
/// OTLP 传输协议：grpc 或 http_protobuf（默认 grpc）
const ENV_OTEL_PROTOCOL: &str = "OTEL_EXPORTER_OTLP_PROTOCOL";
/// OTLP 接收端地址（默认 http://localhost:4317）
const ENV_OTEL_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";
/// 服务名称（默认 assets-platform）
const ENV_OTEL_SERVICE_NAME: &str = "OTEL_SERVICE_NAME";

// ======================== OTel 资源管理 ========================

/// 保存 OTel providers，用于应用退出时的清理
struct OTelGuard {
    tracer_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
    logger_provider: Option<opentelemetry_sdk::logs::SdkLoggerProvider>,
}

static OTEL_GUARD: OnceCell<Mutex<OTelGuard>> = OnceCell::new();

fn get_otel_guard() -> &'static Mutex<OTelGuard> {
    OTEL_GUARD.get_or_init(|| {
        Mutex::new(OTelGuard {
            tracer_provider: None,
            logger_provider: None,
        })
    })
}

/// 获取日志文件目录
fn get_log_dir() -> PathBuf {
    let base = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("assets-platform")
        .join("logs");
    base
}

/// 检查 OTel 是否启用
fn is_otel_enabled() -> bool {
    std::env::var(ENV_OTEL_ENABLED)
        .ok()
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(true) // 默认启用
}

/// 获取 OTLP 协议
fn get_otel_protocol() -> &'static str {
    let protocol = std::env::var(ENV_OTEL_PROTOCOL).unwrap_or_else(|_| "grpc".to_string());
    match protocol.as_str() {
        "http_protobuf" => "http_protobuf",
        _ => "grpc",
    }
}

/// 获取 OTLP 端点
fn get_otel_endpoint() -> String {
    std::env::var(ENV_OTEL_ENDPOINT).unwrap_or_else(|_| "http://localhost:4317".to_string())
}

/// 获取服务名称
fn get_otel_service_name() -> String {
    std::env::var(ENV_OTEL_SERVICE_NAME).unwrap_or_else(|_| "assets-platform".to_string())
}

/// 创建 OTel 资源
fn create_otel_resource() -> opentelemetry_sdk::Resource {
    opentelemetry_sdk::Resource::builder()
        .with_service_name(get_otel_service_name())
        .with_attribute(opentelemetry::KeyValue::new(
            "service.version",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_attribute(opentelemetry::KeyValue::new(
            "deployment.environment",
            if cfg!(debug_assertions) {
                "development"
            } else {
                "production"
            },
        ))
        .build()
}

/// 创建 OTLP SpanExporter（根据协议选择 gRPC 或 HTTP）
fn create_span_exporter() -> Result<opentelemetry_otlp::SpanExporter> {
    let protocol = get_otel_protocol();
    let endpoint = get_otel_endpoint();

    match protocol {
        "http_protobuf" => opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(endpoint)
            .build()
            .map_err(|e| anyhow!("创建 OTLP SpanExporter 失败: {}", e)),
        _ => opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()
            .map_err(|e| anyhow!("创建 OTLP SpanExporter 失败: {}", e)),
    }
}

/// 创建 OTLP LogExporter（根据协议选择 gRPC 或 HTTP）
fn create_log_exporter() -> Result<opentelemetry_otlp::LogExporter> {
    let protocol = get_otel_protocol();
    let endpoint = get_otel_endpoint();

    match protocol {
        "http_protobuf" => opentelemetry_otlp::LogExporter::builder()
            .with_http()
            .with_endpoint(endpoint)
            .build()
            .map_err(|e| anyhow!("创建 OTLP LogExporter 失败: {}", e)),
        _ => opentelemetry_otlp::LogExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()
            .map_err(|e| anyhow!("创建 OTLP LogExporter 失败: {}", e)),
    }
}

/// 初始化 tracing 日志系统（不含 OTel）
///
/// 初始化控制台日志和文件日志（生产环境）。
/// OTel 部分需要 Tokio 运行时，由 `init_otel()` 在 setup 中调用。
///
/// - 开发环境 (debug_assertions)：控制台彩色文本输出，默认 debug 级别
/// - 生产环境：JSON 格式写入文件 + 控制台输出，默认 info 级别
/// - 均可通过 `RUST_LOG` 环境变量覆盖日志级别
pub fn init_tracing() -> Result<()> {
    // 从环境变量或默认值创建过滤器
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        if cfg!(debug_assertions) {
            EnvFilter::new("debug")
        } else {
            EnvFilter::new("info")
        }
    });

    // 控制台层：开发环境用文本格式，生产环境用 JSON 格式
    let console_layer = if cfg!(debug_assertions) {
        // 开发环境：彩色文本输出
        fmt::layer()
            .with_target(true)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true)
            .with_ansi(true)
            .boxed()
    } else {
        // 生产环境：JSON 格式输出到控制台
        fmt::layer()
            .json()
            .with_target(true)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true)
            .boxed()
    };

    // 构建 subscriber - 使用 registry + env_filter + console_layer
    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer);

    // 生产环境额外添加文件日志层
    #[cfg(not(debug_assertions))]
    let subscriber = {
        let log_dir = get_log_dir();
        std::fs::create_dir_all(&log_dir)?;

        // 按日期轮转日志文件
        let timestamp = chrono::Local::now().format("%Y%m%d").to_string();
        let log_file = log_dir.join(format!("app-{}.log", timestamp));

        let file = std::fs::File::create(&log_file)?;
        let file_layer = fmt::layer()
            .json()
            .with_writer(file)
            .with_target(true)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true)
            .boxed();

        subscriber.with(file_layer)
    };

    // 先初始化基础 subscriber（不含 OTel）
    subscriber.try_init()?;

    tracing::info!("日志系统初始化完成（基础层）");
    if cfg!(debug_assertions) {
        tracing::debug!("运行在开发模式");
    } else {
        tracing::info!("运行在生产模式，日志目录: {:?}", get_log_dir());
    }

    Ok(())
}

/// 初始化 OpenTelemetry 层（需要在 Tokio 运行时上下文中调用）
///
/// 在 Tauri 的 `setup` 回调中调用，此时 Tokio 运行时已就绪。
/// 如果 `OTEL_ENABLED=false` 则跳过。
///
/// 由于 tracing subscriber 已在 `init_tracing()` 中初始化，
/// 这里通过 `opentelemetry::global` API 设置全局 tracer provider，
/// 这样 tracing-opentelemetry 可以在运行时自动拾取。
pub fn init_otel() -> Result<()> {
    if !is_otel_enabled() {
        tracing::info!(
            "OpenTelemetry 已禁用（可通过 {} 环境变量启用）",
            ENV_OTEL_ENABLED
        );
        return Ok(());
    }

    tracing::info!(
        "OpenTelemetry 已启用，协议: {}, 端点: {}",
        get_otel_protocol(),
        get_otel_endpoint()
    );

    let resource = create_otel_resource();

    // 初始化 TracerProvider
    let span_exporter = create_span_exporter()?;
    let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(span_exporter)
        .build();

    // 初始化 LoggerProvider
    let log_exporter = create_log_exporter()?;
    let logger_provider = opentelemetry_sdk::logs::SdkLoggerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(log_exporter)
        .build();

    // 通过 opentelemetry::global 设置全局 tracer provider
    // tracing-opentelemetry 会自动拾取全局 tracer provider
    opentelemetry::global::set_tracer_provider(tracer_provider.clone());

    // 注意：opentelemetry 0.32.0 的 global API 中没有 set_logger_provider 方法。
    // LoggerProvider 通过 opentelemetry-appender-tracing 的 OpenTelemetryTracingBridge
    // 作为 tracing Layer 集成，需要在 subscriber 初始化时添加。
    // 此处仅保存 logger_provider 以便应用退出时 shutdown。

    // 保存 providers 以便 shutdown
    let mut guard = get_otel_guard()
        .lock()
        .map_err(|e| anyhow!("获取 OTel guard 锁失败: {}", e))?;
    guard.tracer_provider = Some(tracer_provider);
    guard.logger_provider = Some(logger_provider);

    tracing::info!("OpenTelemetry 层初始化完成");
    Ok(())
}

/// 关闭 tracing 系统，确保所有 spans 和 logs 被 flush
///
/// 应在应用退出时调用
pub fn shutdown_tracing() {
    tracing::info!("正在关闭日志系统...");

    // 关闭 OTel providers，确保数据被导出
    if let Some(guard) = OTEL_GUARD.get() {
        if let Ok(mut guard) = guard.lock() {
            // 先 shutdown tracer provider
            if let Some(tp) = guard.tracer_provider.take() {
                if let Err(e) = tp.shutdown() {
                    eprintln!("关闭 OTel TracerProvider 时出错: {}", e);
                }
            }
            // 再 shutdown logger provider
            if let Some(lp) = guard.logger_provider.take() {
                if let Err(e) = lp.shutdown() {
                    eprintln!("关闭 OTel LoggerProvider 时出错: {}", e);
                }
            }
        }
    }
}
