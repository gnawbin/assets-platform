//! S3 连通性探测（诊断工具）
//!
//! 复现并定位 "connection closed before message completed" 问题：
//! 直接调用 `create_multipart_upload` 对配置的 S3 端点（RustFS）发起请求，
//! 可切换 endpoint / 路径风格等参数，观察 SDK 的实际请求与失败模式。
//!
//! 用法（在 src-tauri 目录下）：
//! ```bash
//! S3_ENDPOINT=http://localhost:9000 \
//! S3_ACCESS_KEY=xxx S3_SECRET_KEY=xxx S3_BUCKET=assets \
//! RUST_LOG=aws_smithy_runtime=trace,aws_smithy_http=trace,assets_storage=debug \
//! cargo run -p assets-storage --example s3_probe
//! ```

use assets_storage::s3::{S3Client, S3Config};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = S3Config::from_env().expect("缺少 S3_* 环境变量");
    eprintln!(
        "==> endpoint={} bucket={} region={}",
        config.endpoint, config.bucket, config.region
    );

    let client = S3Client::new(config)
        .await
        .expect("S3 客户端初始化失败");

    let key = format!("probe/s3_probe_{}.bin", std::process::id());
    eprintln!("==> 发起 CreateMultipartUpload: bucket={} key={}", "assets", key);

    match client
        .create_multipart_upload("assets", &key, "application/octet-stream")
        .await
    {
        Ok(upload_id) => {
            eprintln!("✅ CreateMultipartUpload 成功: upload_id={}", upload_id);
        }
        Err(e) => {
            eprintln!("❌ CreateMultipartUpload 失败:");
            eprintln!("   {}", e.full_message());
            std::process::exit(1);
        }
    }
}
