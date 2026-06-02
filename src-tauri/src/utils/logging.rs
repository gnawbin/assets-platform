//! 日志初始化模块
//!
//! 基于 tracing + tracing-subscriber 的统一日志管理
//! - 开发环境：控制台文本输出，debug 级别
//! - 生产环境：JSON 格式输出到文件 + 控制台，info 级别
//! - 支持 RUST_LOG 环境变量动态控制日志级别

use anyhow::Result;
use std::fs;
use std::path::PathBuf;
use tracing_subscriber::{
    fmt,
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
    Layer,
};

/// 获取日志文件目录
fn get_log_dir() -> PathBuf {
    let base = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("assets-platform")
        .join("logs");
    base
}

/// 初始化 tracing 日志系统
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

    // 注册订阅者
    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer);

    // 生产环境额外添加文件日志层
    #[cfg(not(debug_assertions))]
    let subscriber = {
        let log_dir = get_log_dir();
        fs::create_dir_all(&log_dir)?;

        // 按日期轮转日志文件
        let timestamp = chrono::Local::now().format("%Y%m%d").to_string();
        let log_file = log_dir.join(format!("app-{}.log", timestamp));

        let file = fs::File::create(&log_file)?;
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

    subscriber.try_init()?;

    tracing::info!("日志系统初始化完成");
    if cfg!(debug_assertions) {
        tracing::debug!("运行在开发模式");
    } else {
        tracing::info!("运行在生产模式，日志目录: {:?}", get_log_dir());
    }

    Ok(())
}
