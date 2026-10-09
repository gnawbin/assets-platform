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

# 可选：pip 镜像源（国内构建加速）。默认走官方 PyPI，不改变默认行为。
# 覆盖示例：
#   docker compose ... build \
#     --build-arg PIP_INDEX_URL=https://pypi.tuna.tsinghua.edu.cn/simple \
#     --build-arg PIP_TRUSTED_HOST=pypi.tuna.tsinghua.edu.cn
ARG PIP_INDEX_URL=
ARG PIP_TRUSTED_HOST=
RUN if [ -n "$PIP_INDEX_URL" ]; then \
      printf '[global]\nindex-url = %s\ntrusted-host = %s\n' \
        "$PIP_INDEX_URL" "$PIP_TRUSTED_HOST" > /etc/pip.conf; \
    fi

# 先装第三方依赖，利用构建缓存（仅 pyproject.toml 变更时重装）。
# 注意：不能在此处直接 `pip install .` —— 此时源码尚未复制，
# 而 pyproject.toml 的 [tool.setuptools] packages 显式列出了 models/parsers/...，
# setuptools 会因「package directory does not exist」而失败。
# 故先从 pyproject.toml 抽取 project.dependencies 单独安装依赖层。
#
# 依赖含 torch / cuda-* 等上百 MB 级 wheel（合计数 GB），而镜像源（尤其国内镜像）
# 对 wheel 响应不做 HTTP 缓存，一旦某个文件读超时，整层构建就得从零重下。
# 因此拆成「先下载、再离线安装」两步，并把 wheel 目录挂到 BuildKit cache mount：
#   1) pip download -d /wheels：cache mount 不写入镜像层；
#      重试时已下载的 wheel 会被跳过（pip 输出 File was already downloaded），
#      只补缺失的，代价从「重下数 GB」降到「补几个文件」；
#   2) pip install --no-index --find-links /wheels：纯本地安装，不再受网络影响；
#   3) --timeout / --retries 放宽单文件下载超时与重试次数。
COPY pyproject.toml ./
RUN --mount=type=cache,target=/wheels \
    python -c "import tomllib; print('\n'.join(tomllib.load(open('pyproject.toml','rb'))['project']['dependencies']))" > /tmp/requirements.txt \
 && pip download --no-cache-dir --timeout 120 --retries 20 -d /wheels -r /tmp/requirements.txt \
 && pip install --no-cache-dir --no-index --find-links /wheels -r /tmp/requirements.txt \
 && rm -f /tmp/requirements.txt

# 再复制源码（.dockerignore 已排除 tests/data/缓存等）并安装本包本体
# （--no-deps：依赖已在上一层安装，避免重复解析与下载）
COPY . .
RUN pip install --no-cache-dir --no-deps .

EXPOSE 8321

# slim 镜像不含 curl，用 python 自检；/health 豁免认证故无需 token。
HEALTHCHECK --interval=30s --timeout=5s --start-period=60s --retries=3 \
  CMD python -c "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8321/health', timeout=3)"

# 容器内必须监听 0.0.0.0 才能被宿主机访问；
# 因此 DOC_PARSER_TOKEN 必须非空 —— 由 B0-4b 的启动自检强制（空 token 直接拒绝启动）。
CMD ["python", "-m", "uvicorn", "main:app", "--host", "0.0.0.0", "--port", "8321"]
