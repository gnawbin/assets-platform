//! 解析选项
//!
//! 与 Python 侧车 `/parse` 接口的 `options` 字段对齐（`dict` 语义：缺省即默认），
//! 由 `crates/service` 把 `serde_json::Value` 转成本结构后传入，避免解析层
//! 到处做 `json["xxx"].as_str()`。

use std::path::PathBuf;

use serde_json::Value;

/// OCR 语言缺省值（与 `parsers/pdf_parser.py:17` 一致）
pub const DEFAULT_OCR_LANGUAGE: &str = "chi_sim+eng";
/// 视频抽帧固定间隔缺省值（秒，与 `video_parser.py` 的 `FRAME_INTERVAL_SEC` 一致）
pub const DEFAULT_FRAME_INTERVAL: f64 = 30.0;
/// 视频抽帧上限（Rust 侧新增保护，见设计文档 §5.5）
pub const DEFAULT_MAX_FRAMES: usize = 300;

/// 单个文件的解析选项
#[derive(Debug, Clone)]
pub struct ParseOptions {
    /// OCR 语言，形如 `chi_sim+eng`
    pub ocr_language: String,
    /// 视频固定间隔抽帧秒数
    pub frame_interval: f64,
    /// 视频抽帧张数上限
    pub max_frames: usize,
    /// 中间产物（视频帧等）落盘目录；`None` 表示用系统临时目录
    pub work_dir: Option<PathBuf>,
    /// 视频解析后是否跳过向量化入库（聊天场景为 `true`）
    pub skip_index: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            ocr_language: DEFAULT_OCR_LANGUAGE.to_string(),
            frame_interval: DEFAULT_FRAME_INTERVAL,
            max_frames: DEFAULT_MAX_FRAMES,
            work_dir: None,
            skip_index: true,
        }
    }
}

impl ParseOptions {
    /// 从 `serde_json::Value` 读取；非对象 / 字段缺失 / 类型不符时回落默认值
    pub fn from_json(options: &Value) -> Self {
        let mut opts = Self::default();
        let Some(obj) = options.as_object() else {
            return opts;
        };

        if let Some(lang) = obj.get("ocr_language").and_then(Value::as_str) {
            if !lang.trim().is_empty() {
                opts.ocr_language = lang.to_string();
            }
        }
        if let Some(sec) = obj.get("frame_interval").and_then(Value::as_f64) {
            if sec > 0.0 {
                opts.frame_interval = sec;
            }
        }
        if let Some(max) = obj.get("max_frames").and_then(Value::as_u64) {
            if max > 0 {
                opts.max_frames = max as usize;
            }
        }
        if let Some(dir) = obj.get("work_dir").and_then(Value::as_str) {
            if !dir.trim().is_empty() {
                opts.work_dir = Some(PathBuf::from(dir));
            }
        }
        if let Some(skip) = obj.get("skip_index").and_then(Value::as_bool) {
            opts.skip_index = skip;
        }
        opts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_match_python() {
        let opts = ParseOptions::default();
        assert_eq!(opts.ocr_language, "chi_sim+eng");
        assert_eq!(opts.frame_interval, 30.0);
        assert!(opts.skip_index);
    }

    #[test]
    fn from_json_overrides_known_fields() {
        let opts = ParseOptions::from_json(&json!({
            "ocr_language": "eng",
            "frame_interval": 5.5,
            "max_frames": 12,
            "work_dir": "C:/tmp/work",
            "skip_index": false,
        }));
        assert_eq!(opts.ocr_language, "eng");
        assert_eq!(opts.frame_interval, 5.5);
        assert_eq!(opts.max_frames, 12);
        assert_eq!(
            opts.work_dir.as_deref().and_then(|p| p.to_str()),
            Some("C:/tmp/work")
        );
        assert!(!opts.skip_index);
    }

    #[test]
    fn from_json_ignores_invalid_values() {
        let opts = ParseOptions::from_json(&json!({
            "ocr_language": "",
            "frame_interval": 0,
            "max_frames": -3,
            "work_dir": 42,
            "skip_index": "yes",
        }));
        assert_eq!(opts.ocr_language, DEFAULT_OCR_LANGUAGE);
        assert_eq!(opts.frame_interval, DEFAULT_FRAME_INTERVAL);
        assert_eq!(opts.max_frames, DEFAULT_MAX_FRAMES);
        assert!(opts.work_dir.is_none());
        assert!(opts.skip_index);
    }

    #[test]
    fn from_json_accepts_non_object() {
        let opts = ParseOptions::from_json(&json!(null));
        assert_eq!(opts.ocr_language, DEFAULT_OCR_LANGUAGE);
    }
}
