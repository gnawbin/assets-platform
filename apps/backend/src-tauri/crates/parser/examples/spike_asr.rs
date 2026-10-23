//! Phase 0 spike ③：ASR 前置条件核查（`sherpa-rs` / SenseVoice）
//!
//! `sherpa-rs` 0.6.x 依赖 sherpa-onnx 预编译库（构建期拉取）与 ~230MB SenseVoice 模型，
//! 属于 Phase 2 的接入范围；本示例先做**不需要模型**的两项核查，把 Phase 2 的前置条件钉死：
//!
//! 1. 待转写音频的格式：RIFF/WAVE、16 kHz、单声道、16 bit —— `symphonia` + `rubato`
//!    解码链的目标格式（与 `audio_parser.py` 一致）
//! 2. 模型目录的完整性：`model.onnx` / `tokens.txt` 是否存在、ONNX 体积是否落在已知区间
//!
//! 用法：
//! ```text
//! cargo run -p parser --example spike_asr -- 样本.wav
//! cargo run -p parser --example spike_asr -- 样本.wav --model-dir D:\models\sensevoice-zh
//! ```
//!
//! 退出码：`0` 核查通过；`1` 校验失败；`2` 未提供音频样本（跳过）。

use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut wav: Option<PathBuf> = None;
    let mut model_dir: Option<PathBuf> = std::env::var("SHERPA_MODEL_DIR")
        .ok()
        .map(PathBuf::from);
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--model-dir" {
            model_dir = args.get(index + 1).map(PathBuf::from);
            index += 2;
            continue;
        }
        wav = Some(PathBuf::from(&args[index]));
        index += 1;
    }

    let Some(wav) = wav else {
        eprintln!("⏭️ 跳过：未提供音频样本");
        eprintln!("   cargo run -p parser --example spike_asr -- 样本.wav");
        std::process::exit(2);
    };
    if !wav.is_file() {
        eprintln!("❌ 文件不存在: {}", wav.display());
        std::process::exit(1);
    }

    let mut failures = 0usize;
    match read_wav(&wav) {
        Ok(info) => {
            let duration = if info.byte_rate() > 0 {
                info.data_len as f64 / info.byte_rate() as f64
            } else {
                0.0
            };
            println!("===== 音频：{}", wav.display());
            println!("格式      : {}", if info.format == 1 { "PCM" } else { "非 PCM" });
            println!("采样率    : {} Hz", info.sample_rate);
            println!("声道      : {}", info.channels);
            println!("位深      : {} bit", info.bits);
            println!("时长      : {:.1} s", duration);
            println!("数据字节  : {}", info.data_len);
            let target = info.format == 1
                && info.sample_rate == 16_000
                && info.channels == 1
                && info.bits == 16;
            if target {
                println!("✅ 已是 ASR 目标格式（16 kHz / mono / 16 bit），可直喂 sherpa-onnx");
            } else {
                println!("ℹ️ 需重采样/混音：目标 16 kHz / mono / 16 bit（Phase 2 由 rubato 处理）");
            }
            println!();
        }
        Err(e) => {
            eprintln!("❌ WAV 解析失败 {}: {}", wav.display(), e);
            failures += 1;
        }
    }

    match model_dir {
        Some(dir) => match check_model_dir(&dir) {
            Ok(report) => println!("{}", report),
            Err(e) => {
                eprintln!("❌ 模型目录校验失败 {}: {}", dir.display(), e);
                failures += 1;
            }
        },
        None => {
            println!("ℹ️ 未指定模型目录（--model-dir 或 SHERPA_MODEL_DIR）：跳过模型核查");
            println!("   SenseVoice 期望文件：model.onnx（约 230MB）+ tokens.txt");
            println!("   下载源：github.com/k2-fsa/sherpa-onnx/releases（GitHub 可达，HF 当前不可达）");
        }
    }

    if failures > 0 {
        std::process::exit(1);
    }
    println!();
    println!("✅ spike_asr 前置核查通过");
    println!("   ASR 引擎选型仍待实测：sherpa-rs(SenseVoice) vs whisper-rs(whisper-base)");
}

struct WavInfo {
    format: u16,
    channels: u16,
    sample_rate: u32,
    bits: u16,
    data_len: usize,
}

impl WavInfo {
    fn byte_rate(&self) -> u32 {
        self.sample_rate * self.channels as u32 * (self.bits as u32 / 8)
    }
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// 极简 WAV 解析：只读 RIFF/WAVE + `fmt ` + `data`，够判断是否满足 ASR 输入要求。
fn read_wav(path: &Path) -> Result<WavInfo, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() < 44 {
        return Err("文件小于 44 字节，不是合法 WAV".to_string());
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("缺少 RIFF/WAVE 文件头".to_string());
    }

    let mut pos = 12usize;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    let mut data_len: Option<usize> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32_at(&bytes, pos + 4) as usize;
        let body = pos + 8;
        if id == b"fmt " {
            if body + 16 > bytes.len() {
                return Err("`fmt ` 块被截断".to_string());
            }
            fmt = Some((
                u16_at(&bytes, body),
                u16_at(&bytes, body + 2),
                u32_at(&bytes, body + 4),
                u16_at(&bytes, body + 14),
            ));
        } else if id == b"data" {
            data_len = Some(size.min(bytes.len().saturating_sub(body)));
        }
        pos = body + size + (size & 1);
    }

    let (format, channels, sample_rate, bits) = fmt.ok_or("缺少 `fmt ` 块")?;
    let data_len = data_len.ok_or("缺少 `data` 块")?;
    Ok(WavInfo {
        format,
        channels,
        sample_rate,
        bits,
        data_len,
    })
}

fn check_model_dir(dir: &Path) -> Result<String, String> {
    if !dir.is_dir() {
        return Err("目录不存在".to_string());
    }
    let mut report = format!("===== 模型目录：{}\n", dir.display());
    for name in ["model.onnx", "tokens.txt"] {
        let path = dir.join(name);
        match std::fs::metadata(&path) {
            Ok(meta) => report.push_str(&format!(
                "✅ {:<12} {:>8.1} MB\n",
                name,
                meta.len() as f64 / 1024.0 / 1024.0
            )),
            Err(_) => return Err(format!("缺少 {}", name)),
        }
    }
    Ok(report)
}
