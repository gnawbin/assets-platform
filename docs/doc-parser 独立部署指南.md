# doc-parser 独立部署指南

> **版本**：v1.0 ｜ **编制日期**：2026-09-22
> **适用对象**：`apps/doc-parser`（FastAPI 多模态解析服务）+ `apps/backend`（Rust/Tauri 后端，作为调用方）
> **本次范围**：**同机**（Docker 与 systemd 两种形态）。跨机/集中部署**暂不做**，后续事项见 §10
> **关联**：`docs/架构问题与缺陷分析及整改排期.md`（P0-6 / B0-4a / B0-4b / B0-6）

---

## 1. 部署模式与 token 语义

| 模式 | 进程归属 | 监听地址 | token 来源 | 状态 |
|---|---|---|---|---|
| **A 内嵌自启动** | 后端应用拉起 uvicorn（`DOC_PARSER_ENABLED=true`） | `127.0.0.1:8321` | 后端启动时生成随机 UUID，注入自身与子进程 | 现有可用路径 |
| **B 独立进程（systemd）** | 部署方（systemd 守护） | `127.0.0.1:8321`（不对外） | **部署方配置静态 token** | 本文 §4 |
| **C 独立容器（Docker）** | 部署方（Docker 守护） | 容器内 `0.0.0.0`，宿主机发布到 `127.0.0.1:8321` | **部署方配置静态 token（强制非空）** | 本文 §5 |

> ⚠️ **关于"不要写死 token"的说明**：`apps/doc-parser/.env.example` 原有注释"生产环境不要在这里写死（写死会退化为静态密钥方案）"**仅适用于模式 A**。在模式 B/C 下，独立进程无法接收到后端注入的随机值，**静态共享密钥是唯一可行的鉴权方式**，此时应把 token 视为服务凭据，按"配置项 + 600 权限 + 定期轮换"管理。

```
┌──────────────────────────────┐         ┌──────────────────────────────┐
│  后端（Tauri / axum）         │  HTTP   │  doc-parser（systemd/Docker）│
│  DocParserClient             │ ──────▶ │  uvicorn main:app            │
│  base_url ← doc_parser.url   │ X-API-  │  auth_middleware 校验         │
│  token    ← doc_parser.token │ Token   │  /parse /search /ask ...     │
└──────────────────────────────┘         └──────────────────────────────┘
        │                                       │
        │ S3 下载附件 → 本地临时路径              │ Whisper / ffmpeg / OCR / Embedding
        ▼                                       ▼
   本地临时目录（RAII 清理）                SurrealDB（向量记忆）· Ollama（/ask）
```

**接口清单**（`apps/doc-parser/controllers/`，`/health` 与 Swagger 端点豁免鉴权）

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/parse` | 解析单个文件（聊天附件链路使用，`skip_index=true` 仅解析不向量化） |
| POST | `/parse/batch` | 批量解析 |
| GET | `/health` | 健康检查（豁免认证） |
| GET | `/formats` | 支持的文件格式 |
| POST | `/search` | 向量检索视频切片（RAG） |
| POST | `/ask` | 检索 + LLM 生成回答（RAG，依赖 Ollama/云的 LLM） |
| POST | `/workflow/execute` | 执行 AI 工作流 |

---

## 2. 前置条件

| 依赖 | 要求 | 备注 |
|---|---|---|
| Python | ≥ 3.10（推荐 3.12） | `pyproject.toml` `requires-python = ">=3.10"` |
| **ffmpeg + ffprobe** | 两者都必须存在 | `imageio_ffmpeg` **只含 ffmpeg 不含 ffprobe**，而 `ffmpeg-python` 的 `probe()` 依赖 ffprobe（`README.md:97-102`） |
| tesseract-ocr | 含中文语言包 | 扫描件 PDF / 视频画面 OCR（`OCR_LANGUAGE=chi_sim+eng`） |
| poppler-utils | `pdftoppm` | `pdf2image` 依赖 |
| SurrealDB | 推荐独立实例 `ws://127.0.0.1:8000` | 默认嵌入式 `file://./data/surrealdb` 相对 CWD 落盘，容器内为临时层，**必须挂卷或改用外部实例** |
| Ollama 或云端 LLM | `/ask` 与工作流节点需要 | `VLM_MODE=ollama` + `OLLAMA_BASE_URL` |
| 磁盘/内存 | Whisper 模型与 Embedding 模型缓存较大 | 缓存目录需持久化（`~/.cache` / `/root/.cache`） |

---

## 3. 环境变量全表

> 以下均为 `apps/doc-parser/config.py` **实际读取**的变量（对照源码行号见备注列）。

| 变量 | 默认值 | 说明 |
|---|---|---|
| `PARSER_HOST` | `127.0.0.1` | 监听地址。**容器内必须是 `0.0.0.0`**；`0.0.0.0` 时 token 必须非空（`config.py:12`） |
| `PARSER_PORT` | `8321` | 监听端口（`config.py:13`） |
| `DOC_PARSER_TOKEN` | `""` | 共享密钥。独立部署**必填**；需与后端 `[doc_parser] token` 完全一致（`config.py:18`） |
| `DOC_PARSER_ALLOW_NO_TOKEN` | `0` | **待实现（B0-4b）**：置 `1` 才允许空 token 启动，仅限本机调试 |
| `WHISPER_MODEL` | `base` | tiny/base/small/medium/large（`config.py:21`） |
| `OCR_LANGUAGE` | `chi_sim+eng` | tesseract 语言（`config.py:24`） |
| `FRAME_INTERVAL_SEC` | `30` | 视频抽帧间隔（`config.py:27`） |
| `EMBEDDING_MODEL` | `BAAI/bge-small-zh-v1.5` | 需与后端向量维度一致（`config.py:31`） |
| `EMBEDDING_DIM` | `512` | bge-small-zh=512，bge-base-zh=768（`config.py:32`） |
| `HF_ENDPOINT` | `""` | HuggingFace 端点，国内建议 `https://hf-mirror.com`（`config.py:34`） |
| `CHUNK_WINDOW_SEC` | `30` | 切片时间窗，与 `FRAME_INTERVAL_SEC` 对齐（`config.py:38`） |
| `CHUNK_MAX_CHARS` | `500` | 单切片最大字符数（`config.py:40`） |
| `SURREALDB_URL` | `file://./data/surrealdb` | 支持 `ws://host:8000` / `file://` / `mem://`（`config.py:45`） |
| `SURREALDB_USER` / `SURREALDB_PASS` | `admin` / `Admin@123456` | **默认口令必须改**（`config.py:46-47`） |
| `SURREALDB_NS` / `SURREALDB_DB` / `SURREALDB_TABLE` | `assets` / `knowledge` / `video_knowledge` | 命名空间与表（`config.py:48-50`） |
| `VLM_MODE` | `ollama` | `ollama` / `cloud`（`config.py:53`） |
| `OLLAMA_BASE_URL` | `http://localhost:11434` | 容器内指不到宿主机，需改（见 §5.3） |
| `OLLAMA_MODEL` | `qwen2.5:7b` | 工作流 LLM 节点默认模型（`config.py:55`） |

**后端侧对应变量**（`apps/backend/.env.toml` 的 `[doc_parser]`，经 `load_env()` 映射为大写 `DOC_PARSER_*`）

| 键 | 当前代码是否生效 | 说明 |
|---|---|---|
| `enabled` | ✅ 生效 | `true` 时后端尝试拉起 uvicorn（模式 A） |
| `python` / `host` / `port` | ✅ 生效（仅拉起时） | 独立部署时不再使用 |
| `autostart` | ⏳ **待实现（B0-4a）** | 目前**不生效**（全仓无读取处）；当前用 `enabled = false` 表达"只做客户端，不管进程" |
| `url` | ✅ **已生效** | 经 `load_env()` 的 `SECTION_KEY` 映射为 `DOC_PARSER_URL`（`src/lib.rs:54-65`，调用顺序 `:92 → :96`）；`host/port` 对其无效 |
| `token` | ✅ **已生效** | 同上映射为 `DOC_PARSER_TOKEN`；⚠️ **模式 A（`enabled=true` 且端口空闲）会用随机 UUID 静默覆盖该值**，独立部署须确保走 B/C 分支（`enabled=false`，或对端已占用端口） |
| `timeout_secs` / `health_path` | ⏳ **待实现（B0-4a）** | 当前**不生效**：客户端硬编码 600s（`doc_parser.rs:50`），无健康探测逻辑 |

> ✅ **v1.2 更正**：`[doc_parser] token` 与 `url` **当前就已生效** —— `load_env()`（`src/lib.rs:54-65`）会把任意 `[section] key` 映射为 `SECTION_KEY` 环境变量，且它在 `start_doc_parser()`（`:96`）之前执行。因此"写在 `.env.toml`"与"在启动脚本里 export 环境变量"**两种写法等效**。真正不生效的只有 `autostart`、`timeout_secs`、`health_path`。唯一需注意的坑：模式 A 下随机 UUID 会覆盖你配置的 `token`（见上表）。

---

## 4. 方式一：systemd 部署（同机裸进程，推荐用于生产）

**交付物**（已随本指南提交）：`apps/doc-parser/deploy/doc-parser.service`、`apps/doc-parser/deploy/doc-parser.env.example`

```bash
# 1) 代码与虚拟环境
sudo mkdir -p /opt/doc-parser && sudo chown "$USER" /opt/doc-parser
git clone <repo> /tmp/ap && cp -r /tmp/ap/apps/doc-parser/. /opt/doc-parser/
cd /opt/doc-parser
python -m venv .venv && .venv/bin/pip install -U pip && .venv/bin/pip install .

# 2) 服务账号与数据目录
sudo useradd -r -s /usr/sbin/nologin docparser
sudo mkdir -p /var/lib/doc-parser && sudo chown docparser:docparser /var/lib/doc-parser

# 3) token 生成与配置（关键步骤）
python -c "import uuid; print(uuid.uuid4())"        # 生成 token，两边必须一致
sudo mkdir -p /etc/doc-parser && sudo chmod 700 /etc/doc-parser
sudo install -o root -g root -m 600 \
  /opt/doc-parser/deploy/doc-parser.env.example /etc/doc-parser/doc-parser.env
sudoedit /etc/doc-parser/doc-parser.env             # 填 DOC_PARSER_TOKEN / SURREALDB_PASS 等

# 4) 安装并启动
sudo cp /opt/doc-parser/deploy/doc-parser.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now doc-parser
systemctl status doc-parser --no-pager

# 5) 校验
curl -s http://127.0.0.1:8321/health
# B0-4b 之后应可见鉴权状态，例如：{"status":"ok",...,"auth":"enabled"}

# 6) 鉴权自测（带正确 token → 200；带错误 token → 403；不带 → 401）
TOKEN=$(sudo grep '^DOC_PARSER_TOKEN=' /etc/doc-parser/doc-parser.env | cut -d= -f2-)
curl -s -o /dev/null -w '%{http_code}\n' -H "X-API-Token: $TOKEN" \
     -H 'Content-Type: application/json' \
     -d '{"file_path":"/etc/hostname","options":{}}' http://127.0.0.1:8321/parse
```

**说明**：systemd 形态保持 `127.0.0.1`（不对外）；单元文件已含 `Restart=always`、`NoNewPrivileges`、`ProtectSystem=strict`、`ReadWritePaths=/var/lib/doc-parser /tmp /opt/doc-parser/data` 等加固项。若使用嵌入式 SurrealDB，请把 `SURREALDB_URL` 指到 `/var/lib/doc-parser/surrealdb`（绝对路径），避免依赖 CWD。

**日志与运维**

```bash
journalctl -u doc-parser -f                  # 实时日志（SyslogIdentifier=doc-parser）
sudo systemctl restart doc-parser            # 重启
# 升级：替换 /opt/doc-parser 代码 → .venv/bin/pip install . → systemctl restart doc-parser
```

---

## 5. 方式二：Docker 部署（同机容器）

**交付物**：`apps/doc-parser/Dockerfile`、`apps/doc-parser/.dockerignore`；示例 compose 见 §5.4。

### 5.1 构建

```bash
cd apps/doc-parser
docker build -t doc-parser:0.1.0 .
```

镜像内已安装 `ffmpeg`（含 ffprobe）、`tesseract-ocr` + 中文包、`poppler-utils`、`libgl1`、`libglib2.0-0`。

### 5.2 运行（**注意端口发布方式**）

```bash
# 生成 token（与后端保持一致）
TOKEN=$(python -c "import uuid; print(uuid.uuid4())")

cat > /etc/doc-parser/doc-parser.env <<EOF
PARSER_HOST=0.0.0.0            # 容器内必须 0.0.0.0
PARSER_PORT=8321
DOC_PARSER_TOKEN=$TOKEN
SURREALDB_URL=ws://host.docker.internal:8000
SURREALDB_USER=admin
SURREALDB_PASS=<改成强口令>
OLLAMA_BASE_URL=http://host.docker.internal:11434
HF_ENDPOINT=https://hf-mirror.com
EOF
sudo chmod 600 /etc/doc-parser/doc-parser.env

docker run -d --name doc-parser \
  -p 127.0.0.1:8321:8321 \
  --add-host=host.docker.internal:host-gateway \
  --env-file /etc/doc-parser/doc-parser.env \
  -v docparser-cache:/root/.cache \
  -v docparser-data:/app/data \
  --restart unless-stopped \
  doc-parser:0.1.0
```

**三个必须遵守的点**

1. **`-p 127.0.0.1:8321:8321`**，不要写成 `-p 8321:8321`（后者会发布到所有网卡；容器内 `PARSER_HOST=0.0.0.0` 是不可避免的，端口发布是唯一的外层收敛点）。
2. **`DOC_PARSER_TOKEN` 必须非空**：B0-4b 之后服务会因空 token 拒绝启动，这是有意的（防止容器化后无鉴权暴露）。
3. **挂卷**：`docparser-cache` 保存 Whisper/Embedding 模型避免每次重建下载；`docparser-data` 保存嵌入式 SurrealDB 数据（若使用外部 SurrealDB 可省）。

### 5.3 容器访问宿主机服务

容器内的 `127.0.0.1` 指容器自身。要访问宿主机的 SurrealDB/`Ollama`，用 `--add-host=host.docker.internal:host-gateway` 并配置 `ws://host.docker.internal:8000`、`http://host.docker.internal:11434`。**不建议**用 `--network=host` 绕过（会与 `PARSER_HOST=0.0.0.0` 叠加，直接把服务暴露到所有网卡）。

### 5.4 示例 compose（可选）

```yaml
# apps/doc-parser/deploy/docker-compose.yml
services:
  doc-parser:
    build: ..
    image: doc-parser:0.1.0
    container_name: doc-parser
    ports:
      - "127.0.0.1:8321:8321"      # 只绑本机
    env_file:
      - /etc/doc-parser/doc-parser.env
    extra_hosts:
      - "host.docker.internal:host-gateway"
    volumes:
      - docparser-cache:/root/.cache
      - docparser-data:/app/data
    restart: unless-stopped
    healthcheck:                    # 与镜像内 HEALTHCHECK 一致
      test: ["CMD", "python", "-c",
             "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8321/health', timeout=3)"]
      interval: 30s
      timeout: 5s
      start_period: 60s
      retries: 3

volumes:
  docparser-cache:
  docparser-data:
```

```bash
docker compose -f apps/doc-parser/deploy/docker-compose.yml up -d --build
docker inspect --format '{{.State.Health.Status}}' doc-parser   # 期望 healthy
```

---

## 6. 后端对接配置（Rust 侧）

### 6.1 `[doc_parser]` 段（`apps/backend/.env.toml`）

```toml
[doc_parser]
# 独立部署（systemd / Docker）推荐配置
enabled = false                       # ✅ 当前生效：不从后端拉起进程（= 只做客户端）
url  = "http://127.0.0.1:8321"        # ✅ 当前生效（映射为 DOC_PARSER_URL）
token = "<与 doc-parser 的 DOC_PARSER_TOKEN 完全一致>"   # ✅ 当前生效；⚠️ 勿与 enabled=true 同时保留
# 以下为 B0-4a 后的推荐别名/扩展项，当前不生效：
# autostart = false
# timeout_secs = 600
# health_path = "/health"
```

`apps/backend/.env.example.toml` 已补充该段（含 A/B/C 三模式注释与**逐键生效状态**标注）。

### 6.2 配置写法（两种等效）

`DocParserClient` 只读**环境变量**（`crates/assets-service/src/doc_parser.rs:44-46`），而 `load_env()` 会把 `.env.toml` 的每个 `[section] key` 映射为 `SECTION_KEY` 环境变量（`apps/backend/src-tauri/src/lib.rs:54-65`），且在 `start_doc_parser()` 之前执行（`:92 → :96`）。因此下面两种写法**完全等效**，任选其一：

**写法 A：写在 `.env.toml`（推荐，随仓库外配置文件统一管理）**

```toml
[doc_parser]
enabled = false                       # 不从后端拉起进程（独立部署）
url  = "http://127.0.0.1:8321"
token = "<与 doc-parser 的 DOC_PARSER_TOKEN 完全一致>"
```

**写法 B：进程环境变量（适合容器 / systemd 注入）**

```bash
export DOC_PARSER_ENABLED="false"
export DOC_PARSER_URL="http://127.0.0.1:8321"
export DOC_PARSER_TOKEN="<与 doc-parser 一致>"
```

> ⚠️ **必错写法**：只设 `enabled=false` 而**不提供 token** → 请求不带 `X-API-Token` → 对端配了 token 则 **401**（聊天附件功能整段不可用），对端未配则**无鉴权直通**（安全洞）。
> ⚠️ **模式 A 覆盖**：若保留 `enabled = true` 且端口空闲，后端会生成随机 UUID 并**覆盖**你配置的 `token`（随即将同一值注入子进程，功能自洽，但配置值被静默丢弃，排障时极易误判）。
> 备注：`autostart` 目前**不生效**（无读取处），请用 `enabled` 表达；`timeout_secs` / `health_path` 亦未实现。

### 6.3 配置一致性自检

```bash
# 两侧 token 是否一致（只比对哈希，不打印明文）
sudo grep '^DOC_PARSER_TOKEN=' /etc/doc-parser/doc-parser.env | cut -d= -f2- | sha256sum
grep -A6 '^\[doc_parser\]' apps/backend/.env.toml | grep '^token' | cut -d= -f2- | tr -d ' ' | sha256sum
```

---

## 7. 健康检查与排障

| 现象 | 可能原因 | 处理 |
|---|---|---|
| `curl /health` 正常，`/parse` 返回 **401** "缺少认证令牌" | 服务侧配了 token，客户端没带 | 检查后端进程是否有 `DOC_PARSER_TOKEN`（过渡期）或 `[doc_parser] token`（B0-4a 后） |
| `/parse` 返回 **403** "认证令牌无效" | 两侧 token 不一致（常见于重新生成后只改了一边） | 按 §6.3 比对哈希后统一 |
| 服务**启动即失败**并提示 token 为空 | B0-4b 的 fail-closed 自检生效（预期行为） | 配 `DOC_PARSER_TOKEN`；仅本机调试可临时 `DOC_PARSER_ALLOW_NO_TOKEN=1` |
| 后端报 "doc-parser 请求失败/连接被拒绝" | 服务未启动、端口不对、容器未发布端口 | `systemctl status doc-parser` / `docker ps`；确认 `[doc_parser] url` 与端口一致 |
| 容器内解析报 `FileNotFoundError: 'ffprobe'` | 镜像缺少完整 ffmpeg（`imageio_ffmpeg` 只带 ffmpeg） | 本镜像已装 `ffmpeg`（含 ffprobe）；**自行改造镜像时**不要只装 `imageio_ffmpeg` |
| 首次解析极慢 / 反复下载模型 | 模型缓存未持久化 | Docker 挂 `docparser-cache:/root/.cache`；systemd 确保 `HOME`/缓存目录可写 |
| `/ask` 报连接错误 | 容器内 `localhost:11434` 指不到宿主机 Ollama | 用 `host.docker.internal` + `--add-host=host.docker.internal:host-gateway` |
| 嵌入式 SurrealDB 数据"重启即空" | 数据落在容器临时层 / CWD 相对路径 | 挂 `docparser-data:/app/data` 或改用外部 `ws://` 实例 |
| 端口占用导致后端"跳过启动"且解析 401 | 残留进程 + token 供应缺陷（P0-6b） | `ss -lntp \| grep 8321` 查占用；独立部署下应设 `autostart=false`，让后端不再尝试拉起 |

---

## 8. 安全清单（上线前逐项确认）

- [ ] `DOC_PARSER_TOKEN` 为随机长串（`uuid4` 或 `openssl rand -hex 32`），**两侧一致**
- [ ] `/etc/doc-parser/doc-parser.env` 权限 `600`，属主 `root`，未被纳入版本库
- [ ] 容器端口发布固定为 `127.0.0.1:8321:8321`
- [ ] systemd 形态保持 `PARSER_HOST=127.0.0.1`
- [ ] `SURREALDB_PASS` 已从默认 `Admin@123456` 改为强口令
- [ ] Swagger 文档端点（`/docs`、`/openapi.json` 等）在对外环境按需关闭；当前它们**豁免认证**（仅暴露接口结构）
- [ ] 已确认 `/health` 不返回敏感信息（B0-4b 后仅增加 `auth: enabled/disabled`）
- [ ] token 轮换流程已确定（改两边 → 重启服务与后端）

---

## 9. 已知差异与待实现项

| 项 | 现状 | 归属 |
|---|---|---|
| 后端 `[doc_parser] url/token/autostart/timeout_secs/health_path` 尚不生效 | 当前仅 `enabled/python/host/port` 生效；`url` 与 `token` 只认环境变量 | **B0-4a** |
| `start_doc_parser()` 早退分支跳过 token 生成 | 独立部署时 token 永不被设置 | **B0-4a** |
| 空 token 时 fail-open（带空值 header 可放行） | `apps/doc-parser/main.py:57-67` 仅判 `None` | **B0-4b** |
| `PARSER_HOST=0.0.0.0` 无强制校验 | 容器化后可能无鉴权暴露 | **B0-4b** |
| `/health` 无鉴权状态标识 | 排障需靠日志推断 | **B0-4b** |
| `python main.py` 启动含 `reload=True` | 不适用于生产（`main.py:136-138`） | **B0-4b** |
| 文档不一致：`apps/doc-parser/README.md:123-129` 描述"LanceDB 落盘"，而 `config.py:42-50` 实际是 **SurrealDB** | 易误导部署方（两者数据位置与运维方式不同） | 建议随 B0-6 一并订正 README |
| `README.md` 的 `pip install -e .` / `cp .env.example .env` 与独立部署流程不同 | 无 token 段、无 Docker/systemd 指引 | 本指南已补充，README 建议加一句"独立部署见 docs/doc-parser 独立部署指南.md" |

---

## 10. 跨机 / 集中部署（本次不做，后续事项）

- **传输安全**：跨机必须 TLS（反向代理终止或 uvicorn `--ssl-*`），禁止明文 HTTP 传输 token 与文件内容
- **网络策略**：安全组/防火墙只放通后端所在主机；不使用 `PARSER_HOST=0.0.0.0` 直接暴露公网
- **鉴权增强**：静态共享密钥 → 引入 mTLS 或网关注入身份；token 支持多值/轮换窗口
- **文件传输**：跨机时"后端下载 S3 → 本地临时路径 → 传给 doc-parser"的模式需改为对象存储直传或共享存储（Python 侧无法访问 S3，见 `docs/知识库模块/文档解析与RAG记忆链路设计方案.md:97-98`）
- **可观测**：接入统一日志/指标；把 `/health` 纳入监控与告警

---

## 11. 本指南交付物清单

| 文件 | 说明 |
|---|---|
| `apps/doc-parser/Dockerfile` | 同机容器镜像（含 ffmpeg/ffprobe、tesseract、poppler、healthcheck） |
| `apps/doc-parser/.dockerignore` | 排除 tests/data/缓存/`.env` |
| `apps/doc-parser/deploy/docker-compose.yml` | 同机 compose 示例（端口只绑本机 + 卷 + healthcheck） |
| `apps/doc-parser/deploy/doc-parser.service` | systemd 单元（加固项 + Restart） |
| `apps/doc-parser/deploy/doc-parser.env.example` | `EnvironmentFile` 模板（权限 600） |
| `apps/doc-parser/.env.example` | 已按 A/B/C 三模式重写 token 段 |
| `apps/backend/.env.example.toml` | 新增 `[doc_parser]` 段（标注 B0-4a 生效项） |
| `docs/doc-parser 独立部署指南.md` | 本文件 |
