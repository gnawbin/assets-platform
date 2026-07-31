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

# ─── 认证 ───────────────────────────────────────────
# Tauri 启动 doc-parser 时注入的动态密钥（不要写死在 .env）
# 见 docs/知识库模块/文档解析与RAG记忆链路设计方案.md 第 11 章
API_TOKEN = os.getenv("DOC_PARSER_TOKEN", "")

# ─── Whisper ────────────────────────────────────────
WHISPER_MODEL = os.getenv("WHISPER_MODEL", "base")  # tiny/base/small/medium/large

# ─── OCR ────────────────────────────────────────────
OCR_LANGUAGE = os.getenv("OCR_LANGUAGE", "chi_sim+eng")

# ─── 视频 ───────────────────────────────────────────
FRAME_INTERVAL_SEC = int(os.getenv("FRAME_INTERVAL_SEC", "30"))
