# doc-parser — 多模态文档解析服务

一个专注于「非文本 → 纯文本」转换的轻量解析引擎。

通过 HTTP API 为 Tauri/Rust 后端提供文档解析能力，支持 PDF、图片、音频、视频四种模态。

## 快速启动

```bash
# 1. 安装依赖
pip install -r requirements.txt

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
├── requirements.txt     # 依赖清单
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