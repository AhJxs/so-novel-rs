# ── 构建阶段 ──
FROM rust:1.85-slim AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release

# ── 运行阶段 ──
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/so-novel-rs /usr/local/bin/

# 默认以 Web 模式启动
ENV SO_NOVEL_WEB=1
EXPOSE 8080

# 数据目录（用户可挂载 volume）
VOLUME ["/root/.sonovel"]

CMD ["so-novel-rs"]
