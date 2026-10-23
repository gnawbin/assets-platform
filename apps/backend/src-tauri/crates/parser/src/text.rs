//! 文本层小工具
//!
//! 集中放"与 Python 输出逐字符对齐"的格式化逻辑：扩展名、文件名、数字 / 布尔
//! 转文本。所有模态的解析器都复用这里，避免各写一套导致输出漂移。

use std::path::Path;
use std::time::Instant;

/// 取小写扩展名（不含点）
///
/// 与 `parse_service.py` 的 `_get_file_ext` 一致：无扩展名时返回空串。
pub fn ext_of(path: impl AsRef<Path>) -> String {
    path.as_ref()
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// 取文件名
///
/// 等价于 Python 的 `file_path.rsplit("/", 1)[-1]`，但按当前平台分隔符处理。
pub fn basename(path: impl AsRef<Path>) -> String {
    path.as_ref()
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string()
}

/// 数值 → 文本（对齐 Python `str(float)` / openpyxl 的整型折叠）
///
/// - 整数值（含 `2.0`）输出 `2`，与 openpyxl `values_only=True` 返回 int 的写法一致
/// - 其余用 Rust 的最短往返表示，与 Python 的 `repr` 一致（如 `0.30000000000000004`）
/// - 溢出 / 非有限值的表示与 Python 略有差异（`1e+20` vs `100000000000000000000`），
///   单元格里几乎不会出现，已在设计文档 §5.2 记录
pub fn num_to_text(n: f64) -> String {
    if n.is_nan() {
        return "nan".to_string();
    }
    if n.is_infinite() {
        return if n.is_sign_positive() { "inf" } else { "-inf" }.to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e16 {
        return format!("{}", n as i64);
    }
    format!("{}", n)
}

/// 布尔 → 文本（Python 的 `True` / `False`，不是 Rust 的 `true`）
pub fn bool_to_text(b: bool) -> &'static str {
    if b {
        "True"
    } else {
        "False"
    }
}

/// 从某个起点到现在的毫秒数，用于 `metadata.parse_duration_ms`
pub fn elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_ext_of_lowercases_and_strips_dot() {
        assert_eq!(ext_of("a/b/Report.PDF"), "pdf");
        assert_eq!(ext_of(PathBuf::from("x.docx")), "docx");
        assert_eq!(ext_of("noext"), "", "❌ 无扩展名应为空串");
        assert_eq!(ext_of("dir.name/file"), "", "❌ 目录点号不应算扩展名");
    }

    #[test]
    fn test_basename_matches_python_rsplit() {
        assert_eq!(basename("C:/tmp/测试 文档.docx"), "测试 文档.docx");
        assert_eq!(basename("plain.pdf"), "plain.pdf");
    }

    #[test]
    fn test_num_to_text_folds_integral_floats() {
        assert_eq!(num_to_text(2.0), "2");
        assert_eq!(num_to_text(-17.0), "-17");
        assert_eq!(num_to_text(1.5), "1.5");
        assert_eq!(num_to_text(0.30000000000000004), "0.30000000000000004");
    }

    #[test]
    fn test_num_to_text_handles_non_finite() {
        assert_eq!(num_to_text(f64::NAN), "nan");
        assert_eq!(num_to_text(f64::INFINITY), "inf");
        assert_eq!(num_to_text(f64::NEG_INFINITY), "-inf");
    }

    #[test]
    fn test_bool_to_text_uses_python_spelling() {
        assert_eq!(bool_to_text(true), "True");
        assert_eq!(bool_to_text(false), "False");
    }
}
