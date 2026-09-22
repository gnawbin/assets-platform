# doc-parser 独立部署镜像（同机 Docker 模式）
# 完整用法与排障见 docs/doc-parser 独立部署指南.md
FROM python:3.12-slim

ENV PYTHONUNBUFFERED=1 \
    PIP_NO_CACHE_DIR=1 \
    PARSER_HOST=0.0.0.0 \
    PARSER_PORT=8321

# 系统依赖：
#   ffmpeg         音频提取 / 视频抽帧（ffmpeg-python）
#   ffprobe        随 ffmpeg 一起安装；probe() 必需（imageio_ffmpeg 不含 ffprobe）
#   tesseract-ocr  扫描件 PDF / 画面 OCR，chi-sim 为中文简体语言包
#   poppler-utils  pdf2image 依赖的 pdftoppm
#   libgl1 / libglib2.0-0  Pillow 等图像库的运行时依赖
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ffmpeg \
      tesseract-ocr \
      tesseract-ocr-chi-sim \
      poppler-utils \
      libgl1 \
      libglib2.0-0 \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 先装依赖，利用构建缓存（仅 pyproject.toml 变更时重装）
COPY pyproject.toml ./
RUN pip install --no-cache-dir .

# 再复制源码（.dockerignore 已排除 tests/data/缓存等）
COPY . .

EXPOSE 8321

# slim 镜像不含 curl，用 python 自检；/health 豁免认证故无需 token。
HEALTHCHECK --interval=30s --timeout=5s --start-period=60s --retries=3 \
  CMD python -c "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8321/health', timeout=3)"

# 容器内必须监听 0.0.0.0 才能被宿主机访问；
# 因此 DOC_PARSER_TOKEN 必须非空 —— 由 B0-4b 的启动自检强制（空 token 直接拒绝启动）。
CMD ["python", "-m", "uvicorn", "main:app", "--host", "0.0.0.0", "--port", "8321"]
