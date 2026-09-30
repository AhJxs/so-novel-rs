# ── 构建阶段 ──
# Web-only 构建（`--no-default-features --features web`）不链接 GPUI 系统库，builder 无需装
# xcb / xkbcommon / wayland / egl / mesa / vulkan dev 包；`reqwest` 走 `rustls`，不需要 OpenSSL。
# 前端由 Vite 构建（React+TS），产物由 `rust-embed` 编译期嵌入二进制。
# `--mount=type=cache` 复用 cargo target / registry / git 目录，增量构建数量级加速（需 BuildKit）。
FROM rust:1-slim AS builder
WORKDIR /app

# ── 安装 Bun（前端 Turborepo monorepo 构建） ──
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl unzip \
    && rm -rf /var/lib/apt/lists/* \
    && curl -fsSL https://bun.sh/install | bash
ENV PATH="/root/.bun/bin:${PATH}"

# ── 前端依赖层（独立于 Rust 依赖，利用 Docker 缓存） ──
# monorepo：先拷 root + 各 workspace 的 package.json；bun.lock 锁版本。
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
# build.rs 也会触发 bun run build（CARGO_FEATURE_WEB）。
RUN cargo build --release --no-default-features --features web

# ── 运行阶段 ──
# 只需 ca-certificates（书源 HTTPS）+ tini（PID 1 转发 SIGTERM → CancelToken 干净退出，避免
# `docker stop` 10s 后被 SIGKILL）；Web-only 二进制不链接 GPUI，无需系统库。
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

# 必须显式传 `--web`，让 `startup::detect()` 走 argv 分支解析 `--host` / `--port`；只用
# `ENV SO_NOVEL_WEB=1` 会硬编码 host "127.0.0.1"（src/startup/mod.rs），容器内绑 loopback，
# Docker 的 DNAT 打不进去 → "connection refused"。
CMD ["so-novel-rs", "--web", "--host", "0.0.0.0", "--port", "8080"]
EXPOSE 8080

# 数据目录必须放在 USER so-novel 可写的位置（它写不到 /root）。
VOLUME ["/home/so-novel/.sonovel"]

ENTRYPOINT ["/usr/bin/tini", "--"]
