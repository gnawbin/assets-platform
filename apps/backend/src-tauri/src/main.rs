// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 先在线程中启动 HTTP API 服务器（不依赖 Tauri）
    // 这样即使 Tauri IPC 初始化失败，上传 API 服务仍然可用

    // 再正常启动 Tauri 桌面应用
    assetsplatform_lib::run();
}
