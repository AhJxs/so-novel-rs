# ── 构建阶段 ──
#
# Web-only 构建（`--no-default-features --features web`）：不链接任何 GPUI
# 系统库（xcb / xkbcommon / wayland / egl / mesa / vulkan），仅需网络 / TLS。
# 因此 builder 阶段无需安装 GPUI -dev 包，镜像更精简、构建更快。
#
# `reqwest` 走 `rustls`（Cargo.toml），不需要 OpenSSL —— `libssl-dev` /
# `pkg-config` 无需安装。
#
# `--mount=type=cache` 复用 cargo target / registry / git 目录（增量构建
# 数量级加速；需要 BuildKit，DOCKER_BUILDKIT=1 自动启用）。
# `rust:1-slim` 跟随最新 stable 1.x。
FROM rust:1-slim AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY bundle ./bundle
COPY assets ./assets
COPY locales ./locales
RUN cargo build --release --no-default-features --features web

	# ── 运行阶段 ──
#
# 运行时只需 ca-certificates（书源 HTTPS）+ tini（PID 1 收 SIGTERM → 转给
# 业务进程 → CancelToken 干净退出，避免 `docker stop` 10s 后 SIGKILL）。
#
# 虽然 Web 模式不渲染 GUI，但 so-novel-rs 二进制仍动态链接 GPUI 依赖的
# xcb / xkbcommon / wayland / egl / gl / fontconfig 等库；缺一个就会
# error while loading shared libraries 启动失败。
# 这里仅安装运行时 .so（不带 -dev），体积增量约 30-40 MB。
# 行列与 builder 阶段对齐，方便 review 时对比。
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
