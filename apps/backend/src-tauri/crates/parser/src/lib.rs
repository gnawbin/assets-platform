//! Rust 原生多模态解析后端（`DOC_PARSER_BACKEND=local`）
//!
//! 用纯 Rust crate 在进程内完成 PDF / Office（以及后续的图片 / 音频 / 视频）的
//! 文本化归一，替代 Python `doc-parser` 侧车。输出契约 [`ParseResult`] 与 Python
//! 侧车逐字段一致，因此 `local` / `remote` 两种后端对上层调用方完全透明。
//!
//! 设计文档：`docs/Rust原生多模态解析后端设计方案.md`
//!
//! # 当前阶段（Phase 1）
//!
//! | 模态 | 状态 |
//! |------|------|
//! | PDF | ✅ 文本提取 + 50 字符阈值 + `source_type` 元数据（扫描件 OCR 见 Phase 2） |
//! | Office docx/xlsx | ✅ 段落 + 表格 `" \| "` 拼接、`【工作表: x】` 分组 |
//! | Office doc/xls | ✅ 尽力解析（Python 侧原本直接失败） |
//! | image / audio / video | ⏳ Phase 3~5，当前返回明确错误 |

mod ocr;
mod office;
pub mod options;
mod pdf;
pub mod text;
pub mod types;

use std::path::{Path, PathBuf};

pub use options::ParseOptions;
pub use types::ParseResult;

/// 支持的文件格式 → 文件类型映射
///
/// 与 `apps/doc-parser/services/parse_service.py:31-37` 的 `SUPPORTED_FORMATS`
/// 逐项一致（顺序也一致），便于两侧做输出对比。
pub const SUPPORTED_FORMATS: &[(&str, &[&str])] = &[
    ("pdf", &["pdf"]),
    ("document", &["doc", "docx", "xls", "xlsx"]),
    (
        "image",
        &["jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff"],
    ),
    ("audio", &["mp3", "wav", "ogg", "flac", "m4a", "aac"]),
    ("video", &["mp4", "avi", "mov", "mkv", "wmv", "flv"]),
];

/// 本地后端已实现的扩展名（服务层据此判断能否走 `local`，未实现时给出明确错误）
pub const LOCAL_SUPPORTED_EXTS: &[&str] = &["pdf", "doc", "docx", "xls", "xlsx"];

/// 扩展名 → 文件类型（`pdf` / `document` / `image` / `audio` / `video`）
pub fn detect_file_type(ext: &str) -> Option<&'static str> {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    SUPPORTED_FORMATS
        .iter()
        .find(|(_, exts)| exts.contains(&ext.as_str()))
        .map(|(file_type, _)| *file_type)
}

/// 是否为 doc-parser 支持的文件类型
pub fn is_supported_ext(ext: &str) -> bool {
    detect_file_type(ext).is_some()
}

/// 本地后端是否已实现该扩展名
pub fn is_locally_supported_ext(ext: &str) -> bool {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    LOCAL_SUPPORTED_EXTS.contains(&ext.as_str())
}

/// 解析本地文件 → 纯文本 + 图片路径
///
/// 内部用 `tokio::task::spawn_blocking` 包住同步解析，避免 PDF / Office 的 CPU
/// 密集解析阻塞异步运行时（设计文档 §4.3）。`options` 语义与 Python `/parse`
/// 接口的 `options` 字段一致。
pub async fn parse_file(
    path: impl AsRef<Path>,
    options: ParseOptions,
) -> Result<ParseResult, String> {
    let path: PathBuf = path.as_ref().to_path_buf();
    match tokio::task::spawn_blocking(move || parse_file_blocking(&path, &options)).await {
        Ok(result) => result,
        Err(e) => Err(format!("解析任务执行失败: {}", e)),
    }
}

/// 同步解析入口（供 `spawn_blocking` 内调用，也方便同步上下文与单测直接使用）
pub fn parse_file_blocking(path: &Path, options: &ParseOptions) -> Result<ParseResult, String> {
    if !path.is_file() {
        return Err(format!("文件不存在: {}", path.display()));
    }

    let ext = text::ext_of(path);
    match detect_file_type(&ext) {
        Some("pdf") => pdf::parse(path, options),
        Some("document") => office::parse(path, options),
        Some(file_type) => Err(format!(
            "本地解析尚未支持 .{}（{}），请切换 remote 后端或等待后续阶段",
            ext, file_type
        )),
        _ => Err(format!("不支持的文件类型: .{}", ext)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_formats_match_python() {
        // 5 类 / 24 个扩展名（pdf 1 + document 4 + image 7 + audio 6 + video 6）
        assert_eq!(SUPPORTED_FORMATS.len(), 5);
        let total: usize = SUPPORTED_FORMATS.iter().map(|(_, e)| e.len()).sum();
        assert_eq!(
            total, 24,
            "❌ 扩展名总数应与 parse_service.py 的 SUPPORTED_FORMATS 一致"
        );
    }

    #[test]
    fn test_detect_file_type() {
        assert_eq!(detect_file_type("pdf"), Some("pdf"));
        assert_eq!(detect_file_type(".PDF"), Some("pdf"));
        assert_eq!(detect_file_type("DOCX"), Some("document"));
        assert_eq!(detect_file_type("xls"), Some("document"));
        assert_eq!(detect_file_type("tiff"), Some("image"));
        assert_eq!(detect_file_type("m4a"), Some("audio"));
        assert_eq!(detect_file_type("mkv"), Some("video"));
        assert_eq!(detect_file_type("exe"), None);
    }

    #[test]
    fn test_is_locally_supported_ext() {
        assert!(is_locally_supported_ext("pdf"));
        assert!(is_locally_supported_ext(".docx"));
        assert!(!is_locally_supported_ext("png"));
        assert!(!is_locally_supported_ext("mp4"));
    }

    #[test]
    fn test_missing_file_reports_python_message() {
        let err = parse_file_blocking(
            Path::new("C:/definitely/not/here.pdf"),
            &ParseOptions::default(),
        )
        .expect_err("❌ 不存在的文件应报错");
        assert!(err.starts_with("文件不存在: "), "实际错误：{}", err);
    }

    #[test]
    fn test_unsupported_ext_matches_python_message() {
        let dir = std::env::temp_dir();
        let path = dir.join("parser_unsupported_fixture.exe");
        std::fs::write(&path, b"x").expect("❌ 写入临时文件失败");
        let err = parse_file_blocking(&path, &ParseOptions::default())
            .expect_err("❌ 不支持的扩展名应报错");
        let _ = std::fs::remove_file(&path);
        assert_eq!(err, "不支持的文件类型: .exe");
    }
}
