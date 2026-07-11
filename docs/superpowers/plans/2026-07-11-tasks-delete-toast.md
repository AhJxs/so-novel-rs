# Tasks 删除成功/失败 toast 通知 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `TasksPage::prompt_delete` 确认后给用户 toast 反馈：成功 → Success 文案；运行中 / 缺失 → Warning 文案。

**Architecture:** 把 `AppModel::delete_task(id) -> bool` 改成 `delete_task(id) -> DeleteTaskResult`（区分 `Deleted` / `StillRunning` / `Missing` 三个分支），UI 在 `on_ok` 闭包里 match 后调 `push_success` / `push_warning`。toast 链路全走既有 `pending_ui_events` 队列（`RootView::render` 每帧排空 + `ui_event_to_notification` 翻译 + `window.push_notification`）—— 零基础设施新增。

**Tech Stack:** Rust + GPUI + `gpui_component::notification` + `rust_i18n` + YAML locales。

## File Structure

| 文件 | 变更 | 职责 |
|------|------|------|
| `locales/app.yml` | modify (3 keys appended) | i18n 文案 |
| `src/desktop/model/tasks.rs` | modify (1 enum + 1 fn sig change + 1 inner fn extract + 3 tests) | 纯逻辑层：返回 enum |
| `src/desktop/pages/tasks/mod.rs` | modify (1 import + 1 closure body) | UI 层：match enum 推 toast |

零 `Cargo.toml` 改动。零新 crate 依赖。

## Global Constraints

来自 spec：
- `DeleteTaskResult` 可见性必须是 `pub(crate)`（`mod tasks;` 在 `src/desktop/model/mod.rs:26` 是私有声明，跨模块需要 `pub(crate)`）
- i18n key 必须三语齐备（`en` / `zh-CN` / `zh-TW`），无兜底缺失
- 文件末尾换行 LF；项目提交前会自动被 git core.autocrlf 转 CRLF
- 既有 `book_name` 是 `String` clone（`name_for_ok` 已在 `pages/tasks/mod.rs:120` 捕获），新闭包不引入新捕获
- 现有 `clear_finished_tasks`（`tasks.rs:13-30`）保留不动

---

## Task 1: i18n 文案 — 在 `locales/app.yml` 末尾追加 3 个 key

**Files:**
- Modify: `locales/app.yml` (line 92 之后，注：`library_delete_failed` 已经在 line 88-91)
- No new files

**Interfaces:**
- Consumes: existing `Toasts:` block（line 40-91）—— 同层级追加
- Produces: 3 个新 key，`ts_fmt` 调用方在后续 Task 3 引用：
  - `"Toasts.delete_task_ok"` — 模板 `"Deleted: \"{book_name}\""`（三语齐）
  - `"Toasts.delete_task_still_running"` — 模板含 `{book_name}`
  - `"Toasts.delete_task_missing"` — 无变量

- [ ] **Step 1: 追加 3 个 key 到 `Toasts:` block**

打开 `locales/app.yml`，找到 `Toasts:` block 末尾（line 91: `zh-TW: "刪除失敗: {err}"`），在该行**下一行**插空 1 行 + 新增 3 个 key。

**插入位置**（紧贴 line 91 之后、空一行）：

```yaml
  # 任务删除反馈（TasksPage::prompt_delete 的 on_ok → AppModel::delete_task）
  # {book_name} 与 Tasks.delete_dialog.message 同源 —— 调用方已做空名兜底。
  delete_task_ok:
    en: 'Deleted: "{book_name}"'
    zh-CN: '已删除: "{book_name}"'
    zh-TW: '已刪除: "{book_name}"'
  # false 原因 1：仍然在运行。
  delete_task_still_running:
    en: 'Cannot delete: "{book_name}" is still running'
    zh-CN: '无法删除: "{book_name}" 仍在运行'
    zh-TW: '無法刪除: "{book_name}" 仍在執行'
  # false 原因 2：任务已被并发清掉 / 不存在。
  delete_task_missing:
    en: 'Task no longer exists'
    zh-CN: '任务已不存在'
    zh-TW: '任務已不存在'
```

注意：
- 缩进与上文对齐（`Toasts:` 顶层，下面 `delete_source_ok` 等子项缩进 2 空格）
- YAML 字符串里嵌入双引号用**整段单引号**包起来（`'Deleted: "{book_name}"'`），不用 `""` 包 `""` 转义
- 三语 key 缺一不可，缺了 `ts_fmt` 会在 `ts_and_ts_fmt_work` 测试里报"missing locale"

- [ ] **Step 2: 验证 YAML 加载（编译 + i18n 测试）**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib i18n::tests::ts_and_ts_fmt_work -- --nocapture`
Expected: `test i18n::tests::ts_and_ts_fmt_work ... ok`

如果失败，错误信息会指出哪个 key 在哪个 locale 下解析失败 —— 直接定位到 `app.yml` 上一行。

- [ ] **Step 3: 提交**

```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && \
git add locales/app.yml && \
git commit -m "feat(i18n): add Tasks delete_task_ok / _still_running / _missing toast strings" -m "Three new keys under Toasts: block. Used by Desktop Tasks page's" \
"prompt_delete on_ok to push success/warning notifications after" \
"the dialog confirms deletion. Templates vary by DeleteTaskResult" \
"branch (Deleted / StillRunning / Missing)." -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 2: TDD — `DeleteTaskResult` enum + `delete_task` 重构（pure-fn 测）

**Files:**
- Modify: `src/desktop/model/tasks.rs` (改写整个文件; 引入 `DeleteTaskResult` enum + 抽出 inner fn + 3 单元测试)
- No new files

**Interfaces:**
- Consumes: `DownloadTask` 类型（`src/core/download_task.rs:38`） + `FinishedReason` enum（`src/models/task_record.rs:16`） + 既有 `AppModel::tasks: Vec<DownloadTask>` 字段
- Produces:
  - `pub(crate) enum DeleteTaskResult { Deleted, StillRunning, Missing }` —— `pub(crate)` 是为了 `pages/tasks/mod.rs` 跨模块可见
  - `pub fn delete_task(&mut self, id: u64) -> DeleteTaskResult` —— 保留 `pub`，签名变更（`-bool` → enum）
  - 私有 pure-fn `fn delete_task_inner(tasks: &mut Vec<DownloadTask>, id: u64) -> DeleteTaskResult` —— 仅文件内部使用；只做 find/retain，**不**碰 `self.runtime` / `self.paths` / 持久化

> **为什么抽 inner fn**：`AppModel::new_with_wakeup()` 构造要写 YAML 文件到磁盘，单元测试零成本构造它要 mock 一堆东西。spec 中讨论过的 Option A 是用 `new_with_wakeup().0` 直接造 —— 也能跑但慢 + 副作用风险。纯函数路径 (`Vec<DownloadTask>` 进/出) 沿用同项目既有的 `crate::core::download_task::tests::task_with_rx` 风格（`src/core/download_task.rs:280-308`），零依赖、零磁盘。

- [ ] **Step 1: 写 3 个失败测试**

打开 `src/desktop/model/tasks.rs`。在文件末尾（`impl AppModel` 闭括号之后）追加：

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use crate::models::{FinishedReason, SearchResult};
    use std::path::PathBuf;

    /// 构造一个最小 `DownloadTask`，16 个字段全填默认值；测试只关 `id` + `finished`。
    fn dummy_task(id: u64) -> DownloadTask {
        DownloadTask {
            id,
            origin: SearchResult::default(),
            rx: None,
            cancel: None,
            cancelling: false,
            started_at_unix: 0,
            finished_at_unix: None,
            book_meta: None,
            total_chapters: 0,
            completed: 0,
            failed: 0,
            last_chapter_title: String::new(),
            finished: None,
            failures: Vec::new(),
            version: 0,
        }
    }

    #[test]
    fn delete_task_inner_returns_Deleted_for_finished_task() {
        let mut tasks = vec![{
            let mut t = dummy_task(7);
            t.finished = Some(Ok(PathBuf::from("/tmp/x.epub")));
            t
        }];
        let result = delete_task_inner(&mut tasks, 7);
        assert_eq!(result, DeleteTaskResult::Deleted);
        assert!(tasks.is_empty(), "已结束任务应从 Vec 里移除");
    }

    #[test]
    fn delete_task_inner_returns_StillRunning_for_running_task() {
        let mut tasks = vec![dummy_task(7)]; // finished: None → 运行中
        let result = delete_task_inner(&mut tasks, 7);
        assert_eq!(result, DeleteTaskResult::StillRunning);
        assert_eq!(tasks.len(), 1, "运行中任务不应被 retain 删掉");
        assert_eq!(tasks[0].id, 7);
    }

    #[test]
    fn delete_task_inner_returns_Missing_for_unknown_id() {
        let mut tasks = vec![dummy_task(7)];
        let result = delete_task_inner(&mut tasks, 999);
        assert_eq!(result, DeleteTaskResult::Missing);
        assert_eq!(tasks.len(), 1, "Missing 不应触碰 Vec");
    }
}
```

- [ ] **Step 2: 运行测试，确认它们因"`delete_task_inner` not found / `DeleteTaskResult` not found"编译失败**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib desktop::model::tasks::tests --no-run 2>&1 | tail -20`
Expected: 编译错误 —— `cannot find function delete_task_inner` / `cannot find type DeleteTaskResult`。这是想要的红。

- [ ] **Step 3: 实现 —— 把整个文件改写为**

```rust
//! `AppModel` 任务管理方法
//!
//! `delete_task` 走 3 步骤：
//! 1. `delete_task_inner` 决定能不能删 —— 纯函数返回 `DeleteTaskResult` enum；
//! 2. `&mut self.delete_task` 包装 inner + fire-and-forget 落盘（`self.runtime.spawn_blocking(... crate::db::save_with_trim ...)`）；
//! 3. 调用方（`TasksPage::prompt_delete` 的 `on_ok` 闭包）match enum 决定 push 哪条 toast。

use crate::i18n::ts_fmt;

use super::{AppModel, ops};

/// `AppModel::delete_task` 的返回值。区分三种互斥结果，供 UI 决定哪条 toast 文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeleteTaskResult {
    Deleted,
    StillRunning,
    Missing,
}

impl AppModel {
    /// 清掉所有已结束的任务。
    pub fn clear_finished_tasks(&mut self) {
        let before = self.tasks.len();
        ops::clear_finished_tasks(&mut self.tasks);
        let removed = before - self.tasks.len();
        if removed > 0 {
            let path = self.paths.tasks_file.clone();
            let tasks = self.tasks.clone();
            self.runtime.spawn_blocking(move || {
                if let Err(e) = crate::db::save_with_trim(&path, &tasks) {
                    tracing::warn!("保存任务到文件失败: {e:#}");
                }
            });
            self.push_success(ts_fmt(
                "Toasts.clear_tasks_ok",
                &[("n", &removed.to_string())],
            ));
        }
    }

    /// 删除单条任务记录（仅已结束的，运行中跳过）。
    ///
    /// 返回 `DeleteTaskResult`：
    /// - `Deleted`：找到且已结束，内存 `tasks` retain 移除 + 异步落盘。
    /// - `StillRunning`：找到但还在跑，**不删** —— UI 入口已过滤运行中任务，这里兜底 race。
    /// - `Missing`：id 不存在 —— 同上兜底 concurrent delete。
    ///
    /// 落盘失败由 `tracing::warn!` 记日志，不弹 toast —— 与 `clear_finished_tasks` 行为一致。
    pub fn delete_task(&mut self, id: u64) -> DeleteTaskResult {
        let result = delete_task_inner(&mut self.tasks, id);
        if result == DeleteTaskResult::Deleted {
            let path = self.paths.tasks_file.clone();
            let tasks = self.tasks.clone();
            self.runtime.spawn_blocking(move || {
                if let Err(e) = crate::db::save_with_trim(&path, &tasks) {
                    tracing::warn!("保存任务到文件失败: {e:#}");
                }
            });
        }
        result
    }
}

/// 纯函数：`delete_task` 的内存逻辑部分 —— 找 + retain。
///
/// 抽出来是为了让单元测试不必造 `AppModel`（后者要从磁盘读 yaml）。落盘逻辑保留
/// 在 `delete_task` 这层，因为持久化路径是 `&self` 的，不在内层函数可达范围。
fn delete_task_inner(tasks: &mut Vec<DownloadTask>, id: u64) -> DeleteTaskResult {
    let Some(task) = tasks.iter().find(|t| t.id == id) else {
        return DeleteTaskResult::Missing;
    };
    if task.is_running() {
        return DeleteTaskResult::StillRunning;
    }
    tasks.retain(|t| t.id != id);
    DeleteTaskResult::Deleted
}
```

注意：
- `delete_task_inner` 是私有 fn（无 `pub`），仅供同文件 `delete_task` 与 `tests` 模块引用
- `delete_task_inner` 调 `task.is_running()` —— 项目里 `DownloadTask::is_running()` 是已存在方法（`TaskSummary::is_running` 映射 `finished.is_none()` 同语义）。**如果 `DownloadTask` 上还没有这个方法**，改 `task.finished.is_none()` 等价 1 行表达式即可
- `FinishedReason` / `PathBuf` 的 `use` 只在 `tests` 模块内需要 —— **不要**放到顶层 `use` 块以免污染

- [ ] **Step 4: 运行测试，确认全部通过**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib desktop::model::tasks::tests -- --nocapture`
Expected:
```
test desktop::model::tasks::tests::delete_task_inner_returns_Deleted_for_finished_task ... ok
test desktop::model::tasks::tests::delete_task_inner_returns_Missing_for_unknown_id ... ok
test desktop::model::tasks::tests::delete_task_inner_returns_StillRunning_for_running_task ... ok
```

- [ ] **Step 5: 跑整个 lib 测试，确保没有回归（主要是 i18n 与 `download_task` 测试群）**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib 2>&1 | tail -30`
Expected: all tests pass，无 i18n / download_task 回归。

- [ ] **Step 6: 提交**

```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && \
git add src/desktop/model/tasks.rs && \
git commit -m "refactor(model): delete_task returns DeleteTaskResult enum" -m "Splits delete_task into a pure-fn inner (delete_task_inner on Vec<DownloadTask>)"
"and a thin &mut self wrapper that handles fire-and-forget persistence."
"Pure-fn inner is unit-tested for all three branches (Deleted /"
"StillRunning / Missing). DeleteTaskResult is pub(crate) for the UI"
"match arm in pages/tasks/mod.rs to import." -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 3: UI 集成 — 在 `pages/tasks/mod.rs` 的 `on_ok` 闭包里 match enum 推 toast

**Files:**
- Modify: `src/desktop/pages/tasks/mod.rs` (line 40 后插 1 个 `use`; line 136-142 改写 `on_ok` 闭包 body)
- No new files

**Interfaces:**
- Consumes:
  - `DeleteTaskResult` from `crate::desktop::model::tasks::DeleteTaskResult` (Task 2 产出)
  - `ts` / `ts_fmt` from `crate::i18n` (line 41 已有)
  - `AppModel::push_success` / `push_warning` (line 186-199 in `model/mod.rs`)
  - 既有 `name_for_ok: String` (line 120 已 clone 入闭包捕获)
- Produces: 改写后 `on_ok` 闭包：
  - 调用 `m.delete_task(task_id)` 拿 enum
  - `Deleted` → `m.push_success(ts_fmt("Toasts.delete_task_ok", &[("book_name", &name_for_ok)]))`
  - `StillRunning` → `m.push_warning(ts_fmt("Toasts.delete_task_still_running", &[("book_name", &name_for_ok)]))`
  - `Missing` → `m.push_warning(ts("Toasts.delete_task_missing"))`
  - 末尾仍 `cx.notify(model_id_for_ok); true` —— 列表刷新逻辑保持

- [ ] **Step 1: 加 import**

打开 `src/desktop/pages/tasks/mod.rs`，在 line 40 (`use crate::desktop::model::AppModel;`) 之后**下一行**插：

```rust
use crate::desktop::model::AppModel;
use crate::desktop::model::tasks::DeleteTaskResult;
```

注意：
- 缩进 0（顶层 use），无 trailing semicolon 已有
- `crate::desktop::model::tasks` 路径上 `mod tasks` 是私有，但 `DeleteTaskResult` 是 `pub(crate)` —— 跨模块可见（在 `desktop` crate 内）

- [ ] **Step 2: 改写 `on_ok` 闭包**

当前 line 136-142：

```rust
.on_ok(move |_ev: &ClickEvent, _window, cx| {
    model_for_ok.update(cx, |m, _cx| {
        m.delete_task(task_id);
    });
    cx.notify(model_id_for_ok);
    true // 关闭 dialog
})
```

替换为：

```rust
.on_ok(move |_ev: &ClickEvent, _window, cx| {
    model_for_ok.update(cx, |m, _cx| {
        match m.delete_task(task_id) {
            DeleteTaskResult::Deleted => m.push_success(ts_fmt(
                "Toasts.delete_task_ok",
                &[("book_name", &name_for_ok)],
            )),
            DeleteTaskResult::StillRunning => m.push_warning(ts_fmt(
                "Toasts.delete_task_still_running",
                &[("book_name", &name_for_ok)],
            )),
            DeleteTaskResult::Missing => m.push_warning(
                ts("Toasts.delete_task_missing"),
            ),
        }
    });
    cx.notify(model_id_for_ok);
    true // 关闭 dialog
})
```

注意：
- `Fn` 闭包约束：`name_for_ok` 按值已在 line 120 `clone` 入捕获，`task_id: u64` Copy，`model_for_ok` 也是 `clone`，**全部按值入闭包** —— 仍满足 `Fn` 而不是 `FnOnce`
- 三种 push 调用全部走 `AppModel` 已有的方法（`push_event` → `pending_ui_events` → Root render 排空 → toast）—— 零基础设施变动
- `m.delete_task(task_id)` 拿 `bool` 换成 `DeleteTaskResult`，但调用语法未变

- [ ] **Step 3: 编译 desktop feature，验证零编译错误**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo check -p so-novel-rs --features desktop 2>&1 | tail -30`
Expected: 编译通过，无错误。可能有数条关于未用 `let` `_cx` 等**既有警告**——本任务不引入新警告。如果出现 `error[E0433]: failed to resolve: could not find 'DeleteTaskResult' in 'tasks'`，是 Step 1 的 `use` 没改对路径。

- [ ] **Step 4: 跑 desktop 相关测试**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib desktop 2>&1 | tail -30`
Expected: 全部通过（之前 Task 2 的 3 个测试 + 既有 `desktop::*` 测试群均无回归）。

- [ ] **Step 5: 提交**

```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && \
git add src/desktop/pages/tasks/mod.rs && \
git commit -m "feat(desktop): Tasks prompt_delete on_ok pushes success/warning toast" -m "After the user confirms task deletion, the on_ok closure now"
"matches DeleteTaskResult from AppModel::delete_task and pushes"
"the appropriate UIEvent::Success (Deleted) or UIEvent::Warning"
"(StillRunning / Missing). i18n strings live in locales/app.yml's"
"Toasts block. Toast rendering reuses the existing pending_ui_events"
"→ RootView::render → window.push_notification pipeline — no"
"infrastructure added." -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Self-Review Checklist (执行者用)

执行每个 task 前请确认：

1. **Spec 覆盖率**：
   - Spec §1 (yaml) → Task 1 ✓
   - Spec §2 (enum + signature) → Task 2 ✓
   - Spec §3 (on_ok 闭包) → Task 3 ✓
   - Spec §4 (错误处理) → Task 2/3 隐含覆盖（race 时拿 enum，逻辑安全）
   - Spec §5 (i18n 测试) → Task 1 Step 2 + 不为新增 key 写专测（沿用项目惯例 + 现有 `ts_and_ts_fmt_work` 已覆盖加载链路）

2. **占位符扫描**：
   - 全文无 `TBD` / `TODO` / `implement later`
   - 每个 Step 有具体代码块或具体命令
   - 没有 "类似 Task N" 的引用 —— 3 个任务彼此独立无复用

3. **类型一致性**：
   - `DeleteTaskResult` 在 Task 2 写成 `pub(crate) enum { Deleted, StillRunning, Missing }`，Task 3 import 路径 `crate::desktop::model::tasks::DeleteTaskResult` 与 mod.rs:40 同一行组
   - `delete_task_inner` 签名 `(tasks: &mut Vec<DownloadTask>, id: u64) -> DeleteTaskResult` 在 Task 2 Step 1 测试和 Step 3 实现完全一致
   - i18n key 在 Task 1 yaml 和 Task 3 代码里 100% 字面匹配（`"Toasts.delete_task_ok"` 等）

4. **commit 顺序**：
   - Task 1 yaml → Task 2 enum + tests → Task 3 UI 集成
   - 三个 commit 单独可 bisect（每步 cargo test 都过）
