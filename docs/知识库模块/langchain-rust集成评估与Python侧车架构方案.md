# langchain-rust 集成评估报告 & Python 侧车架构方案

> 最后更新：2026-07-22
> 状态：📋 评估完成，不采纳 langchain-rust
> 相关任务：P4 langchain-rust 集成评估

---

## 目录

1. [背景与评估目的](#1-背景与评估目的)
2. [评估结论](#2-评估结论)
3. [核心场景分析](#3-核心场景分析)
4. [架构方案对比](#4-架构方案对比)
5. [推荐方案：Python 解析侧车 + Rust LLM 引擎](#5-推荐方案python-解析侧车--rust-llm-引擎)
6. [技术实现细节](#6-技术实现细节)
7. [多脚本管理策略](#7-多脚本管理策略)
8. [部署说明](#8-部署说明)
9. [langchain-rust 依赖清理](#9-langchain-rust-依赖清理)
10. [附录：评估过程](#10-附录评估过程)

---

## 1. 背景与评估目的

### 1.1 评估背景

智能问答系统（RAG 多轮对话）在 P0/P1 阶段已基于自研 LLM 调用链路实现。`Cargo.toml` 中声明了 `langchain-rust = "4.6.0"` 和 `langgraph = "0.2.3"` 依赖，但**实际代码中没有任何使用**。

### 1.2 评估目标

| 目标 | 说明 |
|------|------|
| 🎯 评估 langchain-rust 是否能替代自研 LLM 调用链路 | 多厂商适配、故障转移、负载均衡 |
| 🎯 评估 langchain-rust 是否能解决文档解析（PDF/视频） | DocumentLoader、文本提取 |
| 🎯 确定是否需要保留或清理 langchain-rust 依赖 | 减少编译时间、消除死代码 |
| 🎯 如不采纳，确定替代架构方案 | Rust + Python 侧车 |

### 1.3 评估范围

| 能力域 | 评估范围 |
|--------|---------|
| LLM 多厂商调用 | OpenAI / Claude / Qwen / DeepSeek / Volcengine / Tencent / Ollama |
| 负载均衡与故障转移 | 权重随机选择、熔断器、逐个尝试 |
| Prompt 管理 | 模板构建、上下文拼接 |
| 文档解析（PDF） | 文本提取、表格保留、结构完整性 |
| 文档解析（视频） | 音频提取、语音转文字（ASR） |
| Agent / Function Calling | 工具注册、链式编排 |

---

## 2. 评估结论

### 2.1 最终结论

**❌ 不采纳 langchain-rust。**

| 结论项 | 结果 |
|--------|------|
| langchain-rust 是否可替代自研 LLMRouter？ | ❌ 不能（自研更强） |
| langchain-rust 是否可解决 PDF 解析？ | ❌ 不提供此能力 |
| langchain-rust 是否可解决视频解析？ | ❌ 完全不相关 |
| langchain-rust 是否可减少开发量？ | ❌ 不能 |
| **是否移除 langchain-rust 依赖？** | **✅ 从 Cargo.toml 移除** |

### 2.2 评估维度对比

| 对比维度 | 自研实现（LLMRouter） | langchain-rust |
|----------|----------------------|----------------|
| 多厂商支持 | ✅ 7+ 厂商（OpenAI/Claude/Qwen/DeepSeek/Volcengine/Tencent/Ollama） | ⚠️ 支持 OpenAI 格式，国内厂商需自定义 |
| 故障转移 | ✅ 逐个尝试直到成功 | ❌ 需自行实现 |
| 熔断器 | ✅ 配置失败阈值 + 恢复超时 | ❌ 需自行实现 |
| 负载均衡 | ✅ 权重随机选择 | ❌ 需自行实现 |
| Provider 动态管理 | ✅ 从 `llm_model` 表加载 | ❌ 需自行实现 |
| PDF 解析 | ❌ 需额外 crate | ❌ 不提供 |
| 视频解析 | ❌ 完全不支持 | ❌ 完全不支持 |
| 文本切片 | ✅ 自研 TextChunker | ✅ 有 TextSplitter（但自研已够用） |
| Chain 链式调用 | ⚠️ 手动编排 | ✅ 有 Chain 抽象 |
| Agent / Tool | ❌ 未实现 | ✅ 有抽象 |
| 编译时间 | ✅ 轻量 | ❌ 增加 150+ crate |
| 代码维护 | 自行维护 | 社区维护（Rust 生态较小） |

### 2.3 关于 langgraph 的说明

`langgraph = "0.2.3"` 依赖同样未使用。langgraph 用于构建有状态的多步骤 Agent 工作流（状态图）。当前项目尚无 Agent 需求，故一并移除。

---

## 3. 核心场景分析

### 3.1 场景一：对话中上传 PDF 让大模型推理

```
用户提问 + 上传 PDF
  ↓
提取 PDF 文本
  ↓
文本 + 问题送入 LLM
  ↓
LLM 推理回答
```

| 子步骤 | Rust 实现方案 | Python 实现方案 |
|--------|-------------|----------------|
| PDF 文本提取 | `pdf-extract` crate（简单文本，表格结构丢失） | `PyMuPDF`（保留表格/结构） |
| LLM 推理 | 自研 LLMRouter（7+厂商+负载均衡+熔断器） | Python langchain（需重新配置厂商） |

**问题：** Rust 的 PDF 解析库（`pdf-extract`/`lopdf`）对于中文 PDF、表格、多栏排版的支持较弱，提取的文本会丢失大量结构信息。LLM 推理质量受输入质量直接影响。

### 3.2 场景二：对话中上传视频让大模型推理

```
用户提问 + 上传视频
  ↓
提取音频轨道
  ↓
语音转文字（Whisper）
  ↓
文字 + 问题送入 LLM
  ↓
LLM 推理回答
```

| 子步骤 | Rust 实现 | Python 实现 |
|--------|-----------|------------|
| 音视频分离 | ❌ 无成熟 crate | ✅ ffmpeg-python |
| 语音转文字 | ❌ whisper-rs 不稳定 | ✅ faster-whisper |
| LLM 推理 | ✅ 自研 LLMRouter | ⚠️ 需重新配置 |

**问题：** Rust 生态在视频处理和 ASR 领域几乎没有可行的方案。Python 是事实标准（ffmpeg + whisper）。

### 3.3 场景三：纯 LLM 调用（无附件）

这是当前已实现的场景，自研 `LLMRouter` 完全胜任，不需要 langchain-rust。

### 3.4 各语言在文档/AI 领域的能力矩阵

| 能力 | Python | Rust | Java | C++ |
|------|--------|------|------|-----|
| PDF 解析 | ✅ PyMuPDF 最强 | ⚠️ pdf-extract 勉强可用 | ⚠️ PDFBox 可用 | ✅ poppler |
| 视频解析 | ✅ ffmpeg-python | ❌ 无成熟方案 | ⚠️ JavaCV | ✅ ffmpeg 原生 |
| 语音识别 | ✅ faster-whisper | ❌ whisper-rs 不稳定 | ❌ 无 | ⚠️ whisper.cpp |
| LLM 调用链 | ✅ langchain | ❌ 生态太小 | ⚠️ langchain4j | ❌ 无 |
| AI Agent | ✅ 成熟 | ❌ 萌芽 | ⚠️ 发展中 | ❌ 无 |
| 二进制部署 | ❌ 需打包 | ✅ 原生编译 | ✅ JAR | ✅ 原生编译 |

**结论：Python 在非结构化文档/AI 领域不可替代。**

---

## 4. 架构方案对比

### 4.1 方案总览

| 方案 | 描述 | PDF 解析 | 视频解析 | LLM 调用 | 部署复杂度 | 推荐度 |
|------|------|---------|---------|---------|-----------|-------|
| **A: 纯 Rust 自研** | 全部用 Rust | ⚠️ 勉强 | ❌ 不行 | ✅ 已有 | 低 | ⚠️ |
| **B: Python 解析侧车** | Python 只做解析，LLM 回 Rust | ✅ | ✅ | ✅ **复用** | 中（Sidecar 无感） | **🥇 推荐** |
| **C: Python langchain 全权** | Python 做全套 | ✅ | ✅ | ❌ 链路割裂 | 中 | ❌ |
| **D: 多模态 LLM** | PDF/视频转图片送视觉模型 | ✅ | ✅ | ✅ **复用** | 低 | ✅ 模型支持时可选 |
| **E: PyO3 嵌入 Python** | Rust 进程内嵌 Python | ✅ | ✅ | ✅ **复用** | 高（打包噩梦） | ❌ |

### 4.2 方案 B 详细评估（推荐）

```
Rust (Tauri 主进程)
  │
  ├─ 对话服务 (conversation_service.rs)
  │    └─ 判断附件类型
  │         ├─ .txt/.md → 直接读取
  │         ├─ .pdf     → 调 Python 解析
  │         ├─ .mp4     → 调 Python 解析
  │         └─ 其他     → 调 Python 解析
  │
  ├─ LLMRouter (统一调用入口)
  │    └─ 负载均衡 + 熔断器 + 故障转移
  │         ├─ OpenAI / Claude / Qwen
  │         ├─ DeepSeek / Volcengine / Tencent
  │         └─ Ollama
  │
  └─ Tauri Sidecar / HTTP ──→ Python 解析服务
                                ├─ PDF → PyMuPDF
                                ├─ 视频 → ffmpeg + whisper
                                ├─ 音频 → whisper
                                └─ DOCX → python-docx
```

**关键原则：Python 只做"非结构化 → 文本"的转换，LLM 调用统一走 Rust。**

### 4.3 方案 C 为什么不推荐

Python langchain 如果做全套，会导致 LLM 调用链路割裂：

```
Rust 侧打磨的 LLM 基础设施 (7+厂商+负载均衡+熔断器)
  ↓ 被跳过
Python langchain 需重新配置厂商/密钥/模型
  ↓ 两套配置，维护成本翻倍
不一致时难以排查
```

### 4.4 方案 E (PyO3) 为什么不推荐

| 问题 | 说明 |
|------|------|
| 部署 | 需捆绑 Python DLL + site-packages + .pyd 文件，打包体积增加 200MB+ |
| 安全性 | Python 段错误直接拖垮 Tauri 主进程 |
| GPU 冲突 | Rust 和 Python 可能竞争 GPU 资源 |
| Tauri 生态 | 几乎没有 PyO3 + Tauri 的实践 |
| 模型内存 | whisper 模型 ~2GB，常驻 Rust 进程内存 |

---

## 5. 推荐方案：Python 解析侧车 + Rust LLM 引擎

### 5.1 两种调用方式不能混淆

| 方式 | 使用什么 Rust 工具 | Python 形态 | 每次调用开销 |
|------|-------------------|------------|-------------|
| **Sidecar (CLI 进程)** | **`tauri-plugin-shell`** → `.sidecar("doc-parser")` | PyInstaller 打包的独立 .exe | ~50ms 启动进程 |
| **FastAPI (HTTP 服务)** | **`reqwest`**（项目已有） → `Client::post(...)` | 常驻 HTTP 服务 (`uvicorn`) | ~1ms HTTP 请求 |

**`tauri-plugin-shell` 不是用来调 HTTP 接口的。** 它的作用是"启动一个外部程序并传参"，跟你在命令行里执行 `python main.py --input file.pdf` 完全一样。

选择哪个决定了：

```
┌─ 选 Sidecar → 用 tauri-plugin-shell
│   用户装一个安装包，Python 被打包成 .exe 在里面
│   Rust 通过 shell.sidecar() 启动进程
│   ✅ 用户无感，不担心"两个进程部署麻烦"
│
└─ 选 FastAPI → 用 reqwest
    Python 服务独立运行 (127.0.0.1:8765)
    Rust 通过 reqwest 发 HTTP 请求
    ❌ 需要额外启动 Python 服务
```

### 5.2 进程通信方式对比

| 方式 | 延迟 | 进程管理 | 适合场景 | 复杂度 |
|------|------|---------|---------|--------|
| **Tauri Sidecar CLI** | ~50ms（每次启动） | Tauri 自动管理 | 偶尔调用（如对话中传文件） | 低 |
| **HTTP FastAPI** | ~1ms（常驻） | 系统服务 / 容器 | 频繁调用（如流式处理） | 中 |
| **stdin/stdout 流** | ~30ms | Tauri 自动管理 | 大文件免写磁盘 | 低 |

### 5.3 推荐：Tauri Sidecar（优先）

Tauri 2.x 的 Sidecar 机制可以将 Python 打包进安装包，随主进程自动启动/停止，用户无感知。

需要的已有依赖：

| 组件 | 状态 |
|------|------|
| `tauri = "2.11.5"` | ✅ 已有 |
| `tauri-plugin-shell = "2.3.5"` | ✅ 已有 |
| `tauri.conf.json` externalBin | ❌ 需配置 |
| `capabilities/default.json` shell 权限 | ❌ 需配置 |
| `lib.rs` 注册 shell 插件 | ❌ 需配置 |

### 5.3 备选：HTTP FastAPI

如果 Sidecar 每次启动的开销不可接受，可以用 FastAPI 作为长驻服务：

- Python 依赖：`fastapi` + `uvicorn` + `PyMuPDF` + `faster-whisper`
- 端口：`127.0.0.1:8765`（仅本地监听）
- 部署：独立进程 / 容器 / Tauri Sidecar 管理

---

## 6. 技术实现细节

### 6.1 Tauri Sidecar 配置

**tauri.conf.json 修改：**

```json
{
  "bundle": {
    "externalBin": ["binaries/doc-parser"],
    "active": true,
    "targets": "all"
  }
}
```

**Rust 调用代码：**

```rust
use tauri_plugin_shell::ShellExt;

#[tauri::command]
async fn parse_document(
    app: tauri::AppHandle,
    file_type: String,   // "pdf" | "video" | "audio" | "docx"
    file_path: String,
) -> Result<String, String> {
    let output = app
        .shell()
        .sidecar("doc-parser")
        .map_err(|e| format!("启动解析器失败: {}", e))?
        .args(["--type", &file_type, "--input", &file_path])
        .output()
        .await
        .map_err(|e| format!("解析失败: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}
```

**Python 统一入口：**

```python
#!/usr/bin/env python3
import argparse
import sys

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--type", required=True, choices=["pdf", "video", "audio", "docx"])
    parser.add_argument("--input", required=True)
    args = parser.parse_args()

    if args.type == "pdf":
        from parsers import pdf_parser
        result = pdf_parser.parse(args.input)
    elif args.type == "video":
        from parsers import video_parser
        result = video_parser.parse(args.input)
    elif args.type == "audio":
        from parsers import audio_parser
        result = audio_parser.parse(args.input)
    elif args.type == "docx":
        from parsers import docx_parser
        result = docx_parser.parse(args.input)

    print(result)

if __name__ == "__main__":
    main()
```

### 6.2 HTTP FastAPI 调用

```python
# Python FastAPI 服务
from fastapi import FastAPI
from pydantic import BaseModel
import fitz  # PyMuPDF

app = FastAPI()

class ParseRequest(BaseModel):
    file_path: str

class ParseResponse(BaseModel):
    text: str
    pages: int

@app.post("/parse/pdf", response_model=ParseResponse)
async def parse_pdf(req: ParseRequest):
    doc = fitz.open(req.file_path)
    text = "\n".join([page.get_text() for page in doc])
    return ParseResponse(text=text, pages=len(doc))

@app.post("/parse/video")
async def parse_video(req: ParseRequest):
    # ffmpeg 提取音频 → whisper ASR → 返回文本
    ...

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="127.0.0.1", port=8765)
```

```rust
// Rust HTTP 调用
#[tauri::command]
async fn parse_document(file_type: String, file_path: String) -> Result<String, String> {
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:8765/parse/{}", file_type);

    let resp = client
        .post(&url)
        .json(&serde_json::json!({"file_path": file_path}))
        .send()
        .await
        .map_err(|e| format!("调用解析服务失败: {}", e))?;

    let result = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("解析响应失败: {}", e))?;

    Ok(result["text"].as_str().unwrap_or("").to_string())
}
```

---

## 7. 多脚本管理策略

### 7.1 推荐：一个 Sidecar + 参数路由

```
binaries/
  doc-parser.exe        ← PyInstaller 打包的统一入口
  parsers/
    __init__.py
    pdf_parser.py        ← PyMuPDF
    video_parser.py      ← ffmpeg + faster-whisper
    audio_parser.py      ← faster-whisper
    docx_parser.py       ← python-docx
    ocr_parser.py        ← paddleocr / tesseract
```

**只有一个 `externalBin` 配置项，传输 `--type` 参数区分。**

### 7.2 利用现有 Skill 系统（项目已有）

`skill_registry.rs` 已注册的与文档解析相关的 Skill：

| Skill ID | 名称 | 文件路径 |
|----------|------|---------|
| `doc-parse` | 文档解析 | `skills/builtin/doc_parse.py` |
| `ocr-image` | 图片 OCR | `skills/builtin/ocr_image.py` |

将来这些 Skill 的 `file_path` 可以指向 Python 侧车脚本，通过 `execute_skill` command 统一调度。

---

## 8. 部署说明

### 8.1 Sidecar 打包流程

```bash
# 1. Python 项目目录结构
python-parser/
  main.py
  parsers/
    __init__.py
    pdf_parser.py
    video_parser.py
  requirements.txt    # PyMuPDF, faster-whisper, ffmpeg-python

# 2. PyInstaller 打包
cd python-parser
pip install pyinstaller
pyinstaller --onefile main.py --name doc-parser

# 3. 复制到 Tauri 项目
cp dist/doc-parser.exe ../apps/backend/src-tauri/binaries/doc-parser-x86_64-pc-windows-msvc.exe
```

### 8.2 Tauri 打包

```bash
# 正常打包即可，Sidecar 会自动包含
cd apps/backend/src-tauri
cargo tauri build
```

生成单个安装包，用户安装后自动包含 Python 解析器。

### 8.3 ffmpeg 依赖处理

| 方案 | 说明 |
|------|------|
| 系统安装 | 用户自行安装 ffmpeg 并加入 PATH |
| 捆绑 Sidecar | 将 ffmpeg.exe 作为另一个 Sidecar |
| 按需下载 | 首次解析视频时提示下载 |

---

## 9. langchain-rust 依赖清理

### 9.1 清理内容

从 `apps/backend/src-tauri/Cargo.toml` 移除：

```diff
- langchain-rust = "4.6.0"
- langgraph = "0.2.3"
```

### 9.2 理由

| 检查项 | 结果 |
|--------|------|
| 源码中是否有 `use langchain` 或 `use langgraph` | ❌ 无 |
| `grep -r "langchain\|langgraph" *.rs` | ❌ 零匹配 |
| 是否有计划使用 | ❌ 评估结论：不采纳 |
| 移除后是否影响编译 | ✅ 不影响，纯死依赖 |

---

## 10. 附录：评估过程

### 10.1 参与讨论的关键场景

| 讨论点 | 结论 |
|--------|------|
| langchain-rust 替代自研 LLM 调用 | ❌ 自研更强（多厂商+负载均衡+熔断器） |
| langchain-rust 解决 PDF 解析 | ❌ 不提供此能力 |
| langchain-rust 解决视频解析 | ❌ 完全不相关 |
| Rust 调用 Python langchain | ❌ LLM 链路割裂 |
| Rust 调用 Python 仅做解析 | ✅ 推荐方案 |
| PyO3 嵌入 Python | ❌ 部署复杂、进程不安全 |
| Tauri Sidecar 管理 Python 进程 | ✅ 最佳方案 |
| FastAPI 独立服务 | ✅ 备选方案 |

### 10.2 相关文档

- [智能问答系统（RAG多轮对话）设计方案](./智能问答系统（RAG多轮对话）设计方案.md)
- [知识库与知识图谱架构设计](../知识库与知识图谱架构设计.md)

### 10.3 相关代码文件

| 文件 | 说明 |
|------|------|
| `apps/backend/src-tauri/src/service/llm_gateway_service.rs` | LLM 网关核心（LLMRouter/负载均衡/熔断器） |
| `apps/backend/src-tauri/src/service/conversation_service.rs` | 对话服务（RAG + LLM 调用） |
| `apps/backend/src-tauri/src/service/rag_service.rs` | RAG 检索引擎 |
| `apps/backend/src-tauri/src/engine/skill_registry.rs` | Skill 注册表（含 doc-parse） |

---

> 本文档对应 P4 任务 "langchain-rust 集成评估"。
> 评估结论：❌ 不采纳 langchain-rust，推荐 Python 解析侧车 + Rust LLM 引擎架构。