# Web 前端 Monorepo 重写（模板为基准）— 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `web-ui/` 就地转为 Turborepo + Bun monorepo（`apps/web` + `packages/ui`），以用户提供的 `C:\Users\ThinkBook\Desktop\vite-monorepo` 模板为基准：用模板主题（globals.css）、base-nova 的 Base UI shadcn 组件，6 页 + 侧边栏 + 数据层全量迁入。

**Architecture:** 模板结构原样落入 `web-ui/`（root=turbo+bun workspaces）。`packages/ui`（`@workspace/ui`）承载 globals.css 主题 + base-nova Base UI 组件；`apps/web` 承载全部应用代码（数据层原样迁入、页面按 Base UI API 重写）。Rust 侧 embed 路径 `web-ui/dist` → `web-ui/apps/web/dist`，build.rs/Dockerfile 改用 bun 构建。

**Tech Stack:** Turborepo · Bun 1.4 · Vite 8 · React 19 + TS 6 · Tailwind v4 · shadcn/ui base-nova（Base UI `@base-ui/react`）· lucide-react · react-router v7 · react-query · i18next · sonner

**Spec:** [docs/superpowers/specs/2026-09-08-web-shadcn-redesign-design.md](../specs/2026-09-08-web-shadcn-redesign-design.md)（含「追加：Monorepo 重构方向」节）

## Global Constraints

- Bun 是唯一包管理器（根 `package.json` 用 bun + turbo；**不再用 npm**）。
- `web-ui/apps/web` 构建产物 = `web-ui/apps/web/dist`；Rust `#[folder]` 指到它。
- 组件一律从 `@workspace/ui/components/*` 导入（Base UI 变体，`render` prop 而非 `asChild`）。
- 数据层（lib/hooks/contexts/i18n）逻辑零改动，仅路径/导入调整。
- i18n 三语保留；react-query queryKey 不变；搜索轮询 800ms 不变。
- 后端 API 契约、桌面 GPUI、CLI 一律不动。
- 每 Task 结束：`cd web-ui && bun run build`（或对应子包 `bunx tsc --noEmit`）必须绿。

---

## M0 — 脚手架

### Task 1: 模板 monorepo 脚手架落位 + bun 安装

**Files:**
- Create: `web-ui/package.json`（root）、`web-ui/turbo.json`、`web-ui/apps/web/*`、`web-ui/packages/ui/*`
- 来源：`C:\Users\ThinkBook\Desktop\vite-monorepo` 对应文件

- [ ] **Step 1: 拷贝模板脚手架到 web-ui**

```bash
TPL="C:/Users/ThinkBook/Desktop/vite-monorepo"
cd "c:/Users/ThinkBook/Documents/GitHub/so-novel-rs"
# root 脚手架（不含 .git / node_modules / bun.lock——bun.lock 重新生成）
cp "$TPL/package.json" "$TPL/turbo.json" "$TPL/tsconfig.json" web-ui/
cp "$TPL/.prettierrc" "$TPL/.prettierignore" web-ui/ 2>/dev/null || true
# apps/web（保留模板 vite/tsconfig/package.json 骨架，index.html 后续改）
mkdir -p web-ui/apps
cp -r "$TPL/apps/web" web-ui/apps/web
# packages/ui
cp -r "$TPL/packages/ui" web-ui/packages/ui
```

- [ ] **Step 2: 调整 root package.json**

`web-ui/package.json` 的 `name` 改为 `"web-ui"`，其余（workspaces `apps/*`+`packages/*`、turbo scripts、`packageManager: "bun@1.4.0"`）保留模板原样。

- [ ] **Step 3: bun 安装 + 构建验证**

```bash
cd web-ui
bun install
bun run build        # turbo build → apps/web/dist
bunx tsc --noEmit    # 根 typecheck（或 bun run typecheck）
```

Expected: 模板默认 App（「Project ready!」按钮页）构建成功，`apps/web/dist` 存在。

- [ ] **Step 4: 提交**

```bash
git add web-ui/package.json web-ui/turbo.json web-ui/tsconfig.json web-ui/bun.lock web-ui/apps web-ui/packages
git commit -m "feat(web-ui): 按 vite-monorepo 模板落位 Turborepo+Bun monorepo 脚手架"
```

---

## M1 — packages/ui（主题 + 组件）

### Task 2: 主题 globals.css 就位 + 拉取 base-nova 组件

**Files:**
- Modify: `web-ui/packages/ui/src/styles/globals.css`（模板原样）
- Create: `web-ui/packages/ui/src/components/*.tsx`（bunx shadcn add 生成）

**Interfaces:**
- Consumes: `@base-ui/react`、`cn`、`class-variance-authority`、`shadcn`、`tw-animate-css`、`@fontsource-variable/geist`、`lucide-react`、`zod`（模板 package.json 已有）
- Produces: `@workspace/ui/components/*`（Base UI 变体）、`@workspace/ui/globals.css`、`@workspace/ui/lib/utils`（`export { cn } from "cn"`）

- [ ] **Step 1: 确认 globals.css 与 @source 路径**

模板 `packages/ui/src/styles/globals.css` 里的 `@source "../../../apps/**/*.{ts,tsx}"` 在 web-ui 根下解析为 `web-ui/apps/**`——结构一致无需改。确认 `@import "shadcn/tailwind.css"` 能解析（packages/ui 依赖 `shadcn` 包）。

- [ ] **Step 2: bunx shadcn 拉取组件（在 packages/ui 下）**

```bash
cd web-ui/packages/ui
bunx shadcn@latest add sidebar card button input select switch tabs badge \
  skeleton pagination progress dialog alert-dialog dropdown-menu tooltip \
  separator scroll-area alert empty sheet toggle toggle-group
```

bun 无 npm `allow-scripts` 限制，应直通。若 `empty` 等不在 registry，改用 `bunx shadcn@latest search -q "empty"` 查，或回退自定义空态。

- [ ] **Step 3: 核对组件**

逐一读 `packages/ui/src/components/*.tsx`：
- import 应为 `@workspace/ui/lib/utils` / `@base-ui/react/*` / `lucide-react`（无 `@/registry/...` 残留）
- 无缺 sub-component、图标库正确（lucide）
- 记录各组件实际导出的 prop 名（Select/Dialog/Tabs/ToggleGroup 等 Base UI 变体 API）—— Task 5-11 页面重写要照此写

- [ ] **Step 4: 验证**

```bash
cd web-ui && bun run build && bun run typecheck
```
Expected: packages/ui 无类型错误，turbo 全链过。

- [ ] **Step 5: 提交**

```bash
git add web-ui/packages/ui
git commit -m "feat(web-ui): packages/ui 主题就位 + base-nova Base UI 组件库"
```

---

## M2 — apps/web 脚手架与数据层

### Task 3: apps/web 应用脚手架（main/App/路由骨架）

**Files:**
- Modify: `web-ui/apps/web/package.json`、`web-ui/apps/web/vite.config.ts`、`web-ui/apps/web/index.html`
- Create: `web-ui/apps/web/src/main.tsx`、`web-ui/apps/web/src/App.tsx`、`web-ui/apps/web/src/components/theme-provider.tsx`（模板原样拷入）
- Delete: `web-ui/apps/web/src/App.tsx` 默认内容（重写为路由）

**Interfaces:**
- Consumes: `@workspace/ui/globals.css`、`@workspace/ui/components/*`；`@/lib/api`、`@/contexts/search-context`（Task 4 迁入后）
- Produces: 6 路由（`/search`、`/search/:bookUrl`、`/tasks`、`/library`、`/sources`、`/settings`）挂载点

- [ ] **Step 1: package.json 增依赖**

在 `web-ui/apps/web/package.json` 的 dependencies 加：
```json
"@tanstack/react-query": "^5",
"i18next": "^26",
"react-i18next": "^17",
"react-router-dom": "^7",
"sonner": "^2"
```
（`@workspace/ui`、`react`、`react-dom`、`lucide-react` 模板已有。）

- [ ] **Step 2: vite.config.ts**

```ts
import path from "path"
import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": path.resolve(__dirname, "./src") } },
  server: { proxy: { "/api": "http://localhost:8080" } },
})
```

- [ ] **Step 3: index.html**

title → `So Novel`；`lang="zh-CN"`；favicon 指向 `public/logo.png`（从 `web-ui/public/logo.png` 拷入 `apps/web/public/`）。

- [ ] **Step 4: theme-provider.tsx（模板原样）**

```bash
cp "C:/Users/ThinkBook/Desktop/vite-monorepo/apps/web/src/components/theme-provider.tsx" \
   web-ui/apps/web/src/components/theme-provider.tsx
```

- [ ] **Step 5: main.tsx**

```tsx
import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { BrowserRouter } from "react-router-dom"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { Toaster } from "sonner"

import "@workspace/ui/globals.css"
import { App } from "./App"
import { ThemeProvider } from "@/components/theme-provider"
import { SearchProvider } from "@/contexts/search-context"
import "@/i18n"

const queryClient = new QueryClient({
  defaultOptions: { queries: { staleTime: 30_000, retry: 1 } },
})

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <ThemeProvider>
          <SearchProvider>
            <App />
            <Toaster richColors position="top-right" />
          </SearchProvider>
        </ThemeProvider>
      </BrowserRouter>
    </QueryClientProvider>
  </StrictMode>,
)
```

- [ ] **Step 6: App.tsx 路由骨架**（引用 Task 4-11 产出的页面，先建占位导出，随后逐个实装）

```tsx
import { Navigate, Route, Routes } from "react-router-dom"
import Layout from "@/components/layout/layout"
import SearchPage from "@/routes/search"
import BookDetailPage from "@/routes/book-detail"
import TasksPage from "@/routes/tasks"
import LibraryPage from "@/routes/library"
import SourcesPage from "@/routes/sources"
import SettingsPage from "@/routes/settings"

export function App() {
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route index element={<Navigate to="/search" replace />} />
        <Route path="search" element={<SearchPage />} />
        <Route path="search/:bookUrl" element={<BookDetailPage />} />
        <Route path="tasks" element={<TasksPage />} />
        <Route path="library" element={<LibraryPage />} />
        <Route path="sources" element={<SourcesPage />} />
        <Route path="settings" element={<SettingsPage />} />
      </Route>
    </Routes>
  )
}
```

- [ ] **Step 7: 验证** — `cd web-ui && bun run build && bun run typecheck`
- [ ] **Step 8: 提交**

```bash
git add web-ui/apps/web
git commit -m "feat(web-ui): apps/web 脚手架（main/App/主题提供者/路由骨架）"
```

### Task 4: 数据层迁入 apps/web

**Files:**
- Copy: `web-ui/src/lib/{api,types,utils,language}.ts` → `web-ui/apps/web/src/lib/`
- Copy: `web-ui/src/i18n/` → `web-ui/apps/web/src/i18n/`
- Copy: `web-ui/src/hooks/use-*.ts` → `web-ui/apps/web/src/hooks/`（含 use-mobile.ts）
- Copy: `web-ui/src/contexts/search-context.tsx` → `web-ui/apps/web/src/contexts/`

**Interfaces:**
- Consumes: 无（纯搬迁）
- Produces: `@/lib/api`、`@/lib/types`、`@/lib/utils`、`@/lib/language`、`@/hooks/*`、`@/contexts/search-context`、`@/i18n`

- [ ] **Step 1: 搬迁文件（内容零改动，import 相对路径保持）**

```bash
cd web-ui
mkdir -p apps/web/src/{lib,hooks,contexts,i18n}
cp src/lib/api.ts src/lib/types.ts src/lib/utils.ts src/lib/language.ts apps/web/src/lib/
cp -r src/i18n apps/web/src/i18n
cp src/hooks/*.ts apps/web/src/hooks/
cp src/contexts/search-context.tsx apps/web/src/contexts/
```

- [ ] **Step 2: 验证**

`cd web-ui && bun run typecheck` —— 页面（Task 5-11）尚未迁入，App.tsx 引用的 `@/routes/*` / `@/components/layout/*` 未建，报错属预期；lib/hooks/contexts 本身不应有错。

- [ ] **Step 3: 提交**

```bash
git add web-ui/apps/web/src/lib web-ui/apps/web/src/hooks web-ui/apps/web/src/contexts web-ui/apps/web/src/i18n
git commit -m "feat(web-ui): 数据层迁入 apps/web（api/hooks/contexts/i18n 零改动）"
```

---

## M3 — 页面重写（Base UI）

> 逻辑从 `web-ui/src/routes/*` 照搬（保留分页折叠、轮询、自动保存、codeId 错误分派等全部行为），组件引用从 `@/components/ui/*`（Radix）改为 `@workspace/ui/components/*`（Base UI）。每个页面先 `bunx shadcn@latest view <component>` 或读 `packages/ui/src/components/*.tsx` 确认 Base UI 实际 prop 名，再写。

### Task 5: layout + sidebar + theme-toggle

**Files:**
- Create: `web-ui/apps/web/src/components/layout/layout.tsx`、`sidebar.tsx`、`web-ui/apps/web/src/components/theme-toggle.tsx`
- 参考: `web-ui/src/components/layout/{layout,sidebar}.tsx`、`web-ui/src/components/theme-toggle.tsx`（逻辑照搬）

**要点：**
- `layout.tsx`：`SidebarProvider` + `AppSidebar` + `SidebarInset`（`@workspace/ui/components/sidebar` 的 Base UI 变体导出名以实际为准）
- `sidebar.tsx`：NAV 5 项 + `/tasks` 下载中计数 + `activePath` 高亮（逻辑照搬）；`SidebarMenuButton` 的 `render`/`isActive` 按 Base UI 变体调整（`asChild` → `render={<NavLink .../>}`）
- `theme-toggle.tsx`：`useTheme` 从 `@/components/theme-provider`（模板自研，非 next-themes）；DropdownMenu 触发
- 验证：`cd web-ui && bun run typecheck && bun run build`

### Task 6: 搜索页

**Files:** Create `web-ui/apps/web/src/routes/search.tsx`
- 参考: `web-ui/src/routes/search.tsx`（PAGE_SIZE、pageItems 折叠、handleSearch、ResultCard、Alert 错误、骨架、空态全保留）
- 组件：`@workspace/ui/components/{button,input,select,badge,skeleton,alert,card,pagination}`——Select 的 Base UI API（Trigger/Value/Popup/Item）以实际为准
- 验证：`bun run typecheck && bun run build`

### Task 7: 书籍详情页

**Files:** Create `web-ui/apps/web/src/routes/book-detail.tsx`
- 参考: `web-ui/src/routes/book-detail.tsx`（sourceId null/isLoading/!book 三态、封面 fallback、TOC 懒加载、`await startDl → navigate('/tasks')`）
- 组件：button/card/badge/skeleton/scroll-area/toggle-group；格式选择 ToggleGroup 的 Base UI API
- 验证：同上

### Task 8: 任务页

**Files:** Create `web-ui/apps/web/src/routes/tasks.tsx`
- 参考: `web-ui/src/routes/tasks.tsx`（STATUS_MAP 配色、statusCounts、定量/不定 Progress、AlertDialog 删除、分页、下载完成按钮）
- 组件：badge/button/card/progress/pagination/alert-dialog
- 验证：同上

### Task 9: 书库页

**Files:** Create `web-ui/apps/web/src/routes/library.tsx`
- 参考: `web-ui/src/routes/library.tsx`（EXT_COLOR、Tabs 过滤+计数、AlertDialog、分页、空态）
- 组件：tabs/badge/card/button/skeleton/pagination/alert-dialog
- 验证：同上

### Task 10: 书源页

**Files:** Create `web-ui/apps/web/src/routes/sources.tsx`
- 参考: `web-ui/src/routes/sources.tsx`（并发测速回填、启停 badge、Switch）
- 组件：badge/button/card/switch；Loader2+animate-spin 代替 spinner
- 验证：同上

### Task 11: 设置页

**Files:** Create `web-ui/apps/web/src/routes/settings.tsx`
- 参考: `web-ui/src/routes/settings.tsx`（EditableSettings/validate/commit/update 防抖、codeId 3004/3005 分派、语言切换、只读区、SaveStatus；i18n key 用已修复的 `settings.saving/saved/saveFailed` + `settings.error.*`）
- 组件：card/input/select/switch；数字输入用原生 number input
- 验证：同上；最后 `git add web-ui/apps/web/src/routes && git commit -m "feat(web-ui): 6 页按 Base UI 重写迁入 monorepo"`

---

## M4 — Rust 集成与收尾

### Task 12: Rust embed 路径 + build.rs + Dockerfile

**Files:**
- Modify: `src/web/mod.rs`（`#[folder = "web-ui/dist/"]` → `"web-ui/apps/web/dist/"`）
- Modify: `build.rs`（rerun-if-changed 路径 + `npm run build --prefix web-ui` → `bun run build`，web-ui 根；`SO_NOVEL_SKIP_WEB_BUILD` 检查路径 `web-ui/dist/index.html` → `web-ui/apps/web/dist/index.html`）
- Modify: `Dockerfile`（前端 stage：装 bun 替代 npm；`COPY web-ui/package.json` → 拷 root+apps+packages 的 package.json 后 `bun install`；`npm run build --prefix web-ui` → `cd web-ui && bun run build`）
- Modify: `docs/WEB.md` / README 里 `web-ui/dist` 引用（grep 确认后改）

- [ ] **Step 1: 改 mod.rs embed 路径**
- [ ] **Step 2: 改 build.rs**（Windows 分支 `cmd /c npm ...` → `Command::new("bun").args(["run","build"])`，cwd 设为 web-ui；非 Windows 同理）
- [ ] **Step 3: 改 Dockerfile**（前端 stage 用 bun）
- [ ] **Step 4: 全量验证**

```bash
cd web-ui && bun install && bun run build     # 产物 apps/web/dist
cd .. && cargo build --features web            # build.rs 自动触发 bun 构建 + embed
cargo test --features web                      # 558 通过
```
- [ ] **Step 5: 提交** `git commit -m "feat: Rust 集成 monorepo（embed/build.rs/Dockerfile 切 bun）"`

### Task 13: 端到端验收 + 文档收尾

- [ ] **Step 1: 清理旧 web-ui**

删除已弃用的旧结构残留：`web-ui/src/`（旧独立应用）、`web-ui/package-lock.json`、`web-ui/node_modules`、`web-ui/tailwind.config.ts`（若模板不用）、`web-ui/components.json`（root 级，模板 apps/web 有自己的）。确认无 `@/components/ui` 或 `@heroui` 残留引用。

- [ ] **Step 2: 浏览器端到端走查**

```bash
cargo build --features web && ./target/debug/so-novel-rs.exe --web --port 8081
```
用 Playwright 无障碍快照（不截图）验证：6 页渲染、搜索轮询出结果、侧边栏/主题切换、深/浅色、三语切换。

- [ ] **Step 3: 文档更新**

README「技术栈」补 `web-ui` monorepo（turbo+bun+Base UI）；截图失效注记保留。

- [ ] **Step 4: 全量最终验证 + 提交**

```bash
cd web-ui && bun run build && bun run lint && bun run typecheck
cargo build --features web && cargo test --features web
```
`git commit -m "chore(web-ui): 清理旧结构，monorepo 重写完成"`
