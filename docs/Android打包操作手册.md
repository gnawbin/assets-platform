# Tauri 生成 Android 程序操作手册（资产管理平台）

> 适用范围：本仓库 `apps/backend` 下的 Tauri v2 桌面应用（`apps/backend/src-tauri`）。
> 目标：把当前桌面 App 用 Tauri 生成（打包）成 Android 的 APK / AAB，并跑通到真机 / 模拟器。
> 结论先行：**本项目是 Tauri v2，代码层面已基本具备 Android 移动端条件，改动量为 0；主要缺口在构建环境（NDK、环境变量），补齐后两条命令即可出包。**

---

## 一、原理：Tauri v2 如何“生成 Android 程序”

Tauri v2 原生支持 Android/iOS（无需 v1 时代的移动分支）。所谓“生成 Android 程序”，本质上是三步，全部由 `@tauri-apps/cli` 驱动：

1. **初始化（`tauri android init`）**：在 `src-tauri/gen/android/` 生成一套 **Gradle + Kotlin** 的 Android 宿主工程（`app` 模块、`MainActivity` / `WryActivity`、`jniLibs` 目录、mipmap 图标等）。
2. **交叉编译 Rust（CLI 自动完成）**：调用 **NDK** 工具链，把 Rust 代码（含本仓库全部 `crates/*`）编译成 `libassetsplatform_lib.so`，放入 `gen/android/app/src/main/jniLibs/<abi>/`。
3. **Gradle 打包**：把前端静态资源 + `.so` + 图标 + AndroidManifest 打包成 **APK / AAB**。

```
前端 Next.js 静态导出 (apps/web/dist)           ← 打进 APK assets
        │
        ▼
Rust 后端 assetsplatform_lib.so (jniLibs)      ← NDK 交叉编译
        │
        ├─ WebView (Wry / 系统 WebView) 运行前端
        └─ 进程内 axum HTTP API (0.0.0.0) + PostgreSQL（配置见 .env.toml）
```

> 移动端与桌面的关键差异：桌面用系统窗口 + WebKitGTK，Android 用 **Activity + 系统 WebView**；`tauri.conf.json` 里桌面窗口尺寸等配置在移动端会被忽略，但 **Tauri IPC、插件、capability 权限体系完全一致**，前端代码通常无需改动。

---

## 二、本项目就绪度核对（2026-09 实测）

### 2.1 项目侧（已就绪 ✓）

| 检查项 | 结果 |
|--------|------|
| Tauri 版本 | `tauri 2.11.5`（crate）+ `@tauri-apps/cli 2.11.0` = **v2**，Android 内建支持 |
| `Cargo.toml` crate-type | 含 `staticlib` / `cdylib` / `rlib`（移动端必需）✓ |
| 移动端入口 | `src/lib.rs` 的 `run()` 已带 `#[cfg_attr(mobile, tauri::mobile_entry_point)]` ✓ |
| 前端产物 | Next.js `output: "export"` → `apps/web/dist`，与 `frontendDist: "../../web/dist"` 匹配 ✓ |
| applicationId 来源 | `tauri.conf.json` `identifier: "com.it.assets"` ✓ |
| 图标 | `icons/icon.png`（1024px）存在，init 时会生成 Android mipmap ✓ |
| capability | `windows: ["main"]` 与移动端默认窗口 label 一致 ✓ |

### 2.2 构建机侧（缺口 ⚠️）

| 检查项 | 现状 | 需要动作 |
|--------|------|----------|
| JDK | OpenJDK 21（≥17 即可） | 仅需 `export JAVA_HOME` |
| Android SDK | `~/Android/Sdk` 有 `platforms/android-37.0`、`build-tools/36.0.0`、`platform-tools`、licenses 已接受 | `export ANDROID_HOME` |
| **NDK** | ❌ 未安装（无 `~/Android/Sdk/ndk`，也无 `cmdline-tools`） | **必须先装 NDK（见 3.2）** |
| 环境变量 | `JAVA_HOME` / `ANDROID_HOME` / `NDK_HOME` 均未设置 | 写入 `~/.bashrc` 或每次构建前 export |
| Rust Android target | 仅 `aarch64-linux-android` | 只打 arm64 已够；全 ABI 再补 3 个 target |
| Rust 工具链 | rustc / cargo 1.98、rustup 1.29 ✓ | 无 |

### 2.3 结论

- **想“先出个能装的 APK”**：补齐 NDK + 3 个环境变量 → `tauri android init` → `tauri android build --apk -t aarch64`。
- **想真正在移动端跑业务**：还需处理第六节的运行时问题（`.env.toml` 进不了包、数据库可达性、doc-parser 侧车等）。

---

## 三、环境准备（一次性）

### 3.1 官方需求清单

| 需求 | 说明 |
|------|------|
| JDK 17+ | 系统 JDK 或 Android Studio 自带 JBR |
| Android Studio / SDK | 提供 SDK Platform、Platform-Tools、Build-Tools、NDK（Side-by-side）、Command-line Tools |
| `ANDROID_HOME` | 指向 Android SDK 根目录（如 `$HOME/Android/Sdk`） |
| `NDK_HOME` | 指向具体 NDK 版本目录（如 `$ANDROID_HOME/ndk/27.x.x`） |
| `JAVA_HOME` | 指向 JDK 17+ 根目录 |
| Rust Android targets | `aarch64-linux-android`、`armv7-linux-androideabi`、`i686-linux-android`、`x86_64-linux-android` |

### 3.2 本机（Ubuntu / WSL）操作步骤

**① 设置环境变量**（建议追加到 `~/.bashrc` 后 `source ~/.bashrc`）：

```bash
export ANDROID_HOME=$HOME/Android/Sdk
export NDK_HOME=$ANDROID_HOME/ndk/$(ls -1 $ANDROID_HOME/ndk | tail -1)   # 装好 NDK 后再执行
export JAVA_HOME=$(dirname $(dirname $(readlink -f $(which java))))        # 自动定位系统 JDK
```

**② 安装 NDK**——本机当前没有 `cmdline-tools`，二选一：

- **方式 A（命令行，推荐 CI/无界面）**：下载 commandline-tools 解压到 `$ANDROID_HOME/cmdline-tools/latest`，然后：

  ```bash
  sdkmanager --sdk_root=$ANDROID_HOME "ndk;27.2.12479018" "platform-tools"
  sdkmanager --licenses   # 已有 android-sdk-license，若提示则接受
  ```

- **方式 B（Android Studio）**：SDK Manager → SDK Tools → 勾选 `NDK (Side by side)`（勾选 “Show Package Details” 选 27.x）+ `Android SDK Command-line Tools`，点 Apply。

**③ 补 Rust Android target**（至少第一个；日常真机/模拟器只打 arm64 即可）：

```bash
rustup target add aarch64-linux-android                                   # 本机已安装
rustup target add armv7-linux-androideabi i686-linux-android x86_64-linux-android   # 需要全 ABI 时
```

### 3.3 构建前自检

```bash
java -version                      # 期望 17+
echo $JAVA_HOME $ANDROID_HOME $NDK_HOME
ls $ANDROID_HOME/ndk               # 能看到版本目录
rustup target list --installed | grep android
cd apps/backend && pnpm exec tauri info    # 观察 Environment / Packages 是否正常
```

---

## 四、生成 Android 工程：`tauri android init`

### 4.1 执行

```bash
cd apps/backend
pnpm exec tauri android init --ci --skip-targets-install
```

| 参数 | 作用 |
|------|------|
| `--ci` | 跳过交互式提问，全部用默认值 |
| `--skip-targets-install` | 跳过自动 `rustup target add`（target 已装时加速；没装时不要加此参数） |
| `-c, --config` | 合并额外 Tauri 配置（一般用不到） |

### 4.2 生成物与要点

- 产物目录：`apps/backend/src-tauri/gen/android/`，核心是 Gradle 工程（`app` 模块、`gradle wrapper`、`MainActivity.kt` / `WryActivity.kt`、`AndroidManifest.xml`）。
- `applicationId` / 包名 = `tauri.conf.json` 的 `identifier`（`com.it.assets`）。
- App 显示名 = `productName`（`资产管理平台`，中文 OK）。
- init 会基于 `icons/icon.png` 生成各分辨率 mipmap；如需换图标，先 `pnpm tauri icon <源图>` 再重新 init。
- **建议把 `gen/android` 提交到 git**：init 之后若有定制（release 签名、manifest、版本号），提交后便于团队/CI 复现；默认模板本身也可随时重生成。
- 若目录已存在会中止；需要重置时先备份再删除 `gen/android` 后重新 init。

---

## 五、构建 APK / AAB：`tauri android build`

### 5.1 常用命令矩阵

```bash
cd apps/backend

# ① release APK（仅 arm64，日常最快、真机/模拟器都能装）
pnpm exec tauri android build --apk -t aarch64

# ② release APK（全部 ABI：arm64-v8a / armeabi-v7a / x86 / x86_64）
pnpm exec tauri android build --apk

# ③ 按 ABI 拆分产出多个 APK（减小单包体积）
pnpm exec tauri android build --apk --split-per-abi

# ④ AAB（上架 Google Play 用）
pnpm exec tauri android build --aab

# ⑤ debug 包（配合 tauri android run 装机调试）
pnpm exec tauri android build --apk -d -t aarch64
```

### 5.2 CLI 参数速查（v2.11.0）

| 参数 | 说明 |
|------|------|
| `-t, --target <aarch64\|armv7\|i686\|x86_64>` | 编译哪些 ABI；缺省 = all（会要求装齐 4 个 rustup target） |
| `--apk` / `--aab` | 只打 APK / 只打 AAB（都缺省时两者都打） |
| `--split-per-abi` | 按 ABI 拆分产出包 |
| `-d, --debug` | debug 构建 |
| `-o, --open` | 构建后打开 Android Studio |
| `--ci` | 跳过交互提问 |
| `-c, --config` | 合并额外配置；也支持 `tauri.android.conf.json` 平台配置 |

### 5.3 构建内部流程与耗时

1. 先执行 `beforeBuildCommand`（本项目 = `pnpm --prefix ../web build`，产出 `apps/web/dist`）；
2. cargo 用 NDK 交叉编译 `src-tauri` 工作区全部 crate（含 `assets-*` 系列、`sqlx`、`tokio`、`aws-lc-rs` 等）；
3. Gradle `assembleRelease` 打包。

> **首次交叉编译会非常久**（本仓库依赖树大，10 ~ 30+ 分钟属正常），且会在 `src-tauri/target/` 下新增 Android 目标的增量目录。日常迭代建议固定 `-t aarch64` 只编一个 ABI。

### 5.4 产物路径

```text
# 单包（universal）
apps/backend/src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk

# --split-per-abi 后
apps/backend/src-tauri/gen/android/app/build/outputs/apk/arm64-v8a/release/app-arm64-v8a-release.apk
apps/backend/src-tauri/gen/android/app/build/outputs/apk/armeabi-v7a/release/app-armeabi-v7a-release.apk
...

# AAB
apps/backend/src-tauri/gen/android/app/build/outputs/bundle/release/app-release.aab
```

（实际文件以目录内为准，可用 `find src-tauri/gen/android -name "*.apk"` 查找。）

### 5.5 安装 / 运行

```bash
# 真机：开启“开发者选项 + USB 调试”，或启动模拟器后
adb devices

# 开发模式：自动装 debug 包并热连本机 1480（会自动 adb reverse，先跑 next dev）
pnpm exec tauri android dev

# 生产模式：把上面 release APK 装到已连接设备
pnpm exec tauri android run

# 看日志
adb logcat | grep -iE "tauri|rust|assets"
```

> 模拟器访问宿主机服务用 `10.0.2.2`；真机需在同一局域网。Android 端调试 WebView 可在电脑 Chrome 打开 `chrome://inspect`。

### 5.6 发布前必做

- **签名**：模板默认使用 debug keystore（不可上架）。正式发布需自建 keystore，在 `gen/android/app/build.gradle.kts` 配置 `signingConfigs`。
- 核对版本名/版本号（`tauri.conf.json` 的 `version` 与 gradle 中 `versionCode` / `versionName`）。
- 检查权限裁剪与隐私合规（APK 内含完整 Rust 后端，仅保留实际用到的 `capabilities` 权限）。

---

## 六、本项目特有注意事项（“能出包”≠“能跑业务”）

1. **`.env.toml` 进不了 APK**：配置加载逻辑在 `src-tauri/src/lib.rs` 的 `load_env()`，按相对路径/`CARGO_MANIFEST_DIR` 找 `.env.toml`，打包后移动端运行时读不到，只会落到“默认环境变量”分支。若要让移动端连库/调接口，需要把相关配置（PG 地址、API_PORT、token 等）在**移动端编译期注入**（如 `build.rs` + `include_str!`，或通过 Tauri 读 App 私有目录），桌面端行为保持不变。

2. **数据库必须移动端可达**：`apps/backend/.env.toml` 里 Postgres 地址若写 `localhost/127.0.0.1`，Android 上会连到手机自己。模拟器用 `10.0.2.2`（映射宿主机），真机用局域网 IP；并且要让服务端允许该来源连接。

3. **doc-parser（Python 侧车）在 Android 上不可用**：`assets-service/src/doc_parser.rs` 默认解释器是 `/home/ubuntu/conda/envs/aiagent/bin/python`，并按源码目录找 `doc-parser/main.py`——Android 上两者都不存在。代码已做 graceful 降级（启动失败只记日志返回 `None`），**不会导致崩溃**，但多模态/视频解析功能不可用；如需移动端支持，应把 doc-parser 改造成远端服务或在 Rust 侧重写。

4. **内置 HTTP API 无碍但留意明文流量**：`assets-api` 绑 `0.0.0.0`，与 App 同进程，WebView 走 `127.0.0.1` 可访问。若前端在移动端用 `fetch("http://127.0.0.1:...")` 直连 HTTP API，需确认 AndroidManifest 允许 cleartext（Android 9+ 默认禁止明文 HTTP）；优先仍走 Tauri IPC（`@tauri-apps/api` 的 `invoke`，移动端同样支持）。

5. **桌面专属配置会被忽略**：`tauri.conf.json` 的 `app.windows`（1480×800 单窗口）仅桌面生效；移动端固定单窗口 label `main`（与当前 capability 一致，无需改）。多窗口、系统托盘、全局快捷键等桌面能力在 Android 不可用。

6. **fs / dialog 等插件权限范围不同**：桌面可访问任意路径，Android 受沙箱限制（下载目录、文档目录等）。移动端用到文件选择/下载时，需在 `capabilities` 按需加 scope，并走插件提供的移动端路径 API。

7. **交叉编译阻塞风险点**：依赖中含带 C 代码的 crate（如 `aws-lc-rs` ← `jsonwebtoken`）。若报 `cc`/`linker not found`，在 `src-tauri/.cargo/config.toml` 补 NDK 工具链映射（见 7.x），或把 `jsonwebtoken` 换 `ring` / 纯 Rust 特性。

8. **包体与 ABI 建议**：真机几乎都是 arm64；日常开发固定 `-t aarch64`。对外分发想控体积用 `--split-per-abi`；想 Google Play 自动拆分用 `--aab`。

---

## 七、常见报错速查表

| 现象 | 原因 | 处理 |
|------|------|------|
| `Android SDK/NDK not found`、找不到 sdk | `ANDROID_HOME` / `NDK_HOME` 未设或未装 NDK | 按 3.2 安装 NDK 并 export，重启终端 |
| `linker 'aarch64-linux-android-clang' not found` | 缺该 ABI 的 rustup target，或工具链映射缺失 | `rustup target add ...`；或补 `.cargo/config.toml` linker |
| `Java not found` / Gradle 找不到 JDK | `JAVA_HOME` 未指向 JDK 17+ | 按 3.2① export |
| `aws-lc-sys` / `cc` 编译失败 | 交叉编译 C 代码需 NDK clang 环境 | 在 `src-tauri/.cargo/config.toml` 配置各 `aarch64-linux-android` 的 `linker`/`ar`/`cc`，或换 jsonwebtoken 特性 |
| `gen/android already exists` | 重复 init | 备份定制内容后删除 `gen/android` 再 init |
| CLI 与 crate 版本不一致报错 | `@tauri-apps/cli` 与 `tauri` crate 版本错位 | 升级对齐到同一 2.x；`--ignore-version-mismatches` 仅临时调试用 |
| `beforeBuildCommand`/next build 失败 | 前端静态导出问题 | 先单独跑 `pnpm --prefix ../web build` 复现解决 |
| 模拟器/真机白屏、连不上 devUrl | `adb reverse` 或 next dev 未就绪 | 重跑 `pnpm exec tauri android dev`；确认 `next dev` 正常、设备 USB 调试开启 |
| WebView 里 `fetch http://...` 被拦 | Android 9+ 默认禁明文 HTTP | Manifest 开启 `usesCleartextTraffic` / networkSecurityConfig 白名单，或改走 IPC/HTTPS |

---

## 八、参考链接

- Tauri v2 环境准备（含 Android 前置）：<https://v2.tauri.app/start/prerequisites/>
- Tauri v2 移动端开发：<https://v2.tauri.app/develop/>
- Tauri CLI 参考（`android init / build / dev / run`）：<https://v2.tauri.app/reference/cli/>
- Tauri 配置 schema：<https://schema.tauri.app/config/2>

---

## 附录：命令速查（完整流程）

```bash
# 1) 环境（一次性）
export ANDROID_HOME=$HOME/Android/Sdk
export NDK_HOME=$ANDROID_HOME/ndk/<版本>          # 先装 NDK
export JAVA_HOME=<JDK17+ 根目录>
rustup target add aarch64-linux-android

# 2) 生成 Android 工程
cd apps/backend
pnpm exec tauri android init --ci

# 3) 出包（arm64 release APK）
pnpm exec tauri android build --apk -t aarch64

# 4) 装机运行（先连设备/起模拟器）
pnpm exec tauri android dev      # 调试
pnpm exec tauri android run      # 运行 release
```

