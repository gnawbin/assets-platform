//! PDF 解析
//!
//! 对齐 `apps/doc-parser/parsers/pdf_parser.py`：
//! 1. 逐页提取文字并用 `\n` 拼接；
//! 2. 去空白后不足 [`MIN_TEXT_LENGTH`] 个字符 → 视为扫描件，走 OCR 兜底；
//! 3. `metadata.source_type` 取 `text` / `ocr`，OCR 也没识别出内容时回落 `text`。

use std::path::Path;
use std::time::Instant;

use pdf_oxide::document::PdfDocument;
use serde_json::json;

use crate::{ocr, options::ParseOptions, text, types::ParseResult};

/// 少于该字符数视为扫描件（与 `pdf_parser.py:12` 的 `MIN_TEXT_LENGTH` 一致）
pub const MIN_TEXT_LENGTH: usize = 50;

/// 解析 PDF
pub(crate) fn parse(path: &Path, options: &ParseOptions) -> Result<ParseResult, String> {
    let started = Instant::now();

    let doc = PdfDocument::open(path).map_err(|e| open_error(&e.to_string()))?;
    let page_count = doc
        .page_count()
        .map_err(|e| format!("PDF 页数读取失败: {}", e))?;

    let mut pages = Vec::with_capacity(page_count);
    for index in 0..page_count {
        // `extract_text` 是基础布局提取；个别页失败时退到 auto 模式，仍失败则该页为空
        let page_text = doc
            .extract_text(index)
            .or_else(|_| doc.extract_text_auto(index))
            .unwrap_or_default();
        pages.push(page_text);
    }
    let mut raw_text = pages.join("\n");

    // 文字不足 → 扫描件 OCR 兜底（OCR 不可用时保持空文本，即 Python 的静默降级）
    let mut source_type = "text";
    if raw_text.trim().chars().count() < MIN_TEXT_LENGTH {
        raw_text = ocr::ocr_pdf(path, &options.ocr_language).unwrap_or_default();
        source_type = if raw_text.trim().is_empty() {
            "text"
        } else {
            "ocr"
        };
    }

    Ok(
        ParseResult::new(text::basename(path), "pdf", raw_text).with_metadata(json!({
            "page_count": page_count,
            "source_type": source_type,
            "parse_duration_ms": text::elapsed_ms(started),
        })),
    )
}

/// 打开阶段的错误分类：加密文档给出可操作提示，其余带上底层原因
fn open_error(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if lower.contains("encrypt") || lower.contains("password") {
        "文档已加密，暂不支持".to_string()
    } else {
        format!("PDF 打开失败: {}", message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手工构造单页 PDF（xref 偏移精确计算），复用 `tests/pdf_tests.rs` 的构造方式
    fn create_text_pdf(text: &str) -> Vec<u8> {
        let header = b"%PDF-1.4\n";
        let obj1 = b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n";
        let obj2 = b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n";
        let obj3 = b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n";
        let stream_data = format!("BT /F1 12 Tf 100 700 Td ({}) Tj ET\n", text);
        let obj4 = format!(
            "4 0 obj\n<< /Length {} >>\nstream\n{}\nendstream\nendobj\n",
            stream_data.len(),
            stream_data
        );
        let obj5 = b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n";

        let mut parts: Vec<Vec<u8>> = Vec::new();
        let mut offsets = Vec::new();
        parts.push(header.to_vec());
        for obj in [
            obj1.to_vec(),
            obj2.to_vec(),
            obj3.to_vec(),
            obj4.into_bytes(),
            obj5.to_vec(),
        ] {
            offsets.push(parts.iter().map(|p| p.len()).sum::<usize>());
            parts.push(obj);
        }
        let offset_xref = parts.iter().map(|p| p.len()).sum::<usize>();
        let num_objects = 6usize;
        let xref_entries: String = offsets
            .iter()
            .map(|o| format!("{:010} 00000 n \n", o))
            .collect();
        parts.push(
            format!(
                "xref\n0 {}\n0000000000 65535 f \n{}",
                num_objects, xref_entries
            )
            .into_bytes(),
        );
        parts.push(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                num_objects, offset_xref
            )
            .into_bytes(),
        );
        parts.into_iter().flatten().collect()
    }

    fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, bytes).expect("❌ 写入临时 PDF 失败");
        path
    }

    #[test]
    fn test_parse_extracts_text_and_metadata() {
        let text = "Hello World, this fixture text is deliberately longer than fifty characters.";
        let path = write_temp("parser_pdf_text_fixture.pdf", &create_text_pdf(text));
        let result = parse(&path, &ParseOptions::default()).expect("❌ PDF 解析失败");
        let _ = std::fs::remove_file(&path);

        assert_eq!(result.file_type, "pdf");
        assert_eq!(result.file_name, "parser_pdf_text_fixture.pdf");
        assert!(result.images.is_empty(), "❌ Python 的 PDF 结果不带 images");
        let metadata = result.metadata.expect("❌ metadata 不应为空");
        assert_eq!(metadata["page_count"], 1);
        assert_eq!(metadata["source_type"], "text");
        assert!(
            result.raw_text.contains("Hello World"),
            "❌ 应提取到 PDF 文本，实际：{}",
            result.raw_text
        );
    }

    #[test]
    fn test_short_text_triggers_ocr_fallback() {
        let path = write_temp("parser_pdf_short_fixture.pdf", &create_text_pdf("Hi"));
        let result = parse(&path, &ParseOptions::default()).expect("❌ PDF 解析失败");
        let _ = std::fs::remove_file(&path);

        let metadata = result.metadata.expect("❌ metadata 不应为空");
        // 2 个字符 < 50 → 走 OCR 兜底；OCR 未启用 → 空文本 + source_type 回落 text
        assert_eq!(metadata["source_type"], "text");
        assert!(result.raw_text.is_empty(), "OCR 未启用时应降级为空文本");
    }

    #[test]
    fn test_open_error_maps_encrypted_document() {
        assert_eq!(
            open_error("Failed to decrypt: password required (encrypted)"),
            "文档已加密，暂不支持"
        );
        assert!(open_error("Invalid PDF header").starts_with("PDF 打开失败: "));
    }

    #[test]
    fn test_invalid_pdf_returns_error() {
        let path = write_temp("parser_pdf_broken_fixture.pdf", b"not a pdf at all");
        let err = parse(&path, &ParseOptions::default()).expect_err("❌ 非法 PDF 应报错");
        let _ = std::fs::remove_file(&path);
        assert!(err.starts_with("PDF 打开失败: "), "实际错误：{}", err);
    }
}
