"""
doc-parser 配置模块
所有配置通过环境变量注入，支持 .env 文件。
"""

import os
from dotenv import load_dotenv

load_dotenv()

# ─── 服务 ───────────────────────────────────────────
PARSER_HOST = os.getenv("PARSER_HOST", "127.0.0.1")
PARSER_PORT = int(os.getenv("PARSER_PORT", "8321"))

# ─── VLM 模式 ───────────────────────────────────────
# ollama: 本地 Ollama
# cloud:  云端 API（经 Rust llm_gateway_service 中转）
VLM_MODE = os.getenv("VLM_MODE", "ollama")

# Ollama 配置
OLLAMA_BASE_URL = os.getenv("OLLAMA_BASE_URL", "http://localhost:11434")
OLLAMA_VLM_MODEL = os.getenv("OLLAMA_VLM_MODEL", "llava")  # 或 llama3.2-vision

# Rust 后端 LLM 网关地址（云端 VLM 模式用）
RUST_GATEWAY_URL = os.getenv("RUST_GATEWAY_URL", "http://127.0.0.1:1420")

# ─── Whisper ────────────────────────────────────────
WHISPER_MODEL = os.getenv("WHISPER_MODEL", "base")  # tiny/base/small/medium/large

# ─── OCR ────────────────────────────────────────────
OCR_LANGUAGE = os.getenv("OCR_LANGUAGE", "chi_sim+eng")

# ─── 视频 ───────────────────────────────────────────
FRAME_INTERVAL_SEC = int(os.getenv("FRAME_INTERVAL_SEC", "30"))