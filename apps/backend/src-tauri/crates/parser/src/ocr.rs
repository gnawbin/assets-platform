//! OCR 兜底（Phase 2 实现）
//!
//! Python 侧用 `pdf2image` + `pytesseract`：PDF 文字不足 50 字符时逐页转 300dpi
//! 位图做 OCR，失败则静默降级为空文本（`pdf_parser.py:48-59`）。
//!
//! Rust 侧规划：`pdf_oxide` 的 `rendering` 特性负责页面位图，`ort` + PP-OCR /
//! tesseract 负责识别（设计文档 §5.3、§11 Phase 2）。在 OCR 后端接入前，本模块
//! 返回 `None`，PDF 解析器据此走"降级为空文本"的 Python 行为，保证聊天链路不失败。

use std::path::Path;

use tracing::warn;

/// 对整份 PDF 做 OCR，返回按页 `\n` 拼接的文本
///
/// 返回 `None` 表示 OCR 不可用或未识别出内容，调用方应保持空文本并记录
/// `source_type = "text"`（与 Python 一致）。
pub(crate) fn ocr_pdf(path: &Path, language: &str) -> Option<String> {
    warn!(
        "本地 OCR 后端未启用，扫描件降级为空文本: file={}, lang={}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
        language
    );
    None
}
