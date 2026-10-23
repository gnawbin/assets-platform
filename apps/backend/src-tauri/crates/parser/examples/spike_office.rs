//! Phase 0 spike ①：`office_oxide` 对 docx / xlsx / doc / xls 的输出质量
//!
//! 用法：
//! ```text
//! cargo run -p parser --example spike_office
//! cargo run -p parser --example spike_office -- 样本.doc 样本.xls 样本.docx
//! ```
//!
//! 无参数时跑**自检**：内存构造最小 docx / xlsx 夹具并解析，验证「解析 → 字符串」
//! 全链路与 `ParseResult` 形状；真实 `.doc`/`.xls` 保真度需自备二进制样本（仓库内
//! 没有版本化的 Office 样本）。
//!
//! 退出码：`0` 全部成功；`1` 有文件解析失败。

use std::path::PathBuf;

use parser::{parse_file_blocking, ParseOptions};

fn main() {
    let args: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if args.is_empty() {
        selftest();
        return;
    }

    let mut failures = 0usize;
    for path in &args {
        if !path.is_file() {
            eprintln!("❌ 文件不存在: {}", path.display());
            failures += 1;
            continue;
        }
        match parse_file_blocking(path, &ParseOptions::default()) {
            Ok(result) => {
                let metadata = result
                    .metadata
                    .as_ref()
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| "null".to_string());
                println!("===== {}", path.display());
                println!("file_type     : {}", result.file_type);
                println!("file_name     : {}", result.file_name);
                println!("chars         : {}", result.raw_text.chars().count());
                println!("images        : {}", result.images.len());
                println!("metadata      : {}", metadata);
                println!("--- raw_text (前 600 字符) ---");
                let preview: String = result.raw_text.chars().take(600).collect();
                println!("{}", preview);
                if result.raw_text.is_empty() {
                    println!("⚠️ 文本为空：可能是不支持的子格式或样本损坏");
                }
                println!();
            }
            Err(e) => {
                eprintln!("❌ 解析失败 {}: {}", path.display(), e);
                failures += 1;
            }
        }
    }

    if failures > 0 {
        eprintln!("❌ {} 个文件失败", failures);
        std::process::exit(1);
    }
    println!("✅ office spike 通过：{} 个文件", args.len());
}

/// 自检：内存拼装 docx + xlsx，逐项打印解析结果（严格断言由 `tests/office_tests.rs` 负责）。
fn selftest() {
    let cases: [(&str, Vec<u8>); 2] = [
        ("spike_selftest.docx", build_docx()),
        ("spike_selftest.xlsx", build_xlsx()),
    ];

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/spike_out/office");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("❌ 创建输出目录失败: {}", e);
        std::process::exit(1);
    }

    let mut failures = 0usize;
    for (name, bytes) in cases {
        let path = dir.join(name);
        if let Err(e) = std::fs::write(&path, &bytes) {
            eprintln!("❌ 写入夹具失败 {}: {}", path.display(), e);
            failures += 1;
            continue;
        }
        match parse_file_blocking(&path, &ParseOptions::default()) {
            Ok(result) => {
                println!("===== {}（{} 字节）", name, bytes.len());
                println!("file_type : {}", result.file_type);
                println!("raw_text  : {:?}", result.raw_text);
                println!("metadata  : {:?}", result.metadata);
                println!();
            }
            Err(e) => {
                eprintln!("❌ 自检解析失败 {}: {}", name, e);
                failures += 1;
            }
        }
    }

    if failures > 0 {
        std::process::exit(1);
    }
    println!("✅ office 自检通过（OOXML 链路）；真实 .doc/.xls 请按用法传入样本复核");
}

// ======================== 自检夹具（与 tests/office_tests.rs 同源） ========================

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn build_zip(files: &[(&str, &str)]) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, content) in files {
            writer
                .start_file(*name, options)
                .unwrap_or_else(|e| panic!("❌ 写入 zip 条目失败 {}: {}", name, e));
            writer
                .write_all(content.as_bytes())
                .unwrap_or_else(|e| panic!("❌ 写入 zip 内容失败 {}: {}", name, e));
        }
        writer.finish().expect("❌ 完成 zip 失败");
    }
    cursor.into_inner()
}

const BASE_CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
</Types>"#;

const DOCX_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

const DOCX_DOCUMENT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>第一段</w:t></w:r></w:p>
    <w:p><w:r><w:t>第二段</w:t></w:r></w:p>
    <w:tbl>
      <w:tr>
        <w:tc><w:p><w:r><w:t>名称</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>数量</w:t></w:r></w:p></w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:p><w:r><w:t>苹果</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>3</w:t></w:r></w:p></w:tc>
      </w:tr>
    </w:tbl>
  </w:body>
</w:document>"#;

fn build_docx() -> Vec<u8> {
    let content_types = BASE_CONTENT_TYPES.replace(
        "</Types>",
        r#"  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
    );
    build_zip(&[
        ("[Content_Types].xml", &content_types),
        ("_rels/.rels", DOCX_RELS),
        ("word/document.xml", DOCX_DOCUMENT),
    ])
}

const XLSX_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#;

const XLSX_WORKBOOK: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
    <sheet name="销售数据" sheetId="1" r:id="rId1"/>
  </sheets>
</workbook>"#;

const XLSX_WORKBOOK_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>
</Relationships>"#;

const XLSX_SHEET: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>
    <row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>3</v></c></row>
    <row r="3"><c r="A3" t="b"><v>1</v></c></row>
  </sheetData>
</worksheet>"#;

const XLSX_SHARED_STRINGS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="3" uniqueCount="3">
  <si><t>名称</t></si>
  <si><t>数量</t></si>
  <si><t>苹果</t></si>
</sst>"#;

fn build_xlsx() -> Vec<u8> {
    let content_types = BASE_CONTENT_TYPES.replace(
        "</Types>",
        r#"  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>
</Types>"#,
    );
    build_zip(&[
        ("[Content_Types].xml", &content_types),
        ("_rels/.rels", XLSX_RELS),
        ("xl/workbook.xml", XLSX_WORKBOOK),
        ("xl/_rels/workbook.xml.rels", XLSX_WORKBOOK_RELS),
        ("xl/worksheets/sheet1.xml", XLSX_SHEET),
        ("xl/sharedStrings.xml", XLSX_SHARED_STRINGS),
    ])
}
