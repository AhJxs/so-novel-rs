# Web 模式

`so-novel-rs` 第三种运行模式 —— axum HTTP 服务，浏览器访问，
手机 / 平板 / 桌面多端响应式。**和 CLI / GUI 共用同一份 `parser` / `crawler` /
export` / `http` 代码**，行为一致。

本文档覆盖：
- 快速启动（CLI / 环境变量）
- 默认配置 & 安全
- 数据持久化
- API 端点概览
- Docker 部署
- 反向代理（nginx / Caddy）
- 打包脚本（`scripts/package-linux.sh`）
- 故障排查

---

## 快速启动

### 方式 1：CLI 参数

```sh
# 默认 127.0.0.1:8080（仅本机）
so-novel-rs --web

# 显式指定 host / port
so-novel-rs --web --host 0.0.0.0 --port 9000
```

### 方式 2：环境变量（Docker / systemd 友好）

```sh
SO_NOVEL_WEB=1 so-novel-rs --web
```

`SO_NOVEL_WEB=1` 或 `SO_NOVEL_WEB=true` 等价于传 `--web`，便于
Dockerfile / docker-compose / systemd unit 不用改 entrypoint。

> **优先级**：CLI 参数 > 环境变量。如果同时传了 `--web` 和
> `SO_NOVEL_WEB=0`，以 CLI 为准。

启动后浏览器打开 `http://localhost:8080` 即可。

---

## 默认配置

| 项 | 默认 | 含义 |
|---|---|---|
| Host | `127.0.0.1` | 默认只 bind loopback，**外部网络不可达** |
| Port | `8080` |  |
| 访问码 | 空 | 任何能访问 host:port 的人可直接进首页 |
| 持久化目录 | `~/.sonovel/` | 详见下文 |
| 前端 | 单页（`src/web/static/`） | `index.html` / `app.js` / `style.css` / `theme.js` |

> **⚠️ 安全提醒**：把 `--host 0.0.0.0` 暴露到公网前**必须**设置访问码 + 加
> 反向代理（带 HTTPS）。详见 [§ 安全](#安全)。

---

## 数据持久化

Web 进程会读写以下文件，**全部在 `~/.sonovel/` 下**（用 `ConfigPaths::discover()`
发现；root 跑就在 `/root/.sonovel/`，普通用户在 `/home/<user>/.sonovel/`）：

| 路径 | 用途 |
|---|---|
| `config.toml` | 主配置（下载路径 / 格式 / 语言 / 网络等） |
| `rules/main.json` 等 | 书源规则文件（首次启动自动初始化） |
| `sources_config.json` | 当前活跃书源 + 禁用列表 |
| `themes/*.json` | 用户主题（热重载） |
| `tasks.json` | 下载任务记录（自动清理超额已完成任务） |
| `logs/sonovel.YYYY-MM-DD.log` | 运行日志（启动时清 30 天前的） |
| `library/<book_dir_name>/` | 下载中的章节缓存（export 后清理） |

> **Docker 部署必须**把 `~/.sonovel/` 整个目录挂成 volume，否则容器重启
> 数据全丢。详见 [§ Docker 部署](#docker-部署)。

---

## API 端点概览

完整路由定义：[`src/web/routes.rs`](../src/web/routes.rs)。下面是面向用户/集成
的端点速查：

### 搜索 / 书

| Method | Path | 说明 |
|---|---|---|
| GET | `/` | 单页前端（HTML） |
| GET | `/api/search?keyword=&source=&limit=` | 搜索书源 |
| GET | `/api/book/detail?url=&source=` | 抓详情页（书名 / 作者 / 简介 / 封面） |
| GET | `/api/book/toc?url=&source=` | 抓目录（章节列表） |

### 下载 / 任务

| Method | Path | 说明 |
|---|---|---|
| POST | `/api/download` | 启动下载任务（body: `{url, source, output, format, from, to}`） |
| GET | `/api/tasks` | 列出所有任务（含历史） |
| POST | `/api/tasks/{id}/cancel` | 取消任务（按 CancelToken） |

下载进度通过 **SSE**（`/api/download` 返回 `text/event-stream`）实时推送。

### 书库

| Method | Path | 说明 |
|---|---|---|
| GET | `/api/library` | 列出已下载的成品文件 |
| GET | `/api/files/{filename}` | 下载成品文件（epub / txt / pdf / html zip） |
| DELETE | `/api/library/{filename}` | 删除成品 |

### 书源管理

| Method | Path | 说明 |
|---|---|---|
| GET | `/api/sources` | 列出所有书源（含 enabled / disabled） |
| POST | `/api/sources/{id}/toggle` | 切换启用 / 禁用 |
| POST | `/api/sources/{id}/test` | 测速（ping 详情页） |

### 设置 / 认证

| Method | Path | 说明 |
|---|---|---|
| GET | `/api/settings` | 读 `config.toml`（UI 友好的 subset） |
| PUT | `/api/settings` | 写回部分配置（不会覆盖未指定的字段） |
| GET | `/api/auth/status` | 当前是否启用了访问码 |
| POST | `/api/auth` | 提交访问码（返回 cookie / session） |
| POST | `/api/access-code` | 设置访问码（空字符串 = 关闭） |

> 所有 `/api/*` 返回 JSON（除 SSE 端点外）。Session 由 `axum-session` 内存
> 维护（重启 = 重新登录），不持久化。

---

## Docker 部署

[`Dockerfile`](../Dockerfile) 已经把多阶段构建 + 运行时精简 + tini init +
非 root 用户（`uid 1000`）写好了。**默认 entrypoint 是 Web 模式**
（`SO_NOVEL_WEB=1` + `--host 0.0.0.0 --port 8080`），`EXPOSE 8080`，
`VOLUME /home/so-novel/.sonovel`（非 root 后写到用户家目录）。

CI 流程：每次推 `v*` tag，[`.github/workflows/docker-release.yml`](../.github/workflows/docker-release.yml)
会构建多架构镜像（linux/amd64 + linux/arm64/v8）并推到
`ghcr.io/<owner>/so-novel-rs:<tag>` + `:latest`（仅 stable 版）。

### 方式 A：拉取发布镜像（推荐）

```sh
# 稳定版（跟随最新 release）
docker run -d \
  --name so-novel \
  -p 8080:8080 \
  -v so-novel-data:/home/so-novel/.sonovel \
  --restart unless-stopped \
  ghcr.io/ahjxs/so-novel-rs:latest

# 锁版本
docker run -d \
  --name so-novel \
  -p 8080:8080 \
  -v so-novel-data:/home/so-novel/.sonovel \
  --restart unless-stopped \
  ghcr.io/ahjxs/so-novel-rs:0.3.4
```

> **首次拉取后**确认包是 public：访问
> <https://github.com/users/ahjxs/packages/container/so-novel-rs/settings> →
> "Change package visibility" → Public。否则 `docker pull` 会 401 unauthorized。

### 方式 B：自建镜像（高级 / 改源码后）

```sh
# 1. 从仓库根目录构建
docker build -t so-novel .

# 2. 跑（命名卷保数据）
docker run -d \
  --name so-novel \
  -p 8080:8080 \
  -v so-novel-data:/home/so-novel/.sonovel \
  --restart unless-stopped \
  so-novel
```

浏览器访问 `http://<host>:8080`。`so-novel-data` 是命名卷，重启 / 重建
容器都不会丢数据。

### 自定义端口

```sh
# 容器内 8080 → 主机 9000
docker run -d -p 9000:8080 \
  -v so-novel-data:/home/so-novel/.sonovel \
  ghcr.io/ahjxs/so-novel-rs:latest

# 或改环境变量 + entrypoint 覆盖
docker run -d -p 9000:9000 \
  -e SO_NOVEL_WEB=1 \
  --entrypoint so-novel-rs \
  ghcr.io/ahjxs/so-novel-rs:latest \
  --web --host 0.0.0.0 --port 9000
```

### 指定镜像里的访问码（启动后通过 Web UI 设置也可以）

```sh
docker run -d -p 8080:8080 \
  -v so-novel-data:/home/so-novel/.sonovel \
  -e SO_NOVEL_ACCESS_CODE=your-secret-here \
  ghcr.io/ahjxs/so-novel-rs:latest
```

> 提示：环境变量 `SO_NOVEL_ACCESS_CODE` 在容器首次启动时检查，若
> `/home/so-novel/.sonovel/config.toml` 里 `access_code` 为空就写入并启用。
> 已有访问码时**不会**覆盖（避免镜像重建后访问码丢失）。

### Docker Compose

[`docker-compose.yml`](../docker-compose.yml) 还没建（如果你需要可以照
下面模板加）。最小可用版本：

```yaml
services:
  so-novel:
    build: .
    image: so-novel
    container_name: so-novel
    restart: unless-stopped
    ports:
      - "8080:8080"
    volumes:
      - so-novel-data:/home/so-novel/.sonovel
    environment:
      - SO_NOVEL_WEB=1
      - RUST_LOG=info  # 启动 tracing 输出到 docker logs
    healthcheck:
      test: ["CMD", "wget", "--spider", "-q", "http://127.0.0.1:8080/"]
      interval: 30s
      timeout: 5s
      retries: 3

volumes:
  so-novel-data:
```

### 镜像体积 / 构建时间

`Dockerfile` 是标准多阶段：
	- 阶段 1：`rust:1-slim` + 编译 release（~2-5 GB 中间产物）
- 阶段 2：`debian:stable-slim` + 二进制（最终 ~80-150 MB）

> 构建阶段用 `rust:1-slim`（随 Rust / Debian 最新 stable），
> 运行阶段用 `debian:stable-slim` 与之对齐，避免 GLIBC 版本不匹配
> （`rust:1-slim` 基于最新 Debian stable slim，两者必须同版本）。
> 生产优化（如 distroless、scratch、UPX）可以单独做，
> 但要权衡 `ca-certificates` / `glibc` 依赖。

---

## 反向代理（生产环境推荐）

直接 `--host 0.0.0.0` 暴露 8080 不带 TLS 是**不安全**的（API / Cookie /
访问码都在明文）。生产场景必须套一层：

### nginx + Let's Encrypt

```nginx
# /etc/nginx/sites-available/so-novel
server {
    listen 80;
    server_name sonovel.example.com;
    return 301 https://$host$request_uri;
}

server {
    listen 443 ssl http2;
    server_name sonovel.example.com;

    ssl_certificate     /etc/letsencrypt/live/sonovel.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/sonovel.example.com/privkey.pem;

    # 推荐：限制上传大小（下载章节请求体不大）
    client_max_body_size 1m;

    # SSE：禁用 proxy buffering 防止进度事件被缓冲
    location /api/download {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_buffering off;
        proxy_cache off;
        proxy_read_timeout 1h;  # 长任务不死连接
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

关键点：
- **`/api/download` 关掉 `proxy_buffering`**：SSE 推送必须实时，nginx 默认会
  缓冲 4KB 才会发给客户端，体感就是"几秒后才看到一批进度"。
- **`proxy_read_timeout 1h`**：长下载 / 长 session 不被 nginx 主动断。
- **`client_max_body_size 1m`**：限制上传（防滥用）。

### Caddy

```caddy
# /etc/caddy/Caddyfile
sonovel.example.com {
    reverse_proxy 127.0.0.1:8080 {
        # SSE 兼容
        flush_interval -1
    }
}
```

Caddy 自动管 TLS（Let's Encrypt 集成），3 行搞定。

---

## 打包

### 离线 tar.gz（Linux x86_64 / aarch64）

[`scripts/package-linux.sh`](../scripts/package-linux.sh) 把编译产物 + README
+ `bundle/rules/` 打成 tar.gz：

```sh
# 在仓库根目录
cargo build --release
bash scripts/package-linux.sh x86_64-unknown-linux-gnu

# 产物：dist/so-novel-rs-0.3.4-linux-x86_64/  +  .tar.gz
ls dist/
```

CI 路径：[`.github/workflows/release.yml`](../.github/workflows/release.yml)
直接复用这个脚本，多平台矩阵构建后上传到 GitHub Release。

### 跨平台 release 产物

`.github/workflows/release.yml` 的 `build` job 矩阵：

| OS | Target | Archive |
|---|---|---|
| `ubuntu-latest` | `x86_64-unknown-linux-gnu` | `tar.gz`（脚本） |
| `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | `tar.gz`（脚本） |
| `macos-latest` | `aarch64-apple-darwin` | `tar.gz`（步骤） |
| `windows-latest` | `x86_64-pc-windows-msvc` | `zip`（步骤） |

产物命名统一为 `so-novel-rs-<version>-<os>-<arch>.<ext>`，全部带
`bundle/rules/`，用户开箱即用。

### 本地启动 + 验证

```sh
# 解压后
tar -xzf so-novel-rs-0.3.4-linux-x86_64.tar.gz
cd so-novel-rs-0.3.4-linux-x86_64
./so-novel-rs --web --host 0.0.0.0 --port 8080
# 浏览器开 http://localhost:8080
```

---

## 安全

### 默认 vs 暴露

| 场景 | 配置 |
|---|---|
| 本机玩 | `--web`（默认 127.0.0.1）+ 不设访问码 |
| 局域网 / VPN 内部 | `--web --host 0.0.0.0` + 设访问码（Web UI 设置） |
| 公网 / Docker | **必加**反代 + 设访问码 + HTTPS + 防火墙白名单 |

### 访问码

通过 Web UI `设置` → 访问码 启用。启用后未登录请求被拦截在 session
层。`axum-session` 的 session 在内存里（重启 = 重新登录），
**不**持久化到磁盘 —— 这是有意的（容器重建 / 进程崩溃后访问码不失效）。

> ⚠️ **不要**把 `SO_NOVEL_WEB=1` 暴露的容器直接放到公网 + 不设访问码
> + 不加反代。任何能访问 host:port 的人都能：
> - 触发下载（耗你的网络 / IP）
> - 启用 / 禁用书源
> - 修改 `config.toml`（改下载路径 / 代理等）
> - 读你的 `tasks.json`（看到所有下载历史）

### CORS

`web::run` 注册的 tower-http CORS 默认是 permissive
（便于本地 `http://localhost:8080` 跨域开发）。生产反代时一般不需要
CORS（反代和 backend 同源），可关。

### 速率限制 / 滥用

当前**没有**rate-limit。SSE 长连接 + 重复 `/api/search` 都没有限流。
公网暴露前**自己**在反代层加（如 nginx `limit_req`）。

---

## 故障排查

| 现象 | 可能原因 | 排查 |
|---|---|---|
| 容器 `docker logs` 看到 `Bind: address already in use` | 主机端口已被占 | `netstat -tnlp | grep 8080` 改 `--port` 或停冲突进程 |
| 浏览器 `connection refused` | 容器没起来 / 端口没 publish | `docker ps -a` 看状态 / `docker logs so-novel` |
| SSE 进度"卡住"几秒才动 | nginx 缓冲了 | `proxy_buffering off` |
| 启动报 `config.toml parse failed` | 文件损坏（半截写） | `docker exec -it so-novel rm /home/so-novel/.sonovel/config.toml` 让容器重启时重新生成（**会丢配置**） |
| 启动报 `permission denied` 在 `/home/so-novel/.sonovel/` | volume 挂载权限不对 | `chown -R 1000:1000 /path/on/host/.sonovel`（uid 1000 = 容器内 so-novel 用户） |
| 反向代理后 502 Bad Gateway | backend 没起来 / host 错 | `curl http://127.0.0.1:8080` 直连测 |
| 访问码忘了 | — | `docker exec -it so-novel sed -i 's/^access_code = ".*"/access_code = ""/' /home/so-novel/.sonovel/config.toml && docker restart so-novel` |
| 容器重启后书源 / 下载历史没了 | volume 没挂载 | `docker inspect so-novel` 看 `Mounts` 字段 |

### 启用 tracing

容器内 `RUST_LOG=info`（或 `debug`）环境变量会被 tracing 读到，写到
`/home/so-novel/.sonovel/logs/sonovel.YYYY-MM-DD.log`（启动时自动清 30 天前的）：

```sh
docker run -d -p 8080:8080 \
  -v so-novel-data:/home/so-novel/.sonovel \
  -e RUST_LOG=info \
  --name so-novel \
  ghcr.io/ahjxs/so-novel-rs:latest

# 实时跟踪
docker exec -it so-novel tail -f /home/so-novel/.sonovel/logs/sonovel.$(date +%F).log
```

> Tracing 输出**也**走 stdout（`axum` 默认 + `tracing-subscriber` 配置），
> 所以 `docker logs so-novel` 也能看到。

---

## 进一步阅读

- Web 实现入口：[`src/web/mod.rs`](../src/web/mod.rs)（`run()` 函数 + `WebState` + 路由）
- Web 路由：[`src/web/routes.rs`](../src/web/routes.rs)
- Web 处理器（按文件拆）：[`src/web/handlers/`](../src/web/handlers/)
  - `search.rs` / `book.rs` — 搜索 / 详情
  - `download.rs` — 下载任务（SSE 进度）
  - `library.rs` — 书库（成品文件管理）
  - `misc.rs` — 认证 / 设置 / 书源开关
- 前端单页（无构建步骤）：[`src/web/static/`](../src/web/static/)
- CLI 入口 / 模式分发：[`src/main.rs`](../src/main.rs)（`run_web` 函数）
- Docker：[`Dockerfile`](../Dockerfile)
- Linux 打包脚本：[`scripts/package-linux.sh`](../scripts/package-linux.sh)
- Release workflow：[`.github/workflows/release.yml`](../.github/workflows/release.yml)
- [CLI.md](./CLI.md) — CLI 模式
- [CHANGELOG.md](./CHANGELOG.md) — 最新 release
