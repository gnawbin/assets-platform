//! Phase 0 spike ④：`ffmpeg` / `ffprobe` 在 Windows 下的可用性（probe / 抽帧 / 场景检测）
//!
//! Phase 3（视频）需要外部 ffmpeg 二进制，先验证三件事：
//! 1. 二进制定位：PATH、或 `FFMPEG_PATH` / `FFPROBE_PATH` 指定（后续接 `ffmpeg-sidecar`）
//! 2. `ffprobe -print_format json -show_format -show_streams` 能否拿到时长 / 分辨率 / 音轨
//! 3. `-frames:v 1` 单帧导出 + `select='gt(scene,0.3)'` 场景检测能否跑通
//!
//! 用法：
//! ```text
//! cargo run -p parser --example spike_ffmpeg
//! cargo run -p parser --example spike_ffmpeg -- 样本.mp4
//! ```
//!
//! 退出码：`0` 通过；`1` 运行失败；`2` 未找到 ffmpeg（跳过）。

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

fn main() {
    let media: Option<PathBuf> = std::env::args().nth(1).map(PathBuf::from);
    let ffmpeg = locate("FFMPEG_PATH", "ffmpeg");
    let ffprobe = locate("FFPROBE_PATH", "ffprobe");

    let (Some(ffmpeg), Some(ffprobe)) = (ffmpeg, ffprobe) else {
        eprintln!("⏭️ 跳过：未找到 ffmpeg / ffprobe");
        eprintln!("   Windows: winget install Gyan.FFmpeg（或下载 LGPL 构建并设置 FFMPEG_PATH）");
        eprintln!("   注意：必须使用不带 --enable-gpl 的 LGPL 构建");
        std::process::exit(2);
    };

    println!("ffmpeg  : {}", ffmpeg);
    println!("ffprobe : {}", ffprobe);
    println!();
    println!("--- ffmpeg -version ---");
    println!("{}", first_line(&run(&ffmpeg, &["-version"]).unwrap_or_default()));
    println!("--- ffprobe -version ---");
    println!("{}", first_line(&run(&ffprobe, &["-version"]).unwrap_or_default()));
    println!();

    let Some(media) = media else {
        println!("ℹ️ 未提供视频样本：仅验证了二进制可用性。");
        println!("   cargo run -p parser --example spike_ffmpeg -- 样本.mp4");
        return;
    };
    if !media.is_file() {
        eprintln!("❌ 文件不存在: {}", media.display());
        std::process::exit(1);
    }

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/spike_out/video");
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("❌ 创建输出目录失败: {}", e);
        std::process::exit(1);
    }
    let media_str = media.to_string_lossy().to_string();

    // ① probe
    let started = Instant::now();
    match run(
        &ffprobe,
        &[
            "-v", "error", "-print_format", "json", "-show_format", "-show_streams", &media_str,
        ],
    ) {
        Ok(json) => {
            println!(
                "--- ffprobe（{} ms，{} 字节 JSON）---",
                started.elapsed().as_millis(),
                json.len()
            );
            println!("{}", json.chars().take(1500).collect::<String>());
            println!();
        }
        Err(e) => {
            eprintln!("❌ ffprobe 失败: {}", e);
            std::process::exit(1);
        }
    }

    // ② 单帧导出
    let frame_path = out_dir.join("frame_at_1s.png");
    let frame_str = frame_path.to_string_lossy().to_string();
    let started = Instant::now();
    match run(
        &ffmpeg,
        &[
            "-y", "-loglevel", "error", "-ss", "1", "-i", &media_str, "-frames:v", "1", &frame_str,
        ],
    ) {
        Ok(_) => {
            let size = std::fs::metadata(&frame_path).map(|m| m.len()).unwrap_or(0);
            println!(
                "✅ 抽帧：{}（{} KB，{} ms）",
                frame_path.display(),
                size / 1024,
                started.elapsed().as_millis()
            );
            if size == 0 {
                eprintln!("⚠️ 帧文件为空：样本可能短于 1 秒，试试 -ss 0");
            }
        }
        Err(e) => {
            eprintln!("❌ 抽帧失败: {}", e);
            std::process::exit(1);
        }
    }

    // ③ 场景检测（只扫前 120s，避免长视频拖时间）
    let started = Instant::now();
    match run(
        &ffmpeg,
        &[
            "-hide_banner",
            "-nostats",
            "-loglevel",
            "info",
            "-t",
            "120",
            "-i",
            &media_str,
            "-vf",
            "select='gt(scene,0.3)',showinfo",
            "-f",
            "null",
            "-",
        ],
    ) {
        Ok(log) => {
            println!(
                "✅ 场景检测（阈值 0.3，前 120s）：{} 个切换点，{} ms",
                log.matches("pts_time:").count(),
                started.elapsed().as_millis()
            );
        }
        Err(e) => {
            eprintln!("❌ 场景检测失败: {}", e);
            std::process::exit(1);
        }
    }

    println!();
    println!("✅ ffmpeg spike 通过：probe / 抽帧 / 场景检测三条命令均可用");
    println!("   Phase 3 结论：ffmpeg-sidecar 只需托管二进制，命令形态无需改动");
}

/// 定位二进制：优先环境变量，其次 PATH（用 `-version` 探测）。
fn locate(env_key: &str, name: &str) -> Option<String> {
    if let Ok(value) = std::env::var(env_key) {
        if !value.trim().is_empty() {
            return Some(value);
        }
    }
    let ok = Command::new(name)
        .arg("-version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if ok {
        Some(name.to_string())
    } else {
        None
    }
}

/// 运行命令，成功返回 stdout+stderr；失败返回错误描述。
fn run(bin: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(bin)
        .args(args)
        .output()
        .map_err(|e| format!("启动 {} 失败: {}", bin, e))?;
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if output.status.success() {
        Ok(text)
    } else {
        Err(format!(
            "{} 退出码 {:?}: {}",
            bin,
            output.status.code(),
            text.chars().take(500).collect::<String>()
        ))
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("(无输出)").to_string()
}
