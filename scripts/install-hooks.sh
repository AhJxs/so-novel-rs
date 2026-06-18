#!/bin/sh
# 一键启用项目 git hooks：把 core.hooksPath 指向 tracked `.githooks/` 目录。
#
# 用法：sh scripts/install-hooks.sh
# 克隆仓库后执行一次即可；之后 .githooks/pre-commit 会在每次 git commit 时自动
# 格式化暂存的 .rs 文件。

set -e

cd "$(git rev-parse --show-toplevel)"

git config core.hooksPath .githooks
# pre-commit 需可执行（Windows 上 git-bash 也认 chmod 位）
chmod +x .githooks/pre-commit 2>/dev/null || true

echo "已启用项目 git hooks：core.hooksPath = .githooks"
