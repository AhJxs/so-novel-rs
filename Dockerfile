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

# ── 安装 Node.js (Vite 前端构建) ──
RUN apt-get update && apt-get install -y --no-install-recommends \
    nodejs npm \
    && rm -rf /var/lib/apt/lists/*

# ── 前端依赖层（独立于 Rust 依赖，利用 Docker 缓存） ──
#
# `npm ci` 在 Windows host + Docker Desktop (WSL2/Hyper-V) + fuse-overlayfs
# 组合下常见 `npm WARN tar TAR_ENTRY_ERROR EINVAL: invalid argument, fchown`
# —— pacote 调 tar 解包时尝试保留 uid/gid 元数据，fuse 不支持跨 uid 映射。
# **不影响 build**（node_modules 内容正确解压），但日志噪音易掩盖真问题。
# 修法：`--no-audit --no-fund` 减 npm 自身日志 + `--foreground-scripts`
# 阻止 tarball postinstall 时跑（没有 postinstall 但显式声明更稳），
# 然后把 tar fchown 警告 grep 掉 —— 真错仍冒出来。
COPY web-ui/package.json web-ui/package-lock.json ./web-ui/
RUN npm ci --no-audit --no-fund --prefix web-ui 2>&1 | grep -v "TAR_ENTRY_ERROR EINVAL: invalid argument, fchown" || true

# ── Rust 依赖缓存层 ──
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY bundle ./bundle
COPY assets ./assets
COPY locales ./locales

# ── 前端源码 + 构建 ──
COPY web-ui ./web-ui
RUN npm run build --prefix web-ui

# ── Rust 构建 ──
# build.rs 自动触发 npm run build（CARGO_FEATURE_WEB）；此处再跑一次确保无二次构建副作用。
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

# Web 模式由 `SO_NOVEL_WEB=1` 触发（src/main.rs:23-25），也允许用户
# 覆盖 `CMD` 切到 CLI / GUI 模式（`docker run so-novel --help` 之类）。
ENV SO_NOVEL_WEB=1
EXPOSE 8080

# 数据目录写在 /home/so-novel/.sonovel 而不是 /root/...，因为
# USER so-novel 写不到 /root。
VOLUME ["/home/so-novel/.sonovel"]

# tini 收 SIGTERM → 转给 so-novel-rs → CancelToken 触发 Cancelled 事件。
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["so-novel-rs", "--host", "0.0.0.0", "--port", "8080"]