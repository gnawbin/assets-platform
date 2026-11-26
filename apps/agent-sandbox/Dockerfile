# syntax=docker/dockerfile:1
#
# agent-runner 沙箱镜像（OpenShell 工作负载，设计文档 §9.1）
#
# 构建上下文必须是仓库根目录（需要完整 Cargo workspace：`[workspace.dependencies]`）：
#   docker build -f apps/agent-sandbox/Dockerfile -t assets-platform/agent-runner:0.1.0 .
#   docker compose --profile build build agent-runner     # 等价（见根目录 docker-compose.yml）
#
# 运行形态（设计文档 §10.1「一次 exec = 一步」）：
#   - 主进程 `sleep infinity`：沙箱常驻、不承载业务逻辑（`agent-runner serve` 待后续实现）；
#   - 每次 step 由平台 exec 调起：
#       agent-runner step --in /workspace/in/step.json --out /workspace/out/step.json
#
# 镜像内**没有**编译器、包管理器、解释器（python 由 `sandbox_exec` 的底镜像决定，本镜像不提供）。
ARG RUST_VERSION=1

# ---- 构建阶段 -------------------------------------------------------------
FROM rust:${RUST_VERSION}-slim-bookworm AS build
WORKDIR /src

# 整体复制 workspace：Cargo 需要解析全部成员（`members = ["crates/*"]`），
# 且 `-p agent-runner` 只编译该包及其依赖（不会拉起 tauri / sqlx）。
# 构建缓存挂在 registry 与 target 上，不写入镜像层。
COPY apps/backend/src-tauri ./apps/backend/src-tauri
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/apps/backend/src-tauri/target \
    cargo build --release --locked -p agent-runner --bin agent-runner

# ---- 运行阶段 -------------------------------------------------------------
# reqwest 走 rustls/ring（全仓统一），运行镜像不需要 OpenSSL。
FROM debian:bookworm-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd -g 10001 agent \
 && useradd -m -u 10001 -g agent -s /bin/bash agent \
 && mkdir -p /workspace/in /workspace/out /workspace/artifacts \
 && chown -R 10001:10001 /workspace

COPY --from=build /src/apps/backend/src-tauri/target/release/agent-runner /usr/local/bin/agent-runner

ENV HOME=/home/agent \
    RUST_LOG=info
WORKDIR /workspace
# 非 root 运行：策略里的 `process.run_as_user/run_as_group` 与这里保持一致（10001）
USER 10001:10001

# probe 只打印协议版本，不联网、不写文件；容器不健康时平台可先查这一项
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
  CMD ["agent-runner", "probe"]

# 主进程保持存活；具体动作由平台 exec 调起（`agent-runner step ...`）。
# 若改成 ENTRYPOINT，gateway 传入的 command 会与 ENTRYPOINT 拼接，导致 argv 错位。
CMD ["sleep", "infinity"]
