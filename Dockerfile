# ── 构建阶段 ──
#
# Web-only 构建（`--no-default-features --features web`）：不链接任何 GPUI
# 系统库（xcb / xkbcommon / wayland / egl / mesa / vulkan），仅需网络 / TLS。
# 因此 builder 阶段无需安装 GPUI -dev 包，镜像更精简、构建更快。
#
# `reqwest` 走 `rustls`（Cargo.toml），不需要 OpenSSL —— `libssl-dev` /
# `pkg-config` 无需安装。
#
# 前端由 Vite 构建（React+TS），产物由 `rust-embed` 编译期嵌入二进制。
#
# `--mount=type=cache` 复用 cargo target / registry / git 目录（增量构建
# 数量级加速；需要 BuildKit，DOCKER_BUILDKIT=1 自动启用）。
# `rust:1-slim` 跟随最新 stable 1.x。
FROM rust:1-slim AS builder
WORKDIR /app

# ── 安装 Bun（前端 Turborepo monorepo 构建） ──
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl unzip \
    && rm -rf /var/lib/apt/lists/* \
    && curl -fsSL https://bun.sh/install | bash
ENV PATH="/root/.bun/bin:${PATH}"

# ── 前端依赖层（独立于 Rust 依赖，利用 Docker 缓存） ──
# monorepo：先拷 root + 各 workspace 的 package.json，bun install 生成
# hoisted node_modules + workspace 符号链接；bun.lock 锁版本。
COPY web-ui/package.json web-ui/bun.lock web-ui/turbo.json ./web-ui/
COPY web-ui/apps/web/package.json ./web-ui/apps/web/
COPY web-ui/packages/ui/package.json ./web-ui/packages/ui/
RUN cd web-ui && bun install --frozen-lockfile

# ── Rust 依赖缓存层 ──
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY bundle ./bundle
COPY assets ./assets
COPY locales ./locales

# ── 前端源码 + 构建 ──
COPY web-ui ./web-ui
RUN cd web-ui && bun run build

# ── Rust 构建 ──
# build.rs 自动触发 bun run build（CARGO_FEATURE_WEB）；此处再跑一次确保无二次构建副作用。
RUN cargo build --release --no-default-features --features web

# ── 运行阶段 ──
#
# 运行时只需 ca-certificates（书源 HTTPS）+ tini（PID 1 收 SIGTERM → 转给
# 业务进程 → CancelToken 干净退出，避免 `docker stop` 10s 后 SIGKILL）。
#
# Web-only 二进制不链接 GPUI，无需 xcb / xkbcommon / mesa 等系统库。
FROM debian:stable-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates tini \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 1000 --home /home/so-novel --shell /sbin/nologin so-novel \
    && mkdir -p /home/so-novel/.sonovel \
    && chown -R so-novel:so-novel /home/so-novel

COPY --from=builder /app/target/release/so-novel-rs /usr/local/bin/

USER so-novel
WORKDIR /home/so-novel

# Web 模式：必须显式传 `--web` flag，让 `startup::detect()` 走 argv 分支解析
# `--host` / `--port`。**不能**只用 `ENV SO_NOVEL_WEB=1`：env 分支会硬编码
# `host: "127.0.0.1"`（src/startup/mod.rs:58-63），容器内绑 loopback 而非 0.0.0.0，
# Docker `-p 8080:8080` 的 iptables DNAT 把包发到 bridge IP，loopback 不接收 → RST
# → host 上 "connection refused"。`--web` + `--host 0.0.0.0` 才能让 axum 绑 0.0.0.0，
# DNAT 才能命中。
#
# 也允许用户覆盖 `CMD` 切到 CLI / GUI 模式（`docker run so-novel --help` 之类），
# 但若覆盖后丢 `--web` / `--host 0.0.0.0`，外网访问会失败 —— 已知 footgun。
CMD ["so-novel-rs", "--web", "--host", "0.0.0.0", "--port", "8080"]
EXPOSE 8080

# 数据目录写在 /home/so-novel/.sonovel 而不是 /root/...，因为
# USER so-novel 写不到 /root。
VOLUME ["/home/so-novel/.sonovel"]

# tini 收 SIGTERM → 转给 so-novel-rs → CancelToken 触发 Cancelled 事件。
ENTRYPOINT ["/usr/bin/tini", "--"]