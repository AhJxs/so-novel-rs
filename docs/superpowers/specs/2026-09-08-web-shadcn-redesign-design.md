# Web 前端迁移 shadcn UI + 全面重设计 + 后端 API 轮询化 — 设计文档

- 日期：2026-09-08
- 状态：已确认（4 节设计均获用户批准）

## 背景与目标

`web-ui/` 当前使用 HeroUI v3（`@heroui/react`）+ `@gravity-ui/icons`，后端 `src/web/` 用 axum + SSE 流推送搜索/下载进度。本次任务三合一：

1. **前端迁移到 shadcn/ui**（Nova preset）
2. **全面重设计前端页面**（侧边栏布局 + 六页视觉重做，保留现有页面核心与行为）
3. **后端 API 修改**：搜索/下载去掉 SSE 流，改为任务轮询模型

## 已确认决策

| 决策点 | 结论 |
|---|---|
| 重设计范围 | 全面重设计（信息架构保留 6 页核心，导航/布局/视觉重做） |
| API 修改目标 | 仅「简化 SSE → 任务轮询」（不补服务端过滤/单任务查询/阅读器） |
| 页面结构 | 保留现有 6 页核心（搜索/书籍详情/任务/书库/书源/设置） |
| 视觉风格 | shadcn 官方 preset **Nova** |
| 导航布局 | **侧边栏**（shadcn Sidebar，可折叠） |
| 改动编排 | **后端先行双阶段**（Phase A 后端+适配层 → Phase B 前端重设计） |

## 现状要点（实现时对照）

- 后端单源任务存储：`WebState.tasks: Mutex<Vec<DownloadTask>>`（持久化，`tasks.json`），id 由 `state.next_task_id` mint。
- 错误模型：`WebError` → JSON envelope `{ error: { code, code_id, message } }`，per-request locale 翻译；非 SSE handler 用 `read_state_or_json`。
- 下载管道：mint id → push `state.tasks` → spawn per-task drain（mpsc 消费 + 更新 task 状态 + broadcast 供 SSE）→ spawn crawler。SSE 只是 drain 的第二个出口。
- 搜索管道：`crawler::search::search_streaming(http, sources, keyword, limit, cf_bypass, tx)` 经 mpsc 逐源产出 `SourceSearchOutcome`；现消费端是 SSE 流。
- 前端 `SearchProvider`（跨路由保结果）+ `useDownload`（SSE 进度状态机）是仅有的两个 SSE 消费者；`src/lib/sse.ts` 是共享 SSE 解析器。
- 前端 UI 使用 HeroUI 专有 class（`text-default-500`、`bg-default-100`、`bg-surface`、`border-field-border` 等），需全部换成 shadcn 语义 token。

## Phase A — 后端 API 重构

### 搜索：SSE → 任务轮询

新增 3 个端点，替代 `GET /api/search`（SSE）：

| 端点 | 请求 | 响应 |
|---|---|---|
| `POST /api/search` | `{ keyword, source_id?, limit? }` | `201 { "task_id": 123 }` |
| `GET /api/search/{task_id}` | — | 轮询当前累计状态（见下） |
| `DELETE /api/search/{task_id}` | — | `204`，幂等（不存在也 204） |

`GET /api/search/{task_id}` 响应体：

```json
{
  "status": "running" | "done",
  "total_sources": 3,
  "done_sources": 2,
  "results": [ /* SearchResult，当前累计 */ ],
  "source_errors": [
    { "source_id": 1, "source_name": "某书源", "error": "按 locale 翻译的文案" }
  ]
}
```

设计要点：

- **增量轮询**：每次返回当前累计结果，前端每 ~800ms 轮询一次，结果渐进出现，保留现有"流式"观感；客户端只是简单轮询。
- **内存态**：新增 `WebState.search_tasks: Mutex<HashMap<u64, SearchTask>>` 与独立 id 计数（不复用 `next_task_id`）。`SearchTask = { id, keyword, created_at_unix, status, sources_total, sources_done, results: Vec<SearchResult>, source_errors: Vec<SourceSearchError> }`。
- **不持久化**：搜索结果是瞬态，进程重启即失。
- **生命周期**：crawler 跑完所有源 → `status = done`；TTL 10 分钟，每次 `POST /api/search` 时顺手 sweep 过期项；`DELETE` 显式清理。
- **复用爬虫**：`crawler::search::search_streaming` 的 mpsc 通道模型不变，消费端从 SSE 流改为累计进 task 状态。
- **错误处理**：task_id 不存在 → `WebError::NotFound`（404）；keyword 空 → `BadRequest`（400）；锁毒化走 `read_state_or_json` → `WebError::Internal`。全部复用现有 `ErrorCode`，不新增数字码。`source_errors` 内 error 文案按现有 `ErrorCode.key()` 翻译。

### 下载：SSE → JSON `task_id`

- `POST /api/download` 路径与 body 不变：`{ url, source_id, book_name?, format?, chapter_start?, chapter_end? }`。
- 响应从 SSE 流改为 `200 { "task_id": 123 }`。保留现有代码顺序：mint id → push `state.tasks`（+save）→ spawn drain → spawn crawler，最后返回 JSON 而非 SSE 流。
- **进度轮询**：前端用现有 `GET /api/tasks` + react-query `refetchInterval`（`useTasks` 已有该机制），不新增端点。
- **删除**：`ProgressEvent`、`BoxedSseStream`、broadcast channel（drain 的第二个出口）、download 路径的 `read_state_or_sse`/`lock_failure_stream`。锁错误改走 `read_state_or_json`。
- **保留**：`spawn_task_drain` 更新 `state.tasks` + 持久化的职责（SSE 转发只是它的一半工作，剥离后保留状态更新）。

### 路由变更（src/web/routes.rs）

```
+ POST   /api/search               → search::search_create
+ GET    /api/search/{task_id}     → search::search_status
+ DELETE /api/search/{task_id}     → search::search_delete
~ POST   /api/download             → download::download（改返回 JSON task_id）
- GET    /api/search               （SSE，移除）
```

其余端点（book/detail、book/toc、tasks、tasks/{id}/cancel、tasks/{id}、library、library/{filename}、files/{filename}、sources、sources/{id}/toggle、sources/{id}/test、settings GET/PUT、health）一律不动。

### 错误基建清理

- SSE 专用 `read_state_or_sse` 整体移除（最后两个使用方已改掉）。

### 测试（src/web/tests.rs）

- 新增：`POST /api/search` 建任务 → `GET /api/search/{id}` 轮询到 `done` 的集成测试。
- 新增：`POST /api/download` 返回 `{ task_id }` 的测试。
- 其余端点测试保持不动。

## Phase A — 前端适配层

目标：数据层切到新契约，UI 仍跑 HeroUI、可运行验证。**6 个 route + Navbar 不改**，它们只消费 hook 契约。

### api.ts（web-ui/src/lib/api.ts）

| 变更 | 内容 |
|---|---|
| 移除 | `searchBooks`（SSE）、`startDownload` 的 SSE 三件套（done/taskId/started promise）、`consumeSseStream` |
| 新增 | `createSearch(keyword, sourceId?) → { task_id }`；`getSearchStatus(taskId) → SearchStatus`；`deleteSearch(taskId) → void` |
| 改写 | `startDownload(opts) → { task_id }`（普通 JSON POST，直接返回 id） |
| 保留 | `apiFetch`/`ApiError`/`toApiError` 及全部 JSON 端点函数 |

### types.ts（web-ui/src/lib/types.ts）

- 移除 `SearchStreamEvent`、`SearchDoneEvent`、`DownloadProgressEvent`。
- 新增 `SearchStatus`、`SearchSourceError`（对齐后端轮询体）。
- 保留 `StartDownloadResult { task_id }` 作为 download 返回类型。

### 删除 sse.ts + sse.test.ts

Phase A 后无任何 SSE 消费者，整文件删除（含测试）。

### SearchProvider 轮询化（web-ui/src/contexts/search-context.tsx）

`search()` 从「SSE 流回调」改为「建任务 → 轮询」：

```
search(keyword, sourceId):
  abort 上次轮询（AbortController 存 ref，复用现有模式）
  setResults([])/setError(null)/setSourceCount(0)/setSearched(true)/setFetching(true)
  task_id = await createSearch(keyword, sourceId)
  循环:
    st = await getSearchStatus(task_id)
    setResults(st.results)
    setSourceCount(st.done_sources)
    st.source_errors[0] 存在 → setError(首条错误文案)
    if st.status === 'done' 或 signal.aborted → break
    sleep(800ms)
  finally: setFetching(false)
```

- **契约不变**：`results/isFetching/searched/sourceCount/error/search/reset` 原样导出，搜索页零改动。
- `sourceCount` 语义对应 `done_sources`（每完成一个源 +1，含出错源）。
- `reset()` / 新搜索替换旧任务时 fire-and-forget 调 `deleteSearch(task_id)`（服务端 TTL 兜底）。
- 轮询用 `sleep + while` 循环 + `signal.aborted` 检查。

### useDownload 简化（web-ui/src/hooks/use-download.ts）

SSE 进度状态机（`DownloadState` 六字段、abort ref、`handle.taskId` promise 桥）全部移除：

```
start(opts) → await startDownload(opts)   // POST 返回即任务已入 state.tasks
            → qc.invalidateQueries(['tasks'])
            → return
```

- 返回简化为 `{ start }`；`cancel`/`state` 删除（取消改由任务页 `POST /api/tasks/{id}/cancel` 负责）。
- book-detail.tsx 的 `await startDl(...)` → `navigate('/tasks')` 不改，行为等价。

### Phase A 前端影响文件

```
~ web-ui/src/lib/api.ts                  重写 search/download 部分
~ web-ui/src/lib/types.ts                类型增删
~ web-ui/src/contexts/search-context.tsx search() 轮询化
~ web-ui/src/hooks/use-download.ts       简化
- web-ui/src/lib/sse.ts                  删除
- web-ui/src/lib/sse.test.ts             删除
```

其余（routes/components/hooks/use-tasks 等）不动。

## Phase B — 前端全面重设计

### shadcn 初始化（Nova）

- `web-ui/` 下 `npx shadcn@latest init --preset nova`。components.json 已存在，CLI 就地应用；Nova CSS 变量写入 `web-ui/src/index.css`，替换现有 `@import "@heroui/styles"` + 手写 accent/field 覆盖。
- 依赖：移除 `@heroui/react`、`@gravity-ui/icons`；新增 `lucide-react` + shadcn 运行时依赖（CLI 自动装）。
- next-themes、sonner、i18next、react-query、react-router 全部保留。

### 添加的 shadcn 组件

```
sidebar  card  button  input  select  switch  tabs  badge  skeleton
pagination  progress  dialog  alert-dialog  dropdown-menu  tooltip
separator  scroll-area  spinner  alert  empty
```

HeroUI → shadcn 映射：`Chip→Badge`、`ProgressBar→Progress`、`SearchField→Input+图标`、`ButtonGroup→ToggleGroup/组合 Button`、`ConfirmDialog→AlertDialog`、`Spinner→Spinner`、`Tabs→Tabs`、`Pagination→Pagination`。

### 布局：侧边栏骨架

```
<SidebarProvider>
  <AppSidebar />              ← logo + 5 导航项 + 底部主题切换
  <SidebarInset>
    <header>页面标题 / 面包屑</header>
    <main class="p-4 lg:p-6 max-w-6xl"> <Outlet /> </main>
  </SidebarInset>
</SidebarProvider>
```

- 导航项复用现有 `NAV`（搜索/任务/书库/书源/设置）+ lucide 图标；/tasks 项挂「下载中」计数 Badge（现有逻辑保留）。
- 桌面可折叠（shadcn Sidebar 原生）；内容区 `max-w-5xl` 放宽到 `max-w-6xl`。
- `components/layout/navbar.tsx` 重构为侧边栏 + 内容区头栏。

### 共享组件迁移（web-ui/src/components/）

| 现有 | 改为 |
|---|---|
| app-select.tsx | shadcn Select |
| app-switch.tsx | shadcn Switch |
| number-input.tsx | shadcn 数字输入（Input type=number + 步进，或 base NumberField） |
| confirm-dialog.tsx | AlertDialog 封装，保留 `isOpen/onOpenChange/title/message/confirm/cancel` 契约 |
| theme-toggle.tsx | DropdownMenu（浅/深/跟随系统）+ lucide Sun/Moon |

### 六页重设计（Nova 视觉 + 保留全部现有行为）

| 页面 | 视觉 | 保留行为 |
|---|---|---|
| 搜索 | Input+图标 + 源 Select + Button；结果 Card 列表；流错误 Alert；Pagination | 渐进出结果、源计数、分页折叠、空态/初始态 |
| 书籍详情 | 双栏（封面+信息 Card ｜ TOC ScrollArea）；格式 ToggleGroup + Button | 封面 fallback、TOC 懒加载、await start→跳任务页 |
| 任务 | 状态 Badge 汇总 + 任务 Card（图标块/Progress/取消/删除）+ Pagination + AlertDialog | 定量/不定进度条、已结束保留进度、下载完成按钮、时间显示 |
| 书库 | Tabs（all/epub/... + Badge 计数）+ 文件 Card + Pagination + AlertDialog | 前端 ext 过滤、下载/删除、空态 |
| 书源 | 启/停 Badge 汇总 + 一键测速 Button + 列表 Card（状态点/延迟 Badge/Switch） | 并发测速、逐行结果回填 |
| 设置 | 分区 Card + 字段行（Input/Select/Switch/数字）+ 顶部保存状态 + 只读区 | 自动保存（防抖 800ms）、字段级校验、codeId 错误分派、语言即时切换 |

- 全部 HeroUI 专有 class（`text-default-500`、`bg-default-100` 等）换成 shadcn 语义 token（`text-muted-foreground`、`bg-muted` 等）。
- react-query queryKey（`['tasks']`/`['library']`/`['sources']`/`['settings']`）与 invalidate 逻辑保留。

## 测试与验证

### Phase A

- `cargo test`：新增搜索轮询集成测试 + 下载返回 task_id 测试；其余不动。
- `cargo build --features web`：移除 SSE 后编译通过。
- `npm run build`（= tsc --noEmit && vite build）+ `npm run lint`。
- 手工：HeroUI UI 下搜索（增量出结果）、下载（建任务→任务页进度）行为与现状等价。

### Phase B

- 每页迁移后 `npm run build` + `npm run lint`。
- 移除 HeroUI/gravity 依赖后 `npm run build`（残留引用会立即编译报错）。
- `cargo test` 后端回归（零改动）。
- 手工：6 页走查 + 深/浅色 + 三语切换 + 侧边栏折叠。

## 里程碑

| 里程碑 | 内容 | 验证 |
|---|---|---|
| M1 | Phase A 后端：搜索 3 端点 + 下载 JSON + routes + 移除 SSE 基建 + 新测试 | cargo test + cargo build |
| M2 | Phase A 前端适配：api/types 重写、删 sse(+test)、SearchProvider 轮询化、useDownload 简化 | npm build + lint + 手工搜索/下载 |
| M3 | Phase B 骨架：shadcn init(Nova)、index.css token、装组件、侧边栏布局、共享组件迁移 | build + 视觉走查 |
| M4 | Phase B 六页迁移（逐页提交）：搜索→详情→任务→书库→书源→设置 | 每页 build+lint，最后全页走查 |
| M5 | 收尾：移除 HeroUI/gravity、全量 lint+build+cargo test、更新 docs/WEB.md 与 README | 三端全绿 + 手工走查 |

## 文档更新（M5）

- `docs/WEB.md`：API 契约变化（SSE→轮询端点）。
- README：技术栈表（HeroUI→shadcn）、截图（旧截图失效，需用户重新截图）。
- web-ui 内如有 SSE 引用注释一并清理。

## 明确边界（不做）

- 不改桌面 GPUI（`src/desktop/`）与 CLI。
- 不加阅读器/书架首页（用户选择保留现有页面核心）。
- 不加服务端书库过滤/单任务查询端点（API 目标只选「简化 SSE」）。
- 不碰 `bundle/rules/` 书源规则与 `core/` 爬虫逻辑。

## 风险

- **搜索体验回归**：增量轮询的 800ms 延迟相对 SSE 有感知差异，但结果累计显示保住了渐进体验；若用户觉得卡顿可调小轮询间隔（实现期可参数化）。
- **Phase B 机械性替换面大**：HeroUI class → shadcn token 的批量替换易漏，靠 `npm run build` + 视觉走查兜底。
- **shadcn init 覆盖 index.css**：初始化会重写 index.css，现有自定义（accent 等）在 Nova 下不再需要，属预期。

---

## 追加：Monorepo 重构方向（2026-09-08）

> 上述设计（独立 web-ui + Radix shadcn）已实现并通过验收。用户随后提供
> shadcn monorepo 模板（`C:\Users\ThinkBook\Desktop\vite-monorepo`），决定
> **以模板为基准重写 web-ui**，本追加段覆盖该新方向。

### 已确认决策（追加）

| 决策点 | 结论 |
|---|---|
| monorepo 位置 | `web-ui/` 就地转 Turborepo + Bun monorepo（`apps/web` + `packages/ui`） |
| 包管理器 | **Bun**（1.4.0 已装；绕过 npm allow-scripts 限制） |
| 分支策略 | 继续 `web-shadcn-redesign` 分支（数据层沿用） |
| 迁移范围 | 全量：6 页 + 侧边栏 + 数据层迁入；后端只改 embed 路径 |
| 组件层 | 模板 base-nova 的 **Base UI** 组件（`@workspace/ui/components/*`，`render` prop） |
| 主题 | 模板 `globals.css`（Geist 字体 + `shadcn/tailwind.css` + base-nova neutral） |
| 主题切换 | 模板自研 ThemeProvider（localStorage `theme`，class light/dark，`d` 键），弃 next-themes |

### 目标结构

见 plan `2026-09-08-web-monorepo-rewrite.md` 的 File Structure 节。

### 关键改动

1. **packages/ui**（`@workspace/ui`）：模板 globals.css 主题 + `bunx shadcn add` 拉取 base-nova 组件。
2. **apps/web**：数据层（api/types/utils/language/hooks/contexts/i18n）原样迁入；6 页 + sidebar + layout 按 Base UI 组件 API 重写；theme-toggle 适配模板 useTheme。
3. **Rust 集成**：`#[folder = "web-ui/dist/"]` → `web-ui/apps/web/dist/`；`build.rs` 触发命令 `npm run build --prefix web-ui` → `bun run build`（web-ui 根，turbo）；Dockerfile 同理。
4. **验证**：`bun install && bun run build`（turbo）→ apps/web/dist；cargo build/test 回归；浏览器端到端走查。

### 边界（追加）

- 后端 API 契约不再动（轮询模型已落地）。
- 数据层 hooks/contexts 不改逻辑。
- 桌面 GPUI / CLI 不受影响。
