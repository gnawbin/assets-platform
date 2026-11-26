//! 由 vendored `.proto` 生成 OpenShell 客户端代码
//!
//! - `protoc` 与 well-known types 由 `protoc-bin-vendored` 提供，CI / 开发机都不需要预装；
//! - `include_file("_includes.rs")` 生成按 package 嵌套的模块树，
//!   `lib.rs` 里一次性 `include!` 即可（`openshell::v1` / `openshell::datamodel::v1` ...）；
//! - 只生成客户端（`build_server(false)`），避免无谓的 server 代码。

use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let proto_root = manifest_dir.join("proto");

    // 只编译入口文件；被 import 的 proto 由 protoc 递归解析
    let protos = [proto_root.join("openshell.proto")];

    let mut config = tonic_prost_build::Config::new();
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);

    let includes: Vec<PathBuf> = vec![
        proto_root.clone(),
        protoc_bin_vendored::include_path()?,
    ];

    tonic_prost_build::configure()
        .build_server(false)
        .build_client(true)
        .include_file("_includes.rs")
        .compile_with_config(config, &protos, &includes)?;

    println!("cargo:rerun-if-changed={}", Path::new("proto").display());
    Ok(())
}