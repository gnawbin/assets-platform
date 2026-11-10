//! 集成测试：Office 解析（内存构造最小 OOXML 夹具）
//!
//! 夹具在测试内用 `zip` 现场拼装，不依赖外部二进制文件；断言的输出形状与
//! `apps/doc-parser/parsers/docs_parser.py` 完全一致：
//! Word 先正文段落、后表格行；Excel 每个工作表先 `【工作表: x】`；单元格 `" | "` 连接。

use std::io::{Cursor, Write};

use parser::{parse_file_blocking, ParseOptions};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

// ======================== 夹具构造 ========================

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

const CONTENT_TYPES_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
</Types>"#;

fn docx_rels() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#
        .to_string()
}

fn docx_document() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
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
      <w:tr>
        <w:tc><w:p></w:p></w:tc>
        <w:tc><w:p></w:p></w:tc>
      </w:tr>
    </w:tbl>
    <w:p><w:r><w:t>表后段落</w:t></w:r></w:p>
  </w:body>
</w:document>"#
        .to_string()
}

fn docx_content_types() -> String {
    CONTENT_TYPES_RELS.replace(
        "</Types>",
        r#"  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
    )
}

fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, bytes).unwrap_or_else(|e| panic!("❌ 写入临时文件失败 {}: {}", name, e));
    path
}

// ======================== Word ========================

#[test]
fn test_docx_paragraphs_then_table_rows() {
    let bytes = build_zip(&[
        ("[Content_Types].xml", &docx_content_types()),
        ("_rels/.rels", &docx_rels()),
        ("word/document.xml", &docx_document()),
    ]);
    let path = write_temp("parser_docx_fixture.docx", &bytes);

    let result = parse_file_blocking(&path, &ParseOptions::default());
    let _ = std::fs::remove_file(&path);
    let result = result.expect("❌ DOCX 解析失败");

    assert_eq!(result.file_type, "document");
    assert_eq!(result.file_name, "parser_docx_fixture.docx");
    assert!(
        result.images.is_empty(),
        "❌ Office 文档无需 VLM 描述，images 应为空"
    );
    assert_eq!(
        result.raw_text, "第一段\n第二段\n表后段落\n名称 | 数量\n苹果 | 3",
        "❌ 输出顺序应为：正文段落 → 表格行（与 docs_parser.py 一致）"
    );
    let metadata = result.metadata.expect("❌ metadata 不应为空");
    assert_eq!(metadata["sub_type"], "docx");
    assert!(metadata["parse_duration_ms"].as_u64().is_some());
    assert!(metadata.get("error").is_none(), "❌ 正常文档不应带 error");
}

// ======================== Excel ========================

fn xlsx_content_types() -> String {
    CONTENT_TYPES_RELS.replace(
        "</Types>",
        r#"  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>
</Types>"#,
    )
}

fn xlsx_rels() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#
        .to_string()
}

fn xlsx_workbook() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
    <sheet name="销售数据" sheetId="1" r:id="rId1"/>
  </sheets>
</workbook>"#
        .to_string()
}

fn xlsx_workbook_rels() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>
</Relationships>"#
        .to_string()
}

fn xlsx_sheet() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1">
      <c r="A1" t="s"><v>0</v></c>
      <c r="B1" t="s"><v>1</v></c>
    </row>
    <row r="2">
      <c r="A2" t="s"><v>2</v></c>
      <c r="B2"><v>3</v></c>
    </row>
    <row r="3">
      <c r="A3" t="b"><v>1</v></c>
    </row>
  </sheetData>
</worksheet>"#
        .to_string()
}

fn xlsx_shared_strings() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="3" uniqueCount="3">
  <si><t>名称</t></si>
  <si><t>数量</t></si>
  <si><t>苹果</t></si>
</sst>"#
        .to_string()
}

#[test]
fn test_xlsx_sheet_header_and_row_join() {
    let bytes = build_zip(&[
        ("[Content_Types].xml", &xlsx_content_types()),
        ("_rels/.rels", &xlsx_rels()),
        ("xl/workbook.xml", &xlsx_workbook()),
        ("xl/_rels/workbook.xml.rels", &xlsx_workbook_rels()),
        ("xl/worksheets/sheet1.xml", &xlsx_sheet()),
        ("xl/sharedStrings.xml", &xlsx_shared_strings()),
    ]);
    let path = write_temp("parser_xlsx_fixture.xlsx", &bytes);

    let result = parse_file_blocking(&path, &ParseOptions::default());
    let _ = std::fs::remove_file(&path);
    let result = result.expect("❌ XLSX 解析失败");

    assert_eq!(result.file_type, "document");
    assert_eq!(
        result.raw_text, "【工作表: 销售数据】\n名称 | 数量\n苹果 | 3\nTrue",
        "❌ 工作表分组 / 单元格拼接 / 布尔写法应与 docs_parser.py 一致；实际：{}",
        result.raw_text
    );
    let metadata = result.metadata.expect("❌ metadata 不应为空");
    assert_eq!(metadata["sub_type"], "xlsx");
}
