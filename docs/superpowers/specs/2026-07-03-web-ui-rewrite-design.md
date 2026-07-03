# Web UI 重写设计文档

**日期：** 2026-07-03  
**状态：** 待实现  
**作者：** Kiro

## 背景

现有 Web UI 使用 Tailwind CSS CDN + Alpine.js，所有 HTML/JS/CSS 以字符串形式内嵌在 Rust 二进制中，通过占位符替换（`__BODY__`、`__APP_JS__` 等）动态组装。

该方案功能完整但视觉层次感不足，组件复用率低，维护成本随功能增加而上升。本次重写目标：用现代前端技术栈实现更精美的 UI，同时保持零运行时文件依赖（仍内嵌进二进制）。

---

## 技术选型

| 层 | 选型 | 版本 |
|---|---|---|
| 构建工具 | Vite | 6.x |
| 前端框架 | React | 18.x |
| 类型 | TypeScript | 5.x |
| 路由 | React Router | v7 |
| 服务端状态 | TanStack Query | v5 |
| 组件库 | shadcn/ui | latest |
| 样式 | Tailwind CSS | v4 |
| 通知 | Sonner | latest |
| 二进制嵌入 | rust-embed | 8.x |

---

## §1 项目结构

```
so-novel-rs/
├── src/                    # Rust 后端
│   └── web/                # axum handlers（API 路由，按需调整）
├── web-ui/                 # 前端项目根目录
│   ├── src/
│   │   ├── routes/         # 页面组件
│   │   │   ├── search.tsx
│   │   │   ├── book-detail.tsx
│   │   │   ├── tasks.tsx
│   │   │   ├── library.tsx
│   │   │   ├── sources.tsx
│   │   │   └── settings.tsx
│   │   ├── components/     # 共用UI组件（shadcn/ui + 自定义）
│   │   │   ├── ui/         # shadcn/ui 生成组件
│   │   │   ├── layout/     # Navbar, Layout
│   │   │   └── shared/     # 跨页面共用组件
│   │   ├── hooks/          # TanStack Query hooks
│   │   │   ├── use-search.ts
│   │   │   ├── use-book-detail.ts
│   │   │   ├── use-download-progress.ts
│   │   │   ├── use-tasks.ts
│   │   │   ├── use-library.ts
│   │   │   ├── use-sources.ts
│   │   │   └── use-settings.ts
│   │   ├── lib/
│   │   │   ├── api.ts      # fetch 封装，baseURL /api
│   │   │   └── utils.ts
│   │   ├── main.tsx
│   │   └── App.tsx
│   ├── package.json
│   ├── tsconfig.json
│   └── vite.config.ts
├── build.rs                # 自动触发 Vite 构建
└── Cargo.toml
```

---

## §2 路由设计

```
/                 → redirect /search
/search           → 搜索页（书名/作者 + 书源筛选）
/search/:bookUrl  → 书籍详情页（独立 URL，可刷新/分享）
/tasks            → 下载任务页（实时进度）
/library          → 本地书库页（格式筛选 + 下载/删除）
/sources          → 书源管理页（开关 + 测速）
/settings         → 设置页（代理/爬取）
```

`:bookUrl` 为 URL 编码后的书籍来源 URL，与现有后端标识符一致。

详情页从"覆盖搜索结果的内嵌 state"升级为独立路由：浏览器返回键正常、可收藏链接、刷新不丢数据。

---

## §3 数据层（TanStack Query）

所有服务端状态通过自定义 hook 封装，组件不直接调 fetch。

```ts
// hooks/use-search.ts
useSearchResults(keyword: string, sourceId: string)
// staleTime: 5min，keyword 变化时自动 refetch

// hooks/use-book-detail.ts
useBookDetail(bookUrl: string)
useToc(bookUrl: string)  // enabled: false，手动 trigger

// hooks/use-tasks.ts
useTasks()               // refetchInterval: 2000（下载中时）
useCancelTask()          // mutation
useStartDownload()       // mutation，返回 taskId

// hooks/use-download-progress.ts
useDownloadProgress(taskId: number)
// 内部 new EventSource('/api/tasks/:id/stream')
// 完成或失败后自动关闭连接，invalidate useTasks

// hooks/use-library.ts
useLibrary()
useDeleteFile()          // mutation，success → invalidate useLibrary

// hooks/use-sources.ts
useSources()
useToggleSource()        // mutation，optimistic update
useTestSource()          // mutation，不 invalidate（仅返回延迟ms）

// hooks/use-settings.ts
useSettings()
useSaveSettings()        // mutation，debounce 800ms
```

---

## §4 UI 设计规范

### 色彩主题

- 主色：`violet`（比现有 indigo 更柔和，shadcn/ui 原生支持）
- 中性色：`zinc`（shadcn/ui 默认）
- 深色模式：`class` 策略，`ThemeToggle` 组件切换

### shadcn/ui 组件映射

| 功能 | 组件 |
|---|---|
| 搜索结果卡片 | `Card` + `HoverCard`（悬浮显示完整简介） |
| 书源/格式选择 | `ToggleGroup` |
| 下载进度条 | `Progress` |
| 书源开关 | `Switch` |
| 顶部导航 | `NavigationMenu` |
| 设置表单 | `Form` + `Input` + `Label` |
| 操作通知 | `Sonner`（替换当前 toast） |
| 任务状态标签 | `Badge` |
| 分页 | `Pagination` |
| 目录滚动区 | `ScrollArea` |
| 加载占位 | `Skeleton`（搜索结果/详情页） |
| 书库格式筛选 | `Tabs`（全部 / EPUB / TXT / PDF / HTML）|

### 视觉升级亮点

- 搜索结果：`HoverCard` 悬浮预览完整简介，无需跳转
- 加载状态：骨架屏（`Skeleton`）替代 spinner
- 下载完成：`Sonner` 弹出含文件下载链接的通知
- 书库：`Tabs` 按格式过滤，替代当前全列表
- 书源：测速结果以延迟毫秒数 + 颜色 Badge 呈现

---

## §5 Rust 侧集成

### Cargo.toml 新增依赖

```toml
[features]
web = [
    "dep:rust-embed",
    "dep:mime_guess",
    # ... 现有 web deps
]

[dependencies]
rust-embed = { version = "8", features = ["include-exclude"], optional = true }
mime_guess  = { version = "2", optional = true }
```

### axum 静态文件 handler

```rust
#[derive(RustEmbed)]
#[folder = "web-ui/dist/"]
struct Assets;

async fn spa_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref())], file.data).into_response()
        }
        // SPA fallback：客户端路由统一返回 index.html
        None => match Assets::get("index.html") {
            Some(f) => Html(f.data).into_response(),
            None    => StatusCode::NOT_FOUND.into_response(),
        },
    }
}
```

### 路由挂载顺序（src/web/routes.rs）

```rust
Router::new()
    .nest("/api", api_routes())   // 优先匹配 API
    .fallback(spa_handler)        // 其余路径交给 SPA
```

### build.rs 自动化

```rust
use std::process::Command;

fn main() {
    // web-ui/src 任意文件变更时重新触发
    println!("cargo:rerun-if-changed=web-ui/src");
    println!("cargo:rerun-if-changed=web-ui/package.json");
    println!("cargo:rerun-if-changed=web-ui/vite.config.ts");

    // 仅在 web feature 开启时构建前端
    if std::env::var("CARGO_FEATURE_WEB").is_ok() {
        let status = Command::new("npm")
            .args(["run", "build", "--prefix", "web-ui"])
            .status()
            .expect("npm run build 失败，请确保 Node.js 已安装");
        assert!(status.success(), "Vite 构建失败");
    }
}
```

> **CI / Docker 构建顺序：**  
> `npm ci --prefix web-ui` → `npm run build --prefix web-ui` → `cargo build --release --no-default-features --features web`

---

## §6 开发工作流

### 本地开发（双进程）

```bash
# 终端 1：启动 axum 后端
cargo run -- --web --port 8080

# 终端 2：启动 Vite dev server（HMR）
cd web-ui && npm run dev
```

`vite.config.ts` proxy 配置：

```ts
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api': 'http://localhost:8080',
    },
  },
  build: {
    outDir: 'dist',
  },
})
```

### 生产构建

```bash
npm run build --prefix web-ui
cargo build --release --no-default-features --features web
# build.rs 自动串联上述两步
```

---

## §7 API 调整范围

现有 API 端点保持兼容，按 UI 需要做最小扩展：

| 端点 | 变更 | 原因 |
|---|---|---|
| `GET /api/books/:url` | 新增 | 详情页独立路由需要按 URL 查询 |
| `GET /api/tasks/:id/stream` | 保持 | SSE 进度流不变 |
| `GET /api/library` | 新增 `ext` 过滤参数 | 书库 Tabs 筛选 |
| `POST /api/auth/verify` | **删除** | 移除鉴权机制 |
| `GET/POST /api/auth/*` | **删除** | 移除全部鉴权端点 |
| `POST /api/settings/access-code` | **删除** | 设置页安全分组移除 |
| 其余端点 | 不变 | — |

---

## §8 实现范围

### 本次包含

- `web-ui/` 前端项目脚手架（Vite + React + TypeScript + shadcn/ui）
- 全部6个页面组件（Search、BookDetail、Tasks、Library、Sources、Settings）
- TanStack Query hooks 层
- `build.rs` 自动化
- `rust-embed` 集成替换现有字符串内嵌方案
- 2个新 API 端点（`GET /api/books/:url`、`GET /api/library?ext=`）

### 本次不包含

- 桌面 GUI（GPUI 层）改动
- 后端爬取逻辑改动
- ~~用户鉴权机制~~（**删除**：移除现有 6 位访问码验证，Web UI 不再有登录页）

---

## §9 测试策略

- **前端单元测试：** Vitest + Testing Library，覆盖各 hook 和关键组件
- **API 集成验证：** 现有 `cargo test --lib` 覆盖后端逻辑，新端点补充单测
- **手工验收：** 本地跑双进程验证所有页面功能完整

---

*规格版本 v1.0 — 待实现*
