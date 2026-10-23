//! Office 文档解析（Word / Excel → 纯文本）
//!
//! 对齐 `apps/doc-parser/parsers/docs_parser.py` 的输出形状：
//! - Word：正文段落逐行，**之后**追加所有表格行；每行单元格 `strip()` 后非空的用
//!   `" | "` 连接；
//! - Excel：每个工作表先输出 `【工作表: 名称】`，再逐行输出非空单元格，同样 `" | "` 连接；
//! - 解析失败不影响整体：记录 `metadata.error` 并返回空文本（Python 的"尽力解析"语义）。
//!
//! 与 Python 的差异（都是能力增强，不改输出形状）：
//! - Python 的 `python-docx` / `openpyxl` 无法读 `.doc` / `.xls`，历史上直接返回空文本；
//!   Rust 侧由 `office_oxide` 尽力解析，因此这两类文件从"空"变成"有内容"。

use std::path::Path;
use std::time::Instant;

use office_oxide::docx::paragraph::{Paragraph, ParagraphContent, Run, RunContent};
use office_oxide::docx::{BlockElement, DocxDocument};
use office_oxide::xls::XlsDocument;
use office_oxide::xlsx::{Cell, CellValue as XlsxCellValue, XlsxDocument};
use office_oxide::Document;
use serde_json::{Map, Value};
use tracing::warn;

use crate::{options::ParseOptions, text, types::ParseResult};

/// 解析 Word / Excel
pub(crate) fn parse(path: &Path, _options: &ParseOptions) -> Result<ParseResult, String> {
    let started = Instant::now();
    let ext = text::ext_of(path);

    let mut error: Option<String> = None;
    let raw_text = match Document::open(path) {
        Ok(doc) => match ext.as_str() {
            "docx" => doc.as_docx().map(word_text).unwrap_or_default(),
            "doc" => doc.as_doc().map(|d| d.plain_text()).unwrap_or_default(),
            "xlsx" => doc.as_xlsx().map(spreadsheet_text).unwrap_or_default(),
            "xls" => doc
                .as_xls()
                .map(legacy_spreadsheet_text)
                .unwrap_or_default(),
            _ => String::new(),
        },
        Err(e) => {
            // 与 Python 一致：单文件解析失败不阻断上层，交给 metadata 记录
            warn!("Office 文档解析失败: file={}, err={}", path.display(), e);
            error = Some(e.to_string());
            String::new()
        }
    };

    let mut metadata = Map::new();
    metadata.insert("sub_type".to_string(), Value::String(ext));
    metadata.insert(
        "parse_duration_ms".to_string(),
        Value::from(text::elapsed_ms(started)),
    );
    if let Some(error) = error {
        metadata.insert("error".to_string(), Value::String(error));
    }

    Ok(ParseResult::new(text::basename(path), "document", raw_text)
        .with_metadata(Value::Object(metadata)))
}

// ======================== Word ========================

/// DOCX → 纯文本：先正文段落，再所有正文级表格的行
fn word_text(doc: &DocxDocument) -> String {
    let mut lines: Vec<String> = Vec::new();
    for block in &doc.body.elements {
        if let BlockElement::Paragraph(p) = block {
            let t = paragraph_text(p);
            // python-docx：`if p.text.strip()`，保留原文（含首尾空白）
            if !t.trim().is_empty() {
                lines.push(t);
            }
        }
    }

    // python-docx 的 `doc.tables` 不含单元格内的嵌套表格，且在所有段落之后追加
    let mut table_lines: Vec<String> = Vec::new();
    for block in &doc.body.elements {
        if let BlockElement::Table(table) = block {
            for row in &table.rows {
                let cells: Vec<String> = row
                    .cells
                    .iter()
                    .map(|cell| cell_text(&cell.content).trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !cells.is_empty() {
                    table_lines.push(cells.join(" | "));
                }
            }
        }
    }
    lines.extend(table_lines);
    lines.join("\n")
}

/// 单元格文本：等价于 python-docx 的 `cell.text`（只取段落，段落间 `\n`）
fn cell_text(content: &[BlockElement]) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    for block in content {
        if let BlockElement::Paragraph(p) = block {
            paragraphs.push(paragraph_text(p));
        }
    }
    paragraphs.join("\n")
}

/// 段落文本：等价于 python-docx 的 `paragraph.text`（含超链接可见文本）
fn paragraph_text(paragraph: &Paragraph) -> String {
    let mut out = String::new();
    for content in &paragraph.content {
        match content {
            ParagraphContent::Run(run) => push_run_text(run, &mut out),
            ParagraphContent::Hyperlink(link) => {
                for run in &link.runs {
                    push_run_text(run, &mut out);
                }
            }
        }
    }
    out
}

/// run 文本：`w:t` 直出、`w:br` 记 `\n`、`w:tab` 记 `\t`、文本框递归
fn push_run_text(run: &Run, out: &mut String) {
    for content in &run.content {
        match content {
            RunContent::Text(t) => out.push_str(t),
            RunContent::Break(_) => out.push('\n'),
            RunContent::Tab => out.push('\t'),
            RunContent::TextBox(blocks) => {
                for block in blocks {
                    if let BlockElement::Paragraph(p) = block {
                        let t = paragraph_text(p);
                        if t.is_empty() {
                            continue;
                        }
                        if !out.is_empty() && !out.ends_with('\n') {
                            out.push('\n');
                        }
                        out.push_str(&t);
                    }
                }
            }
            // 图片 / 脚注引用 / 域标记等不产生正文文字
            _ => {}
        }
    }
}

// ======================== Excel ========================

/// XLSX → 纯文本：`【工作表: x】` + 每行非空单元格 `" | "` 连接
fn spreadsheet_text(doc: &XlsxDocument) -> String {
    let mut lines: Vec<String> = Vec::new();
    for sheet in &doc.worksheets {
        lines.push(format!("【工作表: {}】", sheet.name));
        for row in &sheet.rows {
            let mut cells: Vec<String> = Vec::new();
            for cell in &row.cells {
                // openpyxl `values_only=True` 的 None → 跳过；空串保留
                if matches!(&cell.value, XlsxCellValue::Empty) {
                    continue;
                }
                cells.push(cell_display(doc, cell));
            }
            if !cells.is_empty() {
                lines.push(cells.join(" | "));
            }
        }
    }
    lines.join("\n")
}

/// 单元格显示文本（等价于 openpyxl 返回的 Python 对象 `str()`）
fn cell_display(doc: &XlsxDocument, cell: &Cell) -> String {
    match &cell.value {
        XlsxCellValue::Empty => String::new(),
        XlsxCellValue::Number(n) => text::num_to_text(*n),
        XlsxCellValue::String(s) => s.clone(),
        XlsxCellValue::SharedString(idx) => {
            doc.shared_strings.get(*idx).unwrap_or_default().to_string()
        }
        XlsxCellValue::Boolean(b) => text::bool_to_text(*b).to_string(),
        XlsxCellValue::Error(e) => e.clone(),
        XlsxCellValue::Date(dt) => dt.to_string(),
    }
}

/// XLS → 纯文本（Python 无对应实现，此处按同样的 `【工作表: x】` 形状输出）
fn legacy_spreadsheet_text(doc: &XlsDocument) -> String {
    let mut lines: Vec<String> = Vec::new();
    for sheet in &doc.sheets {
        lines.push(format!("【工作表: {}】", sheet.name));
        for row in &sheet.rows {
            let cells: Vec<String> = row
                .iter()
                .filter(|v| !matches!(v, office_oxide::xls::CellValue::Empty))
                .map(|v| v.as_text())
                .collect();
            if !cells.is_empty() {
                lines.push(cells.join(" | "));
            }
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_corrupt_document_degrades_to_empty_text() {
        let path = std::env::temp_dir().join("parser_corrupt_fixture.xlsx");
        std::fs::write(&path, b"this is definitely not a zip package")
            .expect("❌ 写入临时文件失败");
        let result = parse(&path, &ParseOptions::default()).expect("❌ 解析失败不应向上报错");
        let _ = std::fs::remove_file(&path);

        assert_eq!(result.file_type, "document");
        assert!(result.raw_text.is_empty(), "❌ 坏文件应降级为空文本");
        let metadata = result.metadata.expect("❌ metadata 不应为空");
        assert_eq!(metadata["sub_type"], "xlsx");
        assert!(
            metadata["error"].as_str().is_some_and(|e| !e.is_empty()),
            "❌ 应记录解析错误，实际 metadata：{}",
            metadata
        );
    }

    #[test]
    fn test_sheet_header_format_matches_python() {
        // 表头格式是下游（对话 / 知识库）依赖的固定串，单独锁死
        let header = format!("【工作表: {}】", "销售数据");
        assert_eq!(header, "【工作表: 销售数据】");
    }
}
