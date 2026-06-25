# ── 构建阶段 ──
#
# GPUI 0.2.2 在 Linux 上链接需要 xkbcommon / xcb / wayland / gl / egl /
# fontconfig / mesa-vulkan，跟 `.github/workflows/release.yml` 的 Linux 步骤
# 完全对齐。**这些是 cargo build 链接期必须的**，缺一个会 link 失败（不是
# 运行时才报 —— 之前原版 Dockerfile 装少了，docker build 必挂在 builder 阶段）。
#
# `reqwest` 走 `rustls`（Cargo.toml），不需要 OpenSSL —— `libssl-dev` /
# `pkg-config` 删了。
#
# `--mount=type=cache` 复用 cargo target / registry / git 目录（增量构建
# 数量级加速；需要 BuildKit，DOCKER_BUILDKIT=1 自动启用）。
# `rust:1-slim` 跟随最新 stable 1.x（1.88+ at 2026-06）。`rust:1.85-slim`
# 会因某个依赖需要 Rust > 1.85 而 cargo build exit 101：
#   "Either upgrade rustc or select compatible dependency versions with
#    cargo update <name>@<current-ver> --precise <compatible-ver>"
# 改用 `cargo update` 钉版本会让依赖图变脆（某个 dep 锁版后其它 dep 也可能
# 触发连锁），不如让 Dockerfile 跟 release.yml (`dtolnay/rust-toolchain@stable`)
# 对齐 —— 两者都用最新 stable 1.x。
# Cargo.toml 里的 `rust-version = "1.85"` 保留：MSRV 仍是 1.85（理论上用户
# 自编译能 build），但 CI / Docker 用最新 stable 跑。
FROM rust:1-slim AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends \
    libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev \
    libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libwayland-dev libgl1-mesa-dev libegl1-mesa-dev \
    libglib2.0-dev mesa-vulkan-drivers \
    && rm -rf /var/lib/apt/lists/*

# 直接复制全部源码（lib+bin 项目的"dummy 缓存"层会因为空 src/lib.rs
# 编译失败 —— cargo 把空 lib 视为缺内容；并且这个项目有 build.rs / proc-macro
# 依赖，dummy 抽象不干净）。BuildKit 的 `cache-to=type=registry` 已经
# 把 cargo target 缓存在 ghcr.io 的 `so-novel-rs/cache` 镜像里，跨架构复用
# 充分，不再需要本地 dummy 层。
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY bundle ./bundle
COPY assets ./assets
COPY locales ./locales
RUN cargo build --release

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
    libxkbcommon0 libxkbcommon-x11-0 libfontconfig1 \
    libxcb1 libxcb-render0 libxcb-shape0 libxcb-xfixes0 \
    libwayland-client0 libwayland-cursor0 libwayland-egl1 \
    libgl1 libegl1 libglib2.0-0 \
    mesa-vulkan-drivers \
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
