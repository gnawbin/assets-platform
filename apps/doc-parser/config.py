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

# ─── Embedding ──────────────────────────────────────
# 与 Rust 端向量维度保持一致（bge-small-zh = 512，bge-base-zh = 768）
EMBEDDING_MODEL = os.getenv("EMBEDDING_MODEL", "BAAI/bge-small-zh-v1.5")
EMBEDDING_DIM = int(os.getenv("EMBEDDING_DIM", "512"))
# HF 下载端点（国内网络可设 https://hf-mirror.com，留空走官方）
HF_ENDPOINT = os.getenv("HF_ENDPOINT", "")

# ─── 文本切片 ───────────────────────────────────────
# 时间窗口切分（秒），与 FRAME_INTERVAL_SEC 对齐
CHUNK_WINDOW_SEC = int(os.getenv("CHUNK_WINDOW_SEC", "30"))
# 单个切片最大字符数，超长二次切分阈值
CHUNK_MAX_CHARS = int(os.getenv("CHUNK_MAX_CHARS", "500"))

# ─── SurrealDB（向量记忆存储）────────────────────────
# 支持远程分布式：ws://127.0.0.1:8000
# 支持嵌入式：file://./data/surrealdb 或 mem://（内存，测试用）
SURREALDB_URL = os.getenv("SURREALDB_URL", "file://./data/surrealdb")
SURREALDB_USER = os.getenv("SURREALDB_USER", "admin")
SURREALDB_PASS = os.getenv("SURREALDB_PASS", "Admin@123456")
SURREALDB_NS = os.getenv("SURREALDB_NS", "assets")
SURREALDB_DB = os.getenv("SURREALDB_DB", "knowledge")
SURREALDB_TABLE = os.getenv("SURREALDB_TABLE", "video_knowledge")

# ─── LLM（/ask RAG 问答生成）─────────────────────────
VLM_MODE = os.getenv("VLM_MODE", "ollama")  # ollama / cloud
OLLAMA_BASE_URL = os.getenv("OLLAMA_BASE_URL", "http://localhost:11434")