# Scripts 目录

本目录包含项目的各种脚本工具。

## fix-pnpm-native-binary.cjs

修复 pnpm 12 无法下载自带原生可执行文件的问题，典型报错：

```text
Downloading the pnpm 12.15.0 binary for linux-x64...
Could not download the pnpm 12.15.0 binary: ...
```

### 原因

`package.json` 的 `packageManager` 把 pnpm 固定在 `pnpm@12.15.0`，而 pnpm 12 不再是一份纯 JS 包，
它需要一个平台相关的原生可执行文件（`@pnpm/exe.<platform>-<arch>`，压缩包约 25 MB）。Corepack
安装 pnpm 包装器时既不安装依赖也不执行生命周期脚本，所以这个可执行文件是在**第一次运行 pnpm
时**由 `bin/pnpm.mjs`（内部使用 `get-pnpm`）临时下载的，而它有两个坑：

- 不会断点续传、不会重试。网络抖动或传输被截断就会失败，并在包装器目录下留下一堆
  `pnpm-native.<hash>.tgz` 碎片（可以用 `gzip -t` 看到 `unexpected end of file`）。
- 镜像返回**相对** `dist.tarball` 时（例如华为云返回
  `/@pnpm/exe.win32-x64/-/exe.win32-x64-12.15.0.tgz`），`new URL()` 直接抛错，任何 pnpm 命令都跑不起来。

### 使用方法

```bash
# 1. 先跑一次 pnpm，让它把包装器解包出来（会报上面那个错，但包装器已经落地）
pnpm -v

# 2. 用脚本重新下载并安装原生可执行文件
node script/fix-pnpm-native-binary.cjs

# 想重新下载（例如验证）时加 --force
node script/fix-pnpm-native-binary.cjs --force
```

脚本会：

- 从 `package.json` 的 `packageManager` 读取版本，按当前平台挑选正确的 `@pnpm/exe.*` 包；
- 从 registry（`COREPACK_NPM_REGISTRY` / `.npmrc` / `npm_config_registry`）读取元数据，
  用**绝对** tarball 地址下载；传输中断时按已下载的字节断点续传并自动重试（默认 8 次）；
- 依次尝试 registry → registry 声明的地址 → 备用镜像（`PNPM_NATIVE_BINARY_MIRROR`，以及内置的
  `https://registry.npmmirror.com`）；
- 用 registry 公布的 integrity 校验字节，校验不过就换下一个源（所以镜像无法塞入错误内容）；
- 把可执行文件原子地写到 `<corepack home>/v1/pnpm/<version>/pnpm-native[.exe]`，再执行
  `--version` 自检。

脚本不会改动项目里的任何文件，可以随时重复执行。

### 可选环境变量

| 变量 | 说明 |
| --- | --- |
| `PNPM_NATIVE_BINARY_MIRROR` | 优先尝试的镜像 registry，多个用逗号分隔 |
| `COREPACK_NPM_REGISTRY` | 读取元数据/下载所用的 registry |
| `COREPACK_HOME` | Corepack 缓存目录（默认 `~/.cache/node/corepack`） |

### 备选方案

不想改缓存的话，也可以直接用 npm 安装同一版本的 pnpm：`npm install -g pnpm@12.15.0`。npm 会把
`@pnpm/exe.*` 一起装好，之后 `pnpm` 启动时不会再联网下载。

## update-version.js

版本升级脚本，用于自动更新项目版本号并创建 git 标签。

### 功能

- 更新 `package.json` 中的版本号
- 更新 `src-tauri/Cargo.toml` 中的版本号
- 更新 `src-tauri/Cargo.lock` 中的版本号（如果存在）
- 自动提交修改的文件并推送到远程仓库
- 创建 git 标签（格式：`v{version}`）
- 推送标签到远程仓库

### 使用方法

```bash
# 方法1：直接运行脚本（支持v前缀）
node scripts/update-version.js v1.2.3
node scripts/update-version.js 1.2.3

# 方法2：使用 npm 脚本
npm run version:update v1.2.3
```

### 参数说明

- `version`: 新的版本号，支持v前缀（如：v1.2.3、1.2.3、v2.0.0-beta.1）

### 注意事项

1. 运行前建议先提交所有未提交的更改
2. 脚本会检查版本号格式的有效性
3. 如果标签已存在，脚本会报错并停止执行
4. 脚本会自动执行以下操作：
   - 添加修改的文件到暂存区
   - 创建提交（格式："chore: bump version to vx.x.x"）
   - 推送更改到远程仓库
   - 创建并推送版本标签

### 示例

```bash
# 升级到版本 0.3.0（支持v前缀）
node scripts/update-version.js v0.3.0
node scripts/update-version.js 0.3.0

# 升级到预发布版本
node scripts/update-version.js v1.0.0-beta.1

# 查看帮助信息
node scripts/update-version.js --help
```