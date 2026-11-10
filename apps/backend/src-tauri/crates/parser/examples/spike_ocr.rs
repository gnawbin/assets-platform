//! Phase 0 spike ②-b：`pdf_oxide` 内置 PP-OCR 的识别效果
//!
//! 与 `spike_pdf_render` 配套：**渲染**负责页面 → 位图，**OCR** 负责位图 → 文本。
//! 注意 `pdf_oxide::ocr::ocr_page()` 只 OCR 页面里「已内嵌的图片」，对矢量页面无效，
//! 所以本 spike 走 `render_page → DynamicImage → OcrEngine::ocr_image` 的正确链路。
//!
//! 前置（模型不随 crate 分发，需自备 PP-OCR ONNX + 字典）：
//! ```text
//! set PPOCR_DET_MODEL=D:\models\ch_PP-OCRv4_det_infer.onnx
//! set PPOCR_REC_MODEL=D:\models\ch_PP-OCRv4_rec_infer.onnx
//! set PPOCR_DICT=D:\models\ppocr_keys_v1.txt
//! cargo run -p parser --features "render ocr" --example spike_ocr
//! cargo run -p parser --features "render ocr" --example spike_ocr -- --image 截图.png
//! cargo run -p parser --features "render ocr" --example spike_ocr -- 扫描件.pdf 300
//! ```
//!
//! 退出码：`0` 跑通；`1` 失败；`2` 缺少模型或未启用 feature（跳过）。

#[cfg(all(feature = "render", feature = "ocr"))]
fn main() {
    imp::run();
}

#[cfg(not(all(feature = "render", feature = "ocr")))]
fn main() {
    eprintln!("⏭️ 跳过：需要同时启用 render + ocr feature");
    eprintln!("   cargo run -p parser --features \"render ocr\" --example spike_ocr");
    std::process::exit(2);
}

#[cfg(all(feature = "render", feature = "ocr"))]
mod imp {
    use std::path::PathBuf;
    use std::time::Instant;

    use pdf_oxide::ocr::{OcrConfig, OcrEngine};
    use pdf_oxide::rendering::{render_page, ImageFormat, RenderOptions};
    use pdf_oxide::PdfDocument;

    pub fn run() {
        let det = std::env::var("PPOCR_DET_MODEL").ok();
        let rec = std::env::var("PPOCR_REC_MODEL").ok();
        let dict = std::env::var("PPOCR_DICT").ok();
        let (Some(det), Some(rec), Some(dict)) = (det, rec, dict) else {
            eprintln!("⏭️ 跳过：未配置 PP-OCR 模型路径");
            eprintln!("   set PPOCR_DET_MODEL=...\\ch_PP-OCRv4_det_infer.onnx");
            eprintln!("   set PPOCR_REC_MODEL=...\\ch_PP-OCRv4_rec_infer.onnx");
            eprintln!("   set PPOCR_DICT=...\\ppocr_keys_v1.txt");
            std::process::exit(2);
        };

        let started = Instant::now();
        let engine = match OcrEngine::new(&det, &rec, &dict, OcrConfig::default()) {
            Ok(engine) => engine,
            Err(e) => {
                eprintln!("❌ OcrEngine 初始化失败: {:?}", e);
                std::process::exit(1);
            }
        };
        println!(
            "🔧 OcrEngine 就绪（{} ms，后端 = tract 纯 Rust）",
            started.elapsed().as_millis()
        );

        let args: Vec<String> = std::env::args().skip(1).collect();
        let image = match args.first().map(String::as_str) {
            Some("--image") => {
                let path = args.get(1).cloned().map(PathBuf::from).unwrap_or_default();
                match image::open(&path) {
                    Ok(img) => {
                        println!(
                            "🖼️ 输入图片：{}（{}x{}）",
                            path.display(),
                            img.width(),
                            img.height()
                        );
                        img
                    }
                    Err(e) => {
                        eprintln!("❌ 读取图片失败 {}: {}", path.display(), e);
                        std::process::exit(1);
                    }
                }
            }
            Some(path) => {
                let path = PathBuf::from(path);
                let dpi: u32 = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(200);
                let png = match render_pdf(&path, dpi) {
                    Ok(bytes) => bytes,
                    Err(code) => std::process::exit(code),
                };
                match image::load_from_memory(&png) {
                    Ok(img) => {
                        println!(
                            "🖼️ 渲染 {} @{}dpi（{}x{}）",
                            path.display(),
                            dpi,
                            img.width(),
                            img.height()
                        );
                        img
                    }
                    Err(e) => {
                        eprintln!("❌ PNG 解码失败: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            None => {
                let png = match render_pdf_bytes(build_demo_pdf(), 200) {
                    Ok(bytes) => bytes,
                    Err(code) => std::process::exit(code),
                };
                match image::load_from_memory(&png) {
                    Ok(img) => {
                        println!("🖼️ 内置演示 PDF @200dpi（{}x{}）", img.width(), img.height());
                        img
                    }
                    Err(e) => {
                        eprintln!("❌ PNG 解码失败: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        };

        let started = Instant::now();
        let output = match engine.ocr_image(&image) {
            Ok(output) => output,
            Err(e) => {
                eprintln!("❌ OCR 失败: {:?}", e);
                std::process::exit(1);
            }
        };
        let ms = started.elapsed().as_millis();

        println!();
        println!("| 指标 | 值 |");
        println!("|------|---:|");
        println!("| 文本框数 | {} |", output.spans.len());
        println!("| 平均置信度 | {:.3} |", output.total_confidence);
        println!("| 耗时(ms) | {} |", ms);
        println!();
        for span in output.spans.iter().take(10) {
            println!("  [{:.3}] {}", span.confidence, span.text);
        }
        if output.spans.len() > 10 {
            println!("  ...（另有 {} 个文本框）", output.spans.len() - 10);
        }
        println!();
        println!("--- reading order ---");
        println!("{}", output.text_in_reading_order());
        println!();
        if output.spans.is_empty() {
            eprintln!("⚠️ 未识别到文本：检查清晰度 / det_threshold / 模型是否为中文模型");
        } else {
            println!("✅ OCR spike 跑通：渲染 → PP-OCR → 阅读顺序文本");
        }
    }

    fn render_pdf(path: &std::path::Path, dpi: u32) -> Result<Vec<u8>, i32> {
        if !path.is_file() {
            eprintln!("❌ 文件不存在: {}", path.display());
            return Err(1);
        }
        let doc = match PdfDocument::open(path) {
            Ok(doc) => doc,
            Err(e) => {
                eprintln!("❌ 打开失败 {}: {}", path.display(), e);
                return Err(1);
            }
        };
        render_doc(&doc, dpi)
    }

    fn render_pdf_bytes(bytes: Vec<u8>, dpi: u32) -> Result<Vec<u8>, i32> {
        match PdfDocument::from_bytes(bytes) {
            Ok(doc) => render_doc(&doc, dpi),
            Err(e) => {
                eprintln!("❌ 演示 PDF 构造失败: {}", e);
                Err(1)
            }
        }
    }

    fn render_doc(doc: &PdfDocument, dpi: u32) -> Result<Vec<u8>, i32> {
        let mut options = RenderOptions::with_dpi(dpi);
        options.format = ImageFormat::Png;
        match render_page(doc, 0, &options) {
            Ok(img) => Ok(img.data),
            Err(e) => {
                eprintln!("❌ 渲染失败: {}", e);
                Err(1)
            }
        }
    }

    /// 单页演示 PDF：一行英文，用于在无真实样本时验证「渲染 → OCR」管线是否通。
    fn build_demo_pdf() -> Vec<u8> {
        let content = "BT /F1 28 Tf 72 700 Td (Phase 0 OCR spike 2026) Tj ET";
        let objects: Vec<String> = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
            format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                content.len(),
                content
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut pdf = String::from("%PDF-1.4\n");
        let mut offsets = Vec::with_capacity(objects.len());
        for (index, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.push_str(&format!("{} 0 obj\n{}\nendobj\n", index + 1, body));
        }
        let xref_start = pdf.len();
        pdf.push_str(&format!("xref\n0 {}\n", objects.len() + 1));
        pdf.push_str("0000000000 65535 f \n");
        for offset in &offsets {
            pdf.push_str(&format!("{:010} 00000 n \n", offset));
        }
        pdf.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            objects.len() + 1,
            xref_start
        ));
        pdf.into_bytes()
    }
}
