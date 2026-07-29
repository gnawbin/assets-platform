//! 集成测试：image crate (v0.25) 图像处理核心功能
//!
//! 测试 image 库的基础功能，包括：
//! - 图像加载与保存（PNG/JPEG/WEBP）
//! - 图像格式转换
//! - 图像处理操作（调整大小、裁剪、旋转、灰度化）
//! - 像素级操作
//! - 边界情况处理
//!
//! 所有测试均在内存中生成测试图像，无需外部测试文件。

use image::{
    error::ImageError,
    imageops::FilterType,
    load_from_memory, load_from_memory_with_format,
    DynamicImage, GenericImage, GenericImageView, ImageBuffer, ImageFormat, Luma, Pixel, Rgb,
    RgbImage, Rgba, RgbaImage,
};

// ======================== 辅助函数 ========================

/// 创建一个 4x4 的 RGBA 测试图像（包含红、绿、蓝、白四色块各 2x2）
///
/// 布局：
/// ```text
/// R R G G
/// R R G G
/// B B W W
/// B B W W
/// ```
/// 其中 R=红色, G=绿色, B=蓝色, W=白色
fn create_test_rgba_image() -> RgbaImage {
    let mut img = RgbaImage::new(4, 4);

    // 红色块 (0,0) - (1,1)
    for y in 0..2 {
        for x in 0..2 {
            img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
        }
    }

    // 绿色块 (2,0) - (3,1)
    for y in 0..2 {
        for x in 2..4 {
            img.put_pixel(x, y, Rgba([0, 255, 0, 255]));
        }
    }

    // 蓝色块 (0,2) - (1,3)
    for y in 2..4 {
        for x in 0..2 {
            img.put_pixel(x, y, Rgba([0, 0, 255, 255]));
        }
    }

    // 白色块 (2,2) - (3,3)
    for y in 2..4 {
        for x in 2..4 {
            img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
        }
    }

    img
}

/// 创建一个 4x4 的 RGB 测试图像
fn create_test_rgb_image() -> RgbImage {
    let mut img = RgbImage::new(4, 4);

    // 红色块 (0,0) - (1,1)
    for y in 0..2 {
        for x in 0..2 {
            img.put_pixel(x, y, Rgb([255, 0, 0]));
        }
    }

    // 绿色块 (2,0) - (3,1)
    for y in 0..2 {
        for x in 2..4 {
            img.put_pixel(x, y, Rgb([0, 255, 0]));
        }
    }

    // 蓝色块 (0,2) - (1,3)
    for y in 2..4 {
        for x in 0..2 {
            img.put_pixel(x, y, Rgb([0, 0, 255]));
        }
    }

    // 白色块 (2,2) - (3,3)
    for y in 2..4 {
        for x in 2..4 {
            img.put_pixel(x, y, Rgb([255, 255, 255]));
        }
    }

    img
}

/// 将图像编码为字节缓冲区
fn encode_image(img: &DynamicImage, format: ImageFormat) -> Vec<u8> {
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), format)
        .expect("❌ 编码图像失败");
    buf
}

// ======================== 图像加载与保存测试 ========================

mod load_save_tests {
    use super::*;

    /// 测试 1：从字节数组加载 PNG 图像
    #[test]
    fn test_load_png_from_memory() {
        let rgba_img = create_test_rgba_image();
        let dyn_img = DynamicImage::ImageRgba8(rgba_img);
        let png_bytes = encode_image(&dyn_img, ImageFormat::Png);

        // 从字节加载 PNG
        let loaded = load_from_memory(&png_bytes).expect("❌ 无法从内存加载 PNG 图像");
        assert_eq!(loaded.width(), 4, "❌ PNG 图像宽度应为 4");
        assert_eq!(loaded.height(), 4, "❌ PNG 图像高度应为 4");
        assert_eq!(
            loaded.color(),
            image::ColorType::Rgba8,
            "❌ PNG 图像颜色类型应为 RGBA8"
        );

        println!("✅ PNG 图像从内存加载成功：{}x{}", loaded.width(), loaded.height());
    }

    /// 测试 2：从字节数组加载 JPEG 图像
    #[test]
    fn test_load_jpeg_from_memory() {
        let rgb_img = create_test_rgb_image();
        let dyn_img = DynamicImage::ImageRgb8(rgb_img);
        let jpeg_bytes = encode_image(&dyn_img, ImageFormat::Jpeg);

        // 从字节加载 JPEG
        let loaded = load_from_memory(&jpeg_bytes).expect("❌ 无法从内存加载 JPEG 图像");
        assert_eq!(loaded.width(), 4, "❌ JPEG 图像宽度应为 4");
        assert_eq!(loaded.height(), 4, "❌ JPEG 图像高度应为 4");

        println!(
            "✅ JPEG 图像从内存加载成功：{}x{}",
            loaded.width(),
            loaded.height()
        );
    }

    /// 测试 3：使用明确的格式标识加载图像
    #[test]
    fn test_load_with_explicit_format() {
        let rgba_img = create_test_rgba_image();
        let dyn_img = DynamicImage::ImageRgba8(rgba_img);
        let png_bytes = encode_image(&dyn_img, ImageFormat::Png);

        // 使用 load_from_memory_with_format 指定格式
        let loaded = load_from_memory_with_format(&png_bytes, ImageFormat::Png)
            .expect("❌ 使用明确格式加载 PNG 失败");

        assert_eq!(loaded.width(), 4);
        assert_eq!(loaded.height(), 4);

        println!(
            "✅ 使用明确格式加载成功：{}x{}",
            loaded.width(),
            loaded.height()
        );
    }

    /// 测试 4：保存图像为不同格式
    #[test]
    fn test_save_to_different_formats() {
        let rgba_img = create_test_rgba_image();
        let dyn_img = DynamicImage::ImageRgba8(rgba_img);

        // 保存为 PNG
        let png_bytes = encode_image(&dyn_img, ImageFormat::Png);
        assert!(!png_bytes.is_empty(), "❌ PNG 编码结果不应为空");
        // PNG 有固定的签名头（8 字节）
        assert_eq!(&png_bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10], "❌ PNG 签名不正确");

        // 保存为 JPEG
        let jpeg_bytes = encode_image(&dyn_img, ImageFormat::Jpeg);
        assert!(!jpeg_bytes.is_empty(), "❌ JPEG 编码结果不应为空");
        // JPEG 有固定的 SOI 标记 FF D8
        assert_eq!(&jpeg_bytes[..2], &[0xFF, 0xD8], "❌ JPEG SOI 标记不正确");

        // 保存为 WEBP
        let webp_bytes = encode_image(&dyn_img, ImageFormat::WebP);
        assert!(!webp_bytes.is_empty(), "❌ WebP 编码结果不应为空");
        // WebP 有固定的 RIFF 头
        assert_eq!(&webp_bytes[..4], b"RIFF", "❌ WebP RIFF 头不正确");

        println!("✅ 图像保存为 PNG/JPEG/WebP 格式均成功");
    }

    /// 测试 5：验证图像的元数据
    #[test]
    fn test_image_metadata() {
        let rgba_img = create_test_rgba_image();
        let dyn_img = DynamicImage::ImageRgba8(rgba_img);

        assert_eq!(dyn_img.width(), 4, "❌ 图像宽度应为 4");
        assert_eq!(dyn_img.height(), 4, "❌ 图像高度应为 4");
        assert!(
            dyn_img.color().has_color(),
            "❌ 图像应有颜色通道"
        );
        assert_eq!(
            dyn_img.color().channel_count(),
            4,
            "❌ RGBA 图像应有 4 个通道"
        );

        println!("✅ 图像元数据验证通过：{}x{}, {}bit, {}通道",
            dyn_img.width(), dyn_img.height(),
            dyn_img.color().bits_per_pixel(),
            dyn_img.color().channel_count());
    }
}

// ======================== 图像格式转换测试 ========================

mod format_conversion_tests {
    use super::*;

    /// 测试 6：PNG → JPEG 格式转换
    #[test]
    fn test_png_to_jpeg_conversion() {
        // 注意：JPEG 不支持透明通道，使用 RGB 图像
        let rgb_img = create_test_rgb_image();
        let dyn_img = DynamicImage::ImageRgb8(rgb_img);

        // 编码为 PNG
        let png_bytes = encode_image(&dyn_img, ImageFormat::Png);
        let loaded_png = load_from_memory(&png_bytes).expect("❌ 加载 PNG 失败");

        // 重新编码为 JPEG
        let jpeg_bytes = encode_image(&loaded_png, ImageFormat::Jpeg);
        let loaded_jpeg = load_from_memory(&jpeg_bytes).expect("❌ 加载 JPEG 失败");

        // JPEG 是有损压缩，尺寸应一致
        assert_eq!(loaded_jpeg.width(), 4, "❌ JPEG 转换后宽度应为 4");
        assert_eq!(loaded_jpeg.height(), 4, "❌ JPEG 转换后高度应为 4");

        println!("✅ PNG → JPEG 格式转换成功");
    }

    /// 测试 7：JPEG → PNG 格式转换
    #[test]
    fn test_jpeg_to_png_conversion() {
        let rgb_img = create_test_rgb_image();
        let dyn_img = DynamicImage::ImageRgb8(rgb_img);

        // 编码为 JPEG
        let jpeg_bytes = encode_image(&dyn_img, ImageFormat::Jpeg);
        let loaded_jpeg = load_from_memory(&jpeg_bytes).expect("❌ 加载 JPEG 失败");

        // 重新编码为 PNG
        let png_bytes = encode_image(&loaded_jpeg, ImageFormat::Png);
        let loaded_png = load_from_memory(&png_bytes).expect("❌ 加载转换后的 PNG 失败");

        assert_eq!(loaded_png.width(), 4, "❌ PNG 转换后宽度应为 4");
        assert_eq!(loaded_png.height(), 4, "❌ PNG 转换后高度应为 4");

        println!("✅ JPEG → PNG 格式转换成功");
    }

    /// 测试 8：RGBA → RGB 通道转换（去掉透明通道）
    #[test]
    fn test_rgba_to_rgb_conversion() {
        let rgba_img = create_test_rgba_image();
        let dyn_img = DynamicImage::ImageRgba8(rgba_img);

        // 转换为 RGB
        let rgb_img = dyn_img.to_rgb8();
        assert_eq!(rgb_img.width(), 4, "❌ 转换后宽度应为 4");
        assert_eq!(rgb_img.height(), 4, "❌ 转换后高度应为 4");

        // 验证右下角白色像素
        let pixel = rgb_img.get_pixel(3, 3);
        assert_eq!(pixel[0], 255); // R
        assert_eq!(pixel[1], 255); // G
        assert_eq!(pixel[2], 255); // B

        // 验证右上角绿色像素
        let pixel = rgb_img.get_pixel(3, 0);
        assert_eq!(pixel[0], 0);  // R
        assert_eq!(pixel[1], 255); // G
        assert_eq!(pixel[2], 0);  // B

        println!("✅ RGBA → RGB 通道转换成功");
    }

    /// 测试 9：RGB → Luma（灰度）转换
    #[test]
    fn test_rgb_to_luma_conversion() {
        let rgb_img = create_test_rgb_image();
        let dyn_img = DynamicImage::ImageRgb8(rgb_img);

        // 转为灰度
        let luma_img = dyn_img.to_luma8();
        assert_eq!(luma_img.width(), 4, "❌ 灰度图宽度应为 4");
        assert_eq!(luma_img.height(), 4, "❌ 灰度图高度应为 4");

        // 灰度值为单通道
        let pixel = luma_img.get_pixel(0, 0);
        assert_eq!(pixel.channels().len(), 1, "❌ 灰度图应为单通道");

        println!("✅ RGB → Luma 灰度转换成功");
    }
}

// ======================== 图像处理操作测试 ========================

mod image_processing_tests {
    use super::*;

    /// 测试 10：图像缩放（resize）
    #[test]
    fn test_image_resize() {
        let rgba_img = create_test_rgba_image();
        let dyn_img = DynamicImage::ImageRgba8(rgba_img);

        // 缩放到 8x8（双倍大小）
        let resized = dyn_img.resize(8, 8, FilterType::Nearest);
        assert_eq!(resized.width(), 8, "❌ 缩放后宽度应为 8");
        assert_eq!(resized.height(), 8, "❌ 缩放后高度应为 8");

        // 使用 Nearest 插值时，放大后的像素应与原始像素一致
        let original_pixel = dyn_img.get_pixel(0, 0); // 红色
        let resized_pixel = resized.get_pixel(0, 0);
        assert_eq!(
            original_pixel, resized_pixel,
            "❌ Nearest 插值放大后像素应与原始像素一致"
        );

        // 缩放到 2x2（缩小一半）
        let resized_small = dyn_img.resize(2, 2, FilterType::Nearest);
        assert_eq!(resized_small.width(), 2, "❌ 缩小后宽度应为 2");
        assert_eq!(resized_small.height(), 2, "❌ 缩小后高度应为 2");

        println!("✅ 图像缩放测试通过：4x4 → 8x8（放大）→ 2x2（缩小）");
    }

    /// 测试 11：图像缩放（保持宽高比）
    #[test]
    fn test_image_resize_keep_aspect_ratio() {
        let rgba_img = create_test_rgba_image();
        let dyn_img = DynamicImage::ImageRgba8(rgba_img);

        // resize_to_fill：填充到目标尺寸，可能裁剪
        let filled = dyn_img.resize_to_fill(8, 4, FilterType::Nearest);
        assert_eq!(filled.width(), 8, "❌ fill 后宽度应为 8");
        assert_eq!(filled.height(), 4, "❌ fill 后高度应为 4");

        // resize_exact：精确缩放到目标尺寸
        let exact = dyn_img.resize_exact(6, 6, FilterType::Nearest);
        assert_eq!(exact.width(), 6, "❌ exact 后宽度应为 6");
        assert_eq!(exact.height(), 6, "❌ exact 后高度应为 6");

        println!("✅ 图像保持宽高比缩放测试通过");
    }

    /// 测试 12：图像裁剪（crop）
    #[test]
    fn test_image_crop() {
        let rgba_img = create_test_rgba_image();
        let dyn_img = DynamicImage::ImageRgba8(rgba_img);

        // 裁剪左上角 2x2 区域（应为全红）
        let cropped = dyn_img.crop_imm(0, 0, 2, 2);
        assert_eq!(cropped.width(), 2, "❌ 裁剪后宽度应为 2");
        assert_eq!(cropped.height(), 2, "❌ 裁剪后高度应为 2");

        // 验证裁剪区域全为红色
        for y in 0..2 {
            for x in 0..2 {
                let pixel = cropped.get_pixel(x, y);
                assert_eq!(
                    pixel,
                    Rgba([255, 0, 0, 255]),
                    "❌ 左上角裁剪区域应为全红色 ({},{})",
                    x,
                    y
                );
            }
        }

        // 裁剪右下角 2x2 区域（应为全白）
        let cropped_bottom_right = dyn_img.crop_imm(2, 2, 2, 2);
        for y in 0..2 {
            for x in 0..2 {
                let pixel = cropped_bottom_right.get_pixel(x, y);
                assert_eq!(
                    pixel,
                    Rgba([255, 255, 255, 255]),
                    "❌ 右下角裁剪区域应为全白色 ({},{})",
                    x,
                    y
                );
            }
        }

        println!("✅ 图像裁剪测试通过");
    }

    /// 测试 13：图像旋转
    #[test]
    fn test_image_rotate() {
        let rgba_img = create_test_rgba_image();
        let dyn_img = DynamicImage::ImageRgba8(rgba_img);

        // 旋转 90 度
        let rotated_90 = dyn_img.rotate90();
        assert_eq!(rotated_90.width(), 4, "❌ 旋转 90° 后宽度应为 4");
        assert_eq!(rotated_90.height(), 4, "❌ 旋转 90° 后高度应为 4");

        // 旋转 180 度
        let rotated_180 = dyn_img.rotate180();
        // 旋转 180° 后，左上角(0,0)应为原来右下角(3,3)的白色
        let pixel = rotated_180.get_pixel(0, 0);
        assert_eq!(
            pixel,
            Rgba([255, 255, 255, 255]),
            "❌ 旋转 180° 后左上角应为白色"
        );

        // 旋转 270 度
        let rotated_270 = dyn_img.rotate270();
        assert_eq!(rotated_270.width(), 4);
        assert_eq!(rotated_270.height(), 4);

        // 验证旋转 270° = 旋转 90° 三次
        let rotated_90_twice = dyn_img.rotate90().rotate90();
        let rotated_180_again = rotated_90_twice.rotate90();
        let pixel_270 = rotated_270.get_pixel(0, 0);
        let pixel_90x3 = rotated_180_again.get_pixel(0, 0);
        assert_eq!(
            pixel_270, pixel_90x3,
            "❌ rotate270() 应等同于 rotate90() × 3"
        );

        println!("✅ 图像旋转测试通过（90°/180°/270°）");
    }

    /// 测试 14：图像灰度化
    #[test]
    fn test_image_grayscale() {
        let rgba_img = create_test_rgba_image();
        let dyn_img = DynamicImage::ImageRgba8(rgba_img);

        // 灰度化
        let gray = dyn_img.grayscale();

        // 尺寸不变
        assert_eq!(gray.width(), 4, "❌ 灰度化后宽度应为 4");
        assert_eq!(gray.height(), 4, "❌ 灰度化后高度应为 4");

        // 灰度化后颜色类型应为 Luma
        assert_eq!(
            gray.color(),
            image::ColorType::L8,
            "❌ 灰度化后颜色类型应为 L8"
        );

        // 验证灰度值计算：Gray = 0.299*R + 0.587*G + 0.114*B
        // 红色 (255,0,0) → 约 76
        let red_gray = gray.get_pixel(0, 0);
        let gray_value = red_gray[0] as f64;
        let expected = (0.299 * 255.0 + 0.587 * 0.0 + 0.114 * 0.0).round() as u8;
        assert_eq!(
            gray_value as u8, expected,
            "❌ 红色灰度化后亮度值应为 {}",
            expected
        );

        println!("✅ 图像灰度化测试通过");
    }

    /// 测试 15：图像翻转
    #[test]
    fn test_image_flip() {
        let rgba_img = create_test_rgba_image();
        let dyn_img = DynamicImage::ImageRgba8(rgba_img);

        // 水平翻转
        let flipped_h = dyn_img.fliph();
        // 翻转后左上角应为原来右上角的绿色
        let pixel = flipped_h.get_pixel(0, 0);
        assert_eq!(
            pixel,
            Rgba([0, 255, 0, 255]),
            "❌ 水平翻转后左上角应为绿色"
        );

        // 垂直翻转
        let flipped_v = dyn_img.flipv();
        // 翻转后左上角应为原来左下角的蓝色
        let pixel = flipped_v.get_pixel(0, 0);
        assert_eq!(
            pixel,
            Rgba([0, 0, 255, 255]),
            "❌ 垂直翻转后左上角应为蓝色"
        );

        println!("✅ 图像翻转测试通过（水平/垂直）");
    }

    /// 测试 16：图像模糊（高斯模糊）
    #[test]
    fn test_image_blur() {
        let rgba_img = create_test_rgba_image();
        let dyn_img = DynamicImage::ImageRgba8(rgba_img);

        // 高斯模糊（sigma = 1.0）
        let blurred = dyn_img.blur(1.0);

        // 尺寸不变
        assert_eq!(blurred.width(), 4, "❌ 模糊后宽度应为 4");
        assert_eq!(blurred.height(), 4, "❌ 模糊后高度应为 4");

        // 模糊后边界像素应有所变化（不再是纯色边界）
        let interior_pixel = blurred.get_pixel(1, 1); // 靠近红色区域内部
        let red_pixel = Rgba([255, 0, 0, 255]);
        assert_ne!(
            interior_pixel, red_pixel,
            "❌ 模糊后内部像素不应保持纯红色"
        );

        println!("✅ 图像高斯模糊测试通过");
    }

    /// 测试 17：图像亮度调整
    #[test]
    fn test_image_brighten() {
        let rgb_img = create_test_rgb_image();
        let dyn_img = DynamicImage::ImageRgb8(rgb_img);

        // 增加亮度 50
        let brightened = dyn_img.brighten(50);
        let pixel = brightened.get_pixel(0, 0);
        // 红色(255,0,0) 增加亮度后 R 应饱和为 255
        assert_eq!(pixel[0], 255, "❌ 增加亮度后 R 通道应为 255（饱和）");
        assert!(
            pixel[1] > 0 || pixel[2] > 0,
            "❌ 增加亮度后 G/B 通道应大于 0"
        );

        // 降低亮度 -50
        let darkened = dyn_img.brighten(-50);
        let pixel = darkened.get_pixel(2, 2);
        // 白色(255,255,255) 降低亮度后应变为灰色
        assert!(
            pixel[0] < 255,
            "❌ 降低亮度后 R 通道应小于 255"
        );
        assert!(
            pixel[0] > 0,
            "❌ 降低亮度后 R 通道应大于 0（灰色）"
        );

        println!("✅ 图像亮度调整测试通过");
    }

    /// 测试 18：图像对比度调整
    #[test]
    fn test_image_adjust_contrast() {
        let rgb_img = create_test_rgb_image();
        let dyn_img = DynamicImage::ImageRgb8(rgb_img);

        // 增加对比度
        let high_contrast = dyn_img.adjust_contrast(100.0);
        let pixel = high_contrast.get_pixel(0, 0);
        // 高对比度下红色应更红（R 饱和）
        assert_eq!(pixel[0], 255, "❌ 高对比度下 R 通道应为 255");

        // 降低对比度
        let low_contrast = dyn_img.adjust_contrast(-100.0);
        let pixel = low_contrast.get_pixel(0, 0);
        // 低对比度下红色应变为灰色
        assert!(
            pixel[0] > 0,
            "❌ 低对比度下 R 通道应大于 0"
        );

        println!("✅ 图像对比度调整测试通过");
    }
}

// ======================== 像素级操作测试 ========================

mod pixel_operations_tests {
    use super::*;

    /// 测试 19：读取和验证单个像素
    #[test]
    fn test_get_pixel() {
        let img = create_test_rgba_image();

        // 四角的像素颜色验证
        assert_eq!(img.get_pixel(0, 0), Rgba([255, 0, 0, 255]), "❌ 左上角应为红色");
        assert_eq!(img.get_pixel(3, 0), Rgba([0, 255, 0, 255]), "❌ 右上角应为绿色");
        assert_eq!(img.get_pixel(0, 3), Rgba([0, 0, 255, 255]), "❌ 左下角应为蓝色");
        assert_eq!(img.get_pixel(3, 3), Rgba([255, 255, 255, 255]), "❌ 右下角应为白色");

        println!("✅ 像素读取测试通过");
    }

    /// 测试 20：设置和修改单个像素
    #[test]
    fn test_put_pixel() {
        let mut img = create_test_rgba_image();

        // 修改像素：将左上角红色改为黄色
        img.put_pixel(0, 0, Rgba([255, 255, 0, 255]));
        assert_eq!(
            img.get_pixel(0, 0),
            Rgba([255, 255, 0, 255]),
            "❌ 修改后像素应为黄色"
        );

        // 验证相邻像素未被修改
        assert_eq!(
            img.get_pixel(0, 1),
            Rgba([255, 0, 0, 255]),
            "❌ 相邻像素应仍为红色"
        );

        println!("✅ 像素修改测试通过");
    }

    /// 测试 21：批量像素操作：将图像所有像素设为同一种颜色
    #[test]
    fn test_fill_pixels() {
        let mut img = create_test_rgba_image();
        let fill_color = Rgba([128, 128, 128, 255]);

        // 逐像素填充
        for y in 0..img.height() {
            for x in 0..img.width() {
                img.put_pixel(x, y, fill_color);
            }
        }

        // 验证所有像素
        for y in 0..img.height() {
            for x in 0..img.width() {
                assert_eq!(
                    img.get_pixel(x, y),
                    fill_color,
                    "❌ 所有像素应被填充为灰色 ({},{})",
                    x,
                    y
                );
            }
        }

        println!("✅ 批量像素填充测试通过");
    }

    /// 测试 22：像素通道操作
    #[test]
    fn test_pixel_channel_operations() {
        let img = create_test_rgba_image();

        // 获取白色像素
        let white_pixel = img.get_pixel(3, 3);
        let channels = white_pixel.channels();

        assert_eq!(channels.len(), 4, "❌ RGBA 像素应有 4 个通道");
        assert_eq!(channels[0], 255); // R
        assert_eq!(channels[1], 255); // G
        assert_eq!(channels[2], 255); // B
        assert_eq!(channels[3], 255); // A

        // 获取各通道分量
        let (r, g, b, a) = white_pixel.0;
        assert_eq!(r, 255);
        assert_eq!(g, 255);
        assert_eq!(b, 255);
        assert_eq!(a, 255);

        println!("✅ 像素通道操作测试通过");
    }

    /// 测试 23：遍历图像所有像素
    #[test]
    fn test_enumerate_pixels() {
        let img = create_test_rgba_image();
        let mut pixel_count = 0u32;

        // 使用像素坐标迭代器
        for (x, y, pixel) in img.enumerate_pixels() {
            assert_eq!(
                *pixel,
                img.get_pixel(x, y),
                "❌ enumerate_pixels 返回的像素应与 get_pixel 一致"
            );
            pixel_count += 1;
        }

        assert_eq!(pixel_count, 16, "❌ 4x4 图像应有 16 个像素");
        println!("✅ 像素遍历测试通过（共 {} 个像素）", pixel_count);
    }
}

// ======================== 边界情况测试 ========================

mod edge_case_tests {
    use super::*;

    /// 测试 24：最小图像（1x1）
    #[test]
    fn test_minimal_image() {
        let pixel = Rgba([128, 64, 32, 255]);
        let mut img = RgbaImage::new(1, 1);
        img.put_pixel(0, 0, pixel);

        assert_eq!(img.width(), 1, "❌ 1x1 图像宽度应为 1");
        assert_eq!(img.height(), 1, "❌ 1x1 图像高度应为 1");
        assert_eq!(
            img.get_pixel(0, 0),
            pixel,
            "❌ 1x1 图像像素应与设置值一致"
        );

        // 编码为 PNG
        let dyn_img = DynamicImage::ImageRgba8(img);
        let png_bytes = encode_image(&dyn_img, ImageFormat::Png);
        let loaded = load_from_memory(&png_bytes).expect("❌ 无法加载 1x1 PNG");
        assert_eq!(loaded.width(), 1, "❌ 加载后的 1x1 图像宽度应为 1");

        println!("✅ 最小图像（1x1）测试通过");
    }

    /// 测试 25：矩形图像（非正方形）
    #[test]
    fn test_rectangular_image() {
        let mut img = RgbaImage::new(8, 4);

        // 上半部分红色，下半部分蓝色
        for y in 0..2 {
            for x in 0..8 {
                img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }
        for y in 2..4 {
            for x in 0..8 {
                img.put_pixel(x, y, Rgba([0, 0, 255, 255]));
            }
        }

        assert_eq!(img.width(), 8, "❌ 矩形图像宽度应为 8");
        assert_eq!(img.height(), 4, "❌ 矩形图像高度应为 4");

        // 验证区域颜色
        assert_eq!(img.get_pixel(0, 0), Rgba([255, 0, 0, 255]), "❌ 上半部分应为红色");
        assert_eq!(img.get_pixel(0, 3), Rgba([0, 0, 255, 255]), "❌ 下半部分应为蓝色");

        // 编码与重新加载
        let dyn_img = DynamicImage::ImageRgba8(img);
        let bytes = encode_image(&dyn_img, ImageFormat::Png);
        let loaded = load_from_memory(&bytes).expect("❌ 无法加载矩形 PNG");
        assert_eq!(loaded.width(), 8);
        assert_eq!(loaded.height(), 4);

        println!("✅ 矩形图像（8x4）测试通过");
    }

    /// 测试 26：所有像素为同一颜色
    #[test]
    fn test_uniform_color_image() {
        let uniform_color = Rgba([42, 128, 200, 255]);
        let mut img = RgbaImage::new(10, 10);

        // 全部设置为同一颜色
        for y in 0..img.height() {
            for x in 0..img.width() {
                img.put_pixel(x, y, uniform_color);
            }
        }

        // 验证所有像素一致
        for y in 0..img.height() {
            for x in 0..img.width() {
                assert_eq!(
                    img.get_pixel(x, y),
                    uniform_color,
                    "❌ 所有像素应为同一颜色 ({},{})",
                    x,
                    y
                );
            }
        }

        println!("✅ 纯色图像（10x10）测试通过");
    }

    /// 测试 27：RGB 与 RGBA 转换边界
    #[test]
    fn test_rgb_rgba_conversion_boundary() {
        // RGB → RGBA（添加 alpha 通道）
        let rgb_img = create_test_rgb_image();
        let rgba = DynamicImage::ImageRgb8(rgb_img).to_rgba8();

        assert_eq!(rgba.width(), 4, "❌ RGB→RGBA 后宽度应为 4");
        assert_eq!(rgba.height(), 4, "❌ RGB→RGBA 后高度应为 4");

        // 验证透明通道默认为 255
        let pixel = rgba.get_pixel(0, 0);
        assert_eq!(pixel[3], 255, "❌ RGB→RGBA 默认 alpha 应为 255");

        // RGBA → RGB（去掉 alpha 通道）
        let rgba_img = create_test_rgba_image();
        let rgb = DynamicImage::ImageRgba8(rgba_img).to_rgb8();

        assert_eq!(rgb.channels().len(), 3, "❌ RGBA→RGB 后应有 3 通道");

        println!("✅ RGB ↔ RGBA 转换边界测试通过");
    }

    /// 测试 28：图像格式检测
    #[test]
    fn test_image_format_detection() {
        // 验证已知格式的幻数
        let rgb_img = create_test_rgb_image();
        let dyn_img = DynamicImage::ImageRgb8(rgb_img);

        // PNG 格式
        let png_bytes = encode_image(&dyn_img, ImageFormat::Png);
        let detected = image::guess_format(&png_bytes).expect("❌ 无法检测 PNG 格式");
        assert_eq!(detected, ImageFormat::Png, "❌ 检测到的格式应为 PNG");

        // JPEG 格式
        let jpeg_bytes = encode_image(&dyn_img, ImageFormat::Jpeg);
        let detected = image::guess_format(&jpeg_bytes).expect("❌ 无法检测 JPEG 格式");
        assert_eq!(detected, ImageFormat::Jpeg, "❌ 检测到的格式应为 JPEG");

        // WebP 格式
        let webp_bytes = encode_image(&dyn_img, ImageFormat::WebP);
        let detected = image::guess_format(&webp_bytes).expect("❌ 无法检测 WebP 格式");
        assert_eq!(detected, ImageFormat::WebP, "❌ 检测到的格式应为 WebP");

        println!("✅ 图像格式检测测试通过（PNG/JPEG/WebP）");
    }

    /// 测试 29：透明通道（alpha）处理
    #[test]
    fn test_alpha_channel() {
        let mut img = RgbaImage::new(4, 4);
        let semi_transparent = Rgba([255, 0, 0, 128]); // 半透明红色
        let fully_transparent = Rgba([0, 255, 0, 0]); // 全透明绿色

        img.put_pixel(0, 0, semi_transparent);
        img.put_pixel(1, 1, fully_transparent);

        // 验证 alpha 值
        assert_eq!(img.get_pixel(0, 0)[3], 128, "❌ 半透明像素 alpha 应为 128");
        assert_eq!(img.get_pixel(1, 1)[3], 0, "❌ 全透明像素 alpha 应为 0");

        // 编码为 PNG（PNG 支持透明通道）
        let dyn_img = DynamicImage::ImageRgba8(img);
        let png_bytes = encode_image(&dyn_img, ImageFormat::Png);
        let loaded = load_from_memory(&png_bytes).expect("❌ 无法加载含透明通道的 PNG");

        // 加载后的透明值可能因编码/解码有微小差异，但应保持一致
        let loaded_pixel = loaded.get_pixel(0, 0);
        assert_eq!(loaded_pixel[3], 128, "❌ 编码/解码后 alpha 应为 128");

        println!("✅ 透明通道测试通过");
    }

    /// 测试 30：无效数据处理
    #[test]
    fn test_invalid_data_handling() {
        // 空字节数组
        let empty: Vec<u8> = vec![];
        let result = load_from_memory(&empty);
        assert!(
            result.is_err(),
            "❌ 空字节数组应返回错误，但得到：{:?}",
            result
        );

        // 无效的随机字节
        let invalid_data = vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05];
        let result = load_from_memory(&invalid_data);
        assert!(
            result.is_err(),
            "❌ 无效数据应返回错误，但得到：{:?}",
            result
        );

        // 截断的 PNG 数据（仅有 4 字节）
        let truncated = vec![137, 80, 78, 71];
        let result = load_from_memory(&truncated);
        assert!(
            result.is_err(),
            "❌ 截断的 PNG 数据应返回错误，但得到：{:?}",
            result
        );

        println!("✅ 无效数据处理测试通过（空数据/无效数据/截断数据）");
    }
}

// ======================== ImageBuffer 与 DynamicImage 测试 ========================

mod image_buffer_tests {
    use super::*;

    /// 测试 31：ImageBuffer 基础操作
    #[test]
    fn test_image_buffer_creation() {
        // 创建 ImageBuffer 的多种方式
        let img1 = RgbaImage::new(100, 50);
        assert_eq!(img1.width(), 100);
        assert_eq!(img1.height(), 50);

        // 使用 from_vec 创建
        let data = vec![128u8; 100 * 50 * 4]; // RGBA 数据
        let img2 = RgbaImage::from_raw(100, 50, data).expect("❌ from_raw 创建失败");
        assert_eq!(img2.width(), 100);
        assert_eq!(img2.height(), 50);

        // 使用 from_fn 创建（渐变图案）
        let img3 = RgbaImage::from_fn(10, 10, |x, y| {
            Rgba([
                (x * 25) as u8,
                (y * 25) as u8,
                128,
                255,
            ])
        });
        assert_eq!(img3.width(), 10);
        assert_eq!(img3.height(), 10);

        // 验证渐变
        let pixel = img3.get_pixel(2, 3);
        assert_eq!(pixel[0], 50, "❌ x=2 时 R 应为 50");  // 2*25=50
        assert_eq!(pixel[1], 75, "❌ y=3 时 G 应为 75");  // 3*25=75

        println!("✅ ImageBuffer 创建测试通过（new/from_raw/from_fn）");
    }

    /// 测试 32：DynamicImage 转换
    #[test]
    fn test_dynamic_image_conversions() {
        let rgba_img = create_test_rgba_image();

        // 创建 DynamicImage
        let dyn_img = DynamicImage::ImageRgba8(rgba_img);

        // 转换为不同的颜色类型
        let _rgb = dyn_img.to_rgb8();
        let _rgba = dyn_img.to_rgba8();
        let _luma = dyn_img.to_luma8();
        let _luma_alpha = dyn_img.to_luma_alpha8();
        let _bgr = dyn_img.to_bgr8();
        let _bgra = dyn_img.to_bgra8();

        // 验证转换后尺寸一致
        assert_eq!(dyn_img.to_rgb8().width(), 4);
        assert_eq!(dyn_img.to_rgba8().width(), 4);
        assert_eq!(dyn_img.to_luma8().width(), 4);

        println!("✅ DynamicImage 转换测试通过（RGB/RGBA/Luma/BGR/BGRA）");
    }

    /// 测试 33：ImageBuffer 像素迭代器
    #[test]
    fn test_pixel_iterators() {
        let img = create_test_rgba_image();

        // 使用 rows() 逐行迭代
        let mut row_count = 0u32;
        for row in img.rows() {
            for _pixel in row {
                // 只计数，不检查
            }
            row_count += 1;
        }
        assert_eq!(row_count, 4, "❌ 应有 4 行");

        // 使用 pixels() 迭代所有像素
        let mut pixel_count = 0u32;
        for _pixel in img.pixels() {
            pixel_count += 1;
        }
        assert_eq!(pixel_count, 16, "❌ 应有 16 个像素");

        println!("✅ 像素迭代器测试通过（rows/pixels）");
    }

    /// 测试 34：图像子区域视图
    #[test]
    fn test_image_sub_view() {
        let img = create_test_rgba_image();

        // 获取子区域视图（左上角 2x2）
        let view = img.view(0, 0, 2, 2);
        assert_eq!(view.width(), 2, "❌ 子视图宽度应为 2");
        assert_eq!(view.height(), 2, "❌ 子视图高度应为 2");

        // 验证子视图中的像素
        let pixel = view.get_pixel(0, 0);
        assert_eq!(
            pixel,
            Rgba([255, 0, 0, 255]),
            "❌ 子视图左上角应为红色"
        );

        println!("✅ 图像子区域视图测试通过");
    }
}