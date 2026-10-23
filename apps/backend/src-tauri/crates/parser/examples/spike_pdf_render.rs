//! Phase 0 spike ②-a：`pdf_oxide` 的 `rendering` 特性（扫描件/OCR 路径的地基）
//!
//! 要回答的两个问题：
//! 1. 纯 Rust 能否把 PDF 页面稳定渲染成位图（不依赖 poppler / MuPDF 等外部二进制）？
//! 2. 72 / 150 / 300 DPI 下的单页耗时与显存量级，能否支撑「每页 300 DPI 喂 OCR」？
//!
//! 运行：
//! ```text
//! cargo run -p parser --features render --example spike_pdf_render
//! cargo run -p parser --features render --example spike_pdf_render -- 样本.pdf 72 150 300
//! ```
//!
//! 退出码：`0` 跑通；`1` 渲染失败；`2` 未启用 `render` feature（跳过）。

#[cfg(feature = "render")]
fn main() {
    imp::run();
}

#[cfg(not(feature = "render"))]
fn main() {
    eprintln!("⏭️ 跳过：未启用 render feature");
    eprintln!("   cargo run -p parser --features render --example spike_pdf_render");
    std::process::exit(2);
}

#[cfg(feature = "render")]
mod imp {
    use std::path::PathBuf;
    use std::time::Instant;

    use pdf_oxide::rendering::{render_page, ImageFormat, RenderOptions};
    use pdf_oxide::PdfDocument;

    pub fn run() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut input: Option<PathBuf> = None;
        let mut dpis: Vec<u32> = Vec::new();
        for arg in &args {
            match arg.parse::<u32>() {
                Ok(dpi) => dpis.push(dpi),
                Err(_) => input = Some(PathBuf::from(arg)),
            }
        }
        if dpis.is_empty() {
            dpis = vec![72, 150, 300];
        }

        let (doc, label) = match &input {
            Some(path) => {
                if !path.is_file() {
                    eprintln!("❌ 文件不存在: {}", path.display());
                    std::process::exit(1);
                }
                match PdfDocument::open(path) {
                    Ok(doc) => (doc, path.display().to_string()),
                    Err(e) => {
                        eprintln!("❌ 打开失败 {}: {}", path.display(), e);
                        std::process::exit(1);
                    }
                }
            }
            None => match PdfDocument::from_bytes(build_demo_pdf()) {
                Ok(doc) => (
                    doc,
                    "内置 2 页演示 PDF（A4 595x842pt，Helvetica 无内嵌字体）".to_string(),
                ),
                Err(e) => {
                    eprintln!("❌ 演示 PDF 构造失败: {}", e);
                    std::process::exit(1);
                }
            },
        };

        let page_count = match doc.page_count() {
            Ok(n) => n,
            Err(e) => {
                eprintln!("❌ page_count 失败: {}", e);
                std::process::exit(1);
            }
        };

        let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/spike_out/render");
        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            eprintln!("❌ 创建输出目录失败: {}", e);
            std::process::exit(1);
        }

        println!("📄 输入：{}", label);
        println!("📄 页数：{}", page_count);
        println!("📁 输出：{}", out_dir.display());
        println!();
        println!("| DPI | 页 | 尺寸(px) | RAW 缓冲(MB) | 墨迹占比 | PNG(KB) | 耗时(ms) |");
        println!("|-----|---:|----------|-------------:|---------:|--------:|---------:|");

        let mut failures = 0usize;
        for dpi in dpis {
            for page in 0..page_count {
                let mut raw_opts = RenderOptions::with_dpi(dpi);
                raw_opts.format = ImageFormat::RawRgba8;
                let started = Instant::now();
                let raw = match render_page(&doc, page, &raw_opts) {
                    Ok(img) => img,
                    Err(e) => {
                        eprintln!("❌ 渲染失败 page={} dpi={}: {}", page, dpi, e);
                        failures += 1;
                        continue;
                    }
                };
                let raw_ms = started.elapsed().as_millis();
                let ink = ink_ratio(&raw.data);
                let buffer_mb = raw.data.len() as f64 / 1024.0 / 1024.0;

                let mut png_opts = RenderOptions::with_dpi(dpi);
                png_opts.format = ImageFormat::Png;
                let started = Instant::now();
                match render_page(&doc, page, &png_opts) {
                    Ok(png) => {
                        let path = out_dir.join(format!("page{}_{}dpi.png", page + 1, dpi));
                        if let Err(e) = png.save(&path) {
                            eprintln!("⚠️ 保存失败 {}: {}", path.display(), e);
                        }
                        println!(
                            "| {} | {} | {}x{} | {:.1} | {:.2}% | {} | {} |",
                            dpi,
                            page + 1,
                            raw.width,
                            raw.height,
                            buffer_mb,
                            ink * 100.0,
                            png.data.len() / 1024,
                            raw_ms + started.elapsed().as_millis()
                        );
                    }
                    Err(e) => {
                        eprintln!("❌ PNG 渲染失败 page={} dpi={}: {}", page, dpi, e);
                        failures += 1;
                    }
                }

                if ink < 0.001 {
                    println!(
                        "⚠️ page {} @{}dpi 墨迹占比 {:.4}% —— 可能是空白页或字体缺失（文本层未渲染）",
                        page + 1,
                        dpi,
                        ink * 100.0
                    );
                }
            }
        }

        println!();
        if failures > 0 {
            eprintln!("❌ 共 {} 次渲染失败", failures);
            std::process::exit(1);
        }
        println!("✅ 渲染 spike 通过：纯 Rust 位图管线可用，无外部二进制依赖");
        println!("   判据：A4@300dpi 单页 RAW ≈ 34.8 MB（2480x3508 RGBA），需按页串行以控内存");
    }

    /// 墨迹占比：非白且非全透明像素的比例，用于识别「渲染成功但内容空白」。
    fn ink_ratio(rgba: &[u8]) -> f32 {
        if rgba.is_empty() {
            return 0.0;
        }
        let mut ink = 0usize;
        for px in rgba.chunks_exact(4) {
            if px[3] > 0 && (px[0] < 245 || px[1] < 245 || px[2] < 245) {
                ink += 1;
            }
        }
        ink as f32 / (rgba.len() / 4) as f32
    }

    /// 现场拼装一个 2 页 A4 PDF（只用 base14 字体，任何环境都能打开）。
    fn build_demo_pdf() -> Vec<u8> {
        let contents = [
            "BT /F1 24 Tf 72 760 Td (Phase 0 spike: pdf_oxide rendering) Tj ET\nBT /F1 14 Tf 72 720 Td (page 1 of 2 - vector text, no embedded font) Tj ET",
            "BT /F1 24 Tf 72 760 Td (page 2 of 2) Tj ET",
        ];
        let objects: Vec<String> = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 7 0 R >> >> /Contents 4 0 R >>".to_string(),
            format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                contents[0].len(),
                contents[0]
            ),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 7 0 R >> >> /Contents 6 0 R >>".to_string(),
            format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                contents[1].len(),
                contents[1]
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
