//! `agent-runner` CLI
//!
//! ```bash
//! agent-runner probe
//! agent-runner step --in /workspace/in/step.json --out /workspace/out/step.json
//! ```

use std::path::{Path, PathBuf};

use agent_core::protocol::{PROTOCOL, StepInput, StepOutput};
use agent_runner::runner::{self, StepError};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "agent-runner",
    version,
    about = "OpenShell 沙箱内单步 agent 执行器"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 执行一步：读 `--in`，写 `--out`
    Step {
        /// 输入 file（step.json）
        #[arg(long = "in")]
        input: PathBuf,
        /// 输出 file（step.json）
        #[arg(long = "out")]
        output: PathBuf,
    },
    /// 打印协议版本与构建信息
    Probe,
}

#[tokio::main]
async fn main() {
    match Cli::parse().command {
        Command::Probe => {
            println!(
                "{}",
                serde_json::json!({"protocol": PROTOCOL, "version": env!("CARGO_PKG_VERSION")})
            );
        }
        Command::Step { input, output } => {
            if let Err(error) = run_step_command(&input, &output).await {
                // 尽量把错误也写进 out/step.json，平台按 kind=error 处理
                let _ = write_output(&output, &StepOutput::error(error.message.clone())).await;
                eprintln!("agent-runner: {}", error.message);
                std::process::exit(error.exit_code);
            }
        }
    }
}

async fn run_step_command(input: &Path, output: &Path) -> Result<(), StepError> {
    let raw = tokio::fs::read_to_string(input)
        .await
        .map_err(|error| StepError::invalid(format!("读取 {} 失败：{error}", input.display())))?;
    let step: StepInput = serde_json::from_str(&raw)
        .map_err(|error| StepError::invalid(format!("step.json 非法：{error}")))?;

    let output_value = runner::run_step(step).await?;
    write_output(output, &output_value)
        .await
        .map_err(|error| StepError::invalid(format!("写入 {} 失败：{error}", output.display())))
}

async fn write_output(path: &Path, output: &StepOutput) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let text = serde_json::to_string_pretty(output).unwrap_or_else(|_| "{}".to_string());
    tokio::fs::write(path, text).await
}