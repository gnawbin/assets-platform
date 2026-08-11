# doc-parser — 多模态文档解析服务

一个专注于「非文本 → 纯文本」转换的轻量解析引擎。

通过 HTTP API 为 Tauri/Rust 后端提供文档解析能力，支持 PDF、图片、音频、视频四种模态。

## 快速启动

```bash
# 1. 安装依赖
pip install -e .

# 2. 复制配置
cp .env.example .env

# 3. 启动
python -m uvicorn main:app --host 127.0.0.1 --port 8321 --reload

# 4. 测试
curl http://127.0.0.1:8321/health
```

## API

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/parse` | 解析文件，返回纯文本 |
| GET | `/health` | 健康检查 |
| GET | `/formats` | 支持的文件格式 |

## 项目结构

```
doc-parser/
├── main.py              # FastAPI 入口
├── config.py            # 环境配置
├── pyproject.toml       # 依赖清单
├── models/              # 数据模型
├── parsers/             # 解析器（PDF/图片/音频/视频）
├── vlm/                 # VLM 客户端（Ollama + 云端）
├── utils/               # 工具函数
└── tests/               # 测试
```

## 配置

通过环境变量或 `.env` 文件配置：

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `VLM_MODE` | `ollama` | VLM 模式：ollama/cloud |
| `OLLAMA_BASE_URL` | `http://localhost:11434` | Ollama 地址 |
| `WHISPER_MODEL` | `base` | Whisper 模型大小 |
| `OCR_LANGUAGE` | `chi_sim+eng` | OCR 语言 |

## 视频解析测试（集成测试）

`tests/test_video_parse.py` 验证视频 → 文本的完整链路：
ffmpeg 提取音频 → Whisper 转写 → 定时抽帧，转写文本保存到 `tests/output/` 下。

### 运行前置条件（Linux/WSL 环境）

| 依赖 | 说明 |
|------|------|
| Python 环境 | `aiagent` conda 环境：`whisper`、`ffmpeg-python`、`pytest`、`pytest-asyncio` 已安装 |
| `ffmpeg` + `ffprobe` | 必须同时具备。注意：`imageio_ffmpeg` 只提供 `ffmpeg`，**不含 `ffprobe`**，而 `ffmpeg-python` 的 `probe()` 需要 `ffprobe` |

`PATH` 环境变量会同时影响 `probe()`（ffprobe）与音频提取/抽帧（ffmpeg），运行时须将两者目录注入 PATH。

### 运行命令

```bash
cd apps/doc-parser

# 将编译好的 ffmpeg/ffprobe 注入 PATH（以 ~/ffmpeg-build 为例）
export PATH="$HOME/ffmpeg-build/bin:$PATH"

# 禁用系统 ROS 插件（launch_testing 等与 pytest 9 不兼容），仅启用 pytest-asyncio
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 \
  /home/ubuntu/conda/envs/aiagent/bin/python -m pytest tests/test_video_parse.py -v -p asyncio
```

> 测试中的 `VIDEO_FILE` 为绝对路径，需按本机实际视频位置修改（当前环境示例：`/mnt/c/project/ceshi.mp4`）。

### 常见问题

| 现象 | 原因 | 解决 |
|------|------|------|
| `FileNotFoundError: 'ffprobe'` | `ffmpeg-python` 的 `probe()` 依赖 `ffprobe`，但 `imageio_ffmpeg` 只带 `ffmpeg` | 安装完整 ffmpeg（含 ffprobe），如 `~/ffmpeg-build/bin`，并注入 `PATH` |
| `PluginValidationError: unknown hook 'pytest_launch_collect_makemodule'` | 系统 `/opt/ros` 的 `launch_testing` / `launch_testing_ros_pytest_entrypoint` 插件与 pytest 9 不兼容 | 使用 `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1` 禁用插件自动加载，并 `-p asyncio` 手动加载 pytest-asyncio |
| `UserWarning: FP16 is not supported on CPU` | Whisper 在 CPU 上自动回退 FP32 | 正常提示，不影响结果 |
| Windows 路径 `D:\xxx.mp4` 不可用 | WSL 下需使用 `/mnt/d/...` 映射路径 | 将 `VIDEO_FILE` 改为 WSL 路径 |

### 一次校验参考

```text
tests/test_video_parse.py::TestVideoParse::test_video_file_exists PASSED [ 50%]
tests/test_video_parse.py::TestVideoParse::test_video_parse_to_text PASSED [100%]

=================== 2 passed, 1 warning in 172.00s (0:02:52) ===================
```

## 视频全量转文本与向量化记忆

视频 "一次上传、永久记忆" 的完整链路（语音转写 + 画面 OCR → 切片 → 向量化入库 → RAG 问答），见：

📄 `docs/知识库模块/视频全量转文本与向量化记忆设计方案.md`

核心链路：

```
视频上传 → VideoParser(Whisper语音 + 帧OCR画面)
        → TextChunker 切片（带时间戳）
        → Embedding 向量化（bge-zh）
        → LanceDB 落盘（记忆持久化，SHA-256 幂等去重）
        → RAG 问答：问题 → 检索相关切片 → LLM 生成回答（引用视频时间点）
```

关键收益：

- **无需重复上传**：同一视频重复上传通过 SHA-256 命中后直接跳过解析；
- **画面信息不再丢失**：PPT/字幕经 OCR 提取为可检索文本；
- **时间回溯**：问答结果可定位到视频具体时间段；实现优先级：P0（帧 OCR + 切片）→ P1（向量化入库 + 幂等）→ P2（/search + /ask 接口）。
