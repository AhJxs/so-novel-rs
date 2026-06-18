#!/usr/bin/env bash
# 打包 so-novel-rs 的 Linux 发行包。
#
# 用法：
#   bash scripts/package-linux.sh                              # 默认 x86_64
#   bash scripts/package-linux.sh aarch64-unknown-linux-gnu    # 指定 target
#
# 行为：
# - 自动选择构建工具：在 Linux 主机上（如 CI runner）用原生 `cargo`，
#   否则用 `cross`（容器化交叉编译，要装 Docker + cross）。
#   设 `FORCE_CROSS=1` 强制使用 cross；`FORCE_CARGO=1` 强制原生 cargo。
# - 把可执行文件 + README 打成 tar.gz。
#   规则 / HTML 模板 / quanben5.js / logo 已通过 `include_*!` 嵌入二进制，
#   无需随包带任何 bundle/ 资源。
#
# 产物：dist/so-novel-rs-<version>-linux-<arch>.tar.gz

set -euo pipefail

NAME=so-novel-rs
TARGET="${1:-x86_64-unknown-linux-gnu}"

# 从 Cargo.toml 取版本号（避免双重维护）。
VERSION=$(grep -E '^version\s*=' Cargo.toml | head -1 | sed -E 's/.*"([^"]+)".*/\1/')
if [ -z "$VERSION" ]; then
    echo "ERROR: 从 Cargo.toml 读不到版本号" >&2
    exit 1
fi

ARCH="${TARGET%%-*}"
STAGE="${NAME}-${VERSION}-linux-${ARCH}"
OUTDIR="dist/${STAGE}"
TARBALL="dist/${STAGE}.tar.gz"

# ----- 选构建工具 -----
choose_builder() {
    if [ "${FORCE_CROSS:-}" = "1" ]; then
        echo "cross"
        return
    fi
    if [ "${FORCE_CARGO:-}" = "1" ]; then
        echo "cargo"
        return
    fi
    # Linux 主机 + 目标是 Linux：直接 cargo（原生编更快，且 CI 上没 Docker）。
    if [ "$(uname -s)" = "Linux" ]; then
        echo "cargo"
        return
    fi
    # 其它平台（Windows / macOS）：用 cross。
    echo "cross"
}

BUILDER=$(choose_builder)
echo "→ 构建工具: $BUILDER"
echo "→ Target  : $TARGET"
echo "→ 版本    : $VERSION"

# ----- 构建 -----
case "$BUILDER" in
    cargo)
        # 主机就是 Linux 时仍可指定 target — 但不同 arch 仍要 cross；
        # 这里保留 --target 让 x86_64 → aarch64 之类的 case 不退化为本机编。
        cargo build --release --target "$TARGET"
        ;;
    cross)
        if ! command -v cross >/dev/null 2>&1; then
            echo "ERROR: 未找到 cross。安装: cargo install cross --git https://github.com/cross-rs/cross" >&2
            exit 1
        fi
        cross build --release --target "$TARGET"
        ;;
esac

BIN="target/${TARGET}/release/${NAME}"
if [ ! -f "$BIN" ]; then
    echo "ERROR: 找不到产物 $BIN" >&2
    exit 1
fi

# ----- 组装包 -----
rm -rf "$OUTDIR"
mkdir -p "$OUTDIR"

cp "$BIN" "$OUTDIR/"
chmod +x "$OUTDIR/${NAME}"

# README 让用户知道运行时依赖。
cat > "$OUTDIR/README.md" <<'EOF'
# So Novel — Linux 包

## 运行

```bash
./so-novel-rs              # GUI 模式
./so-novel-rs sources      # 列书源（CLI）
./so-novel-rs --help       # 看更多子命令
```

首次启动会在 exe 同目录生成 `config.toml` + `sonovel.db`。
默认下载目录是 `~/Documents/Novel/`（XDG `XDG_DOCUMENTS_DIR` 优先，未设置时用 `~/Documents`）。

## 运行时依赖

主流发行版自带，缺的话装一下：

```bash
# Debian / Ubuntu
sudo apt install libxkbcommon0 libgl1 libegl1 libfontconfig1 \
                 libxcb1 libxcb-render0 libxcb-shape0 libxcb-xfixes0 \
                 libwayland-client0 libwayland-cursor0

# Fedora / RHEL
sudo dnf install libxkbcommon mesa-libGL fontconfig \
                 libxcb wayland-libs
```
EOF

# ----- 打 tar.gz -----
( cd dist && tar -czf "${STAGE}.tar.gz" "${STAGE}" )
echo "→ 产物: ${TARBALL}"
ls -lh "${TARBALL}"
