# Tauri + React + Typescript

This template should help get you started developing with Tauri, React and Typescript in Vite.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## 开发环境准备（Prerequisites）

| 依赖 | 版本要求 | 说明 |
| --- | --- | --- |
| Node.js | >= 18 | 前端构建与脚本运行 |
| pnpm | 见 `package.json` 的 `packageManager` 字段 | 包管理器 |
| Rust | 见 `rust-toolchain.toml` | 通过 [rustup](https://rustup.rs/) 安装 |
| C/C++ 编译工具链 | - | Windows：Visual Studio「使用 C++ 的桌面开发」工作负载<br>macOS：`xcode-select --install`<br>Linux：`build-essential` |
| **CMake** | **>= 3.12** | **必需**。`crates/ast` 启用了 tree-sitter 的 `wasm` feature，会引入 `wasmtime-c-api-impl`，其 `build.rs` 直接调用 `cmake`（仅用于安装 C 头文件，不编译 C++） |

一键检查并安装项目依赖：

```bash
pnpm run setup   # 等价于 node script/install-deps.cjs
```

该脚本会检查 Node.js、Yarn、Rust、**CMake**、Tauri CLI 等前置条件，缺 CMake 时会直接给出对应平台的安装指引。

### 安装 CMake

- **Windows（推荐）**：已安装 Visual Studio（勾选「使用 C++ 的桌面开发」工作负载）时**无需额外安装**，VS 自带一份 CMake，位于：

  ```
  C:\Program Files\Microsoft Visual Studio\<版本>\<Edition>\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin
  ```

  把该目录加入**用户 PATH**（系统属性 → 环境变量 → 用户变量 `Path`），然后**完全退出并重新打开**终端与 VS Code。
  其它方式：`winget install --id Kitware.CMake -e`，或从 <https://cmake.org/download/> 安装。
- **macOS**：`brew install cmake`
- **Linux**：`sudo apt-get install cmake`（Ubuntu/Debian）或 `sudo yum install cmake`（CentOS/RHEL）

验证：

```bash
cmake --version   # 需输出 3.12 以上版本
```

### 常见问题：``failed to spawn `cmake`: program not found``

Rust 编译 `wasmtime-c-api-impl` 时崩溃并提示上面的信息，说明执行 `cargo` 的进程环境里没有 `cmake`。注意以下两点：

1. 修改 PATH 后必须**完全重启** VS Code（关闭所有窗口，而不只是 `Reload Window`），因为 rust-analyzer 作为子进程只在启动时继承环境变量；重启后执行一次 `Rust Analyzer: Restart Server`。
2. 若 VS Code 设置了 `rust-analyzer.cargo.targetDir`（例如 `.ra-target`），rust-analyzer 使用独立的目标目录，其中会保留上次失败的构建缓存；重启后会重新执行 build script，无需手动删除该目录。

排查时可先在出问题的环境里执行 `cmake --version`；若终端正常而 IDE 仍报错，基本都是「没有完全重启 VS Code」导致。
