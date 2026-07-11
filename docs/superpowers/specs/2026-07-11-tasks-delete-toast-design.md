# Tasks 删除成功/失败 toast 通知

- Date: 2026-07-11
- Status: Approved
- Scope: `crates/desktop` —— Tasks 页面删除任务反馈
- Related: `src/desktop/pages/tasks/mod.rs:136-142`（现有 `prompt_delete` 的 `on_ok` 闭包）

## 背景

`TasksPage::prompt_delete` 已经实现了二次确认 Dialog：用户点删除 → 弹 Danger 风格确认框 → 确认后调 `AppModel::delete_task(task_id)`。但这条路径没有任何**事后反馈**：

- 成功时 UI 立即消失一条记录（`cx.notify(model_id_for_ok)`），用户可能没察觉行被删了。
- 失败时（运行中任务、不存在的 id）`delete_task` 返回 `false` 但调用方忽略返回值 —— UI 静默无反应。

虽然 UI 层入口**只对已结束任务渲染删除按钮**（`src/desktop/pages/tasks/row.rs:327-348` —— 删除按钮套在 `.when(!running, |this| { ... })` 内部），`delete_task` 内部另有 `is_running()` 兜底；但 race 条件（用户在确认对话框打开期间，任务被其它路径取消 / 完成 / 清除）下，`delete_task` 仍可能返回 `false`。

## 目标

删除确认后给用户明确的 toast 反馈：

- 成功 → Success 风格 toast，文案含书名。
- 仍然在运行 → Warning 文案（含书名）。
- 任务不存在 → Warning 文案。

## 方案 A（已选）：UI 层 `on_ok` 闭包里 push

### 改动清单

#### 1. `locales/app.yml`：3 个新 i18n key

放在 `Toasts:` block 末尾（紧跟 `library_delete_failed` 之后）：

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

三语完整，无兜底缺失（项目只支持这三种语言）。

#### 2. `src/desktop/model/tasks.rs:37` —— `delete_task` 改签名

当前：

```rust
pub fn delete_task(&mut self, id: u64) -> bool { ... }
```

改为 enum 区分两种 `false`：

```rust
/// `AppModel::delete_task` 的返回值。区分三种互斥结果，供 UI 决定哪条 toast 文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteTaskResult {
    Deleted,
    StillRunning,
    Missing,
}

pub fn delete_task(&mut self, id: u64) -> DeleteTaskResult {
    let Some(task) = self.tasks.iter().find(|t| t.id == id) else {
        return DeleteTaskResult::Missing;
    };
    if task.is_running() {
        return DeleteTaskResult::StillRunning;
    }
    self.tasks.retain(|t| t.id != id);
    let path = self.paths.tasks_file.clone();
    let tasks = self.tasks.clone();
    self.runtime.spawn_blocking(move || {
        if let Err(e) = crate::db::save_with_trim(&path, &tasks) {
            tracing::warn!("保存任务到文件失败: {e:#}");
        }
    });
    DeleteTaskResult::Deleted
}
```

`clear_finished_tasks`（邻居方法）不变；它已经是 `push_success` 内嵌，不影响本设计。

> **可见性**：`DeleteTaskResult` 是 `pub`，但 `mod tasks;` 在 `src/desktop/model/mod.rs:26` 已提升为 `pub(crate) mod tasks;` —— 模块路径对 crate 外不可达，因此外部 crate 即便看得到 `pub` 标记也无法命名这个类型、无法构造或 match。Rust 不允许 `pub fn ... -> pub(crate) Type`（返回类型必须 ≥ 函数可见性），所以 `pub` 是唯一合法选择；crate-外隐私靠模块路径而非 enum 自身达成。

#### 3. `src/desktop/pages/tasks/mod.rs:136-142` —— `on_ok` 闭包里 push 三种通知

当前实现：

```rust
.on_ok(move |_ev: &ClickEvent, _window, cx| {
    model_for_ok.update(cx, |m, _cx| {
        m.delete_task(task_id);
    });
    cx.notify(model_id_for_ok);
    true
})
```

改为：

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
            DeleteTaskResult::Missing => m.push_warning(ts(
                "Toasts.delete_task_missing",
            )),
        }
    });
    cx.notify(model_id_for_ok);
    true
})
```

**import 增量**（插在 `src/desktop/pages/tasks/mod.rs:40` 之后，与 `use crate::desktop::model::AppModel` 同行组）：

```rust
use crate::desktop::model::AppModel;
use crate::desktop::model::tasks::DeleteTaskResult;  // 新增
```

`name_for_ok` 已在闭包捕获（line 120：`let name_for_ok = name.clone();`），无需扩展捕获。`push_warning` / `push_success` / `push_event` 已存在于 `AppModel`（`src/desktop/model/mod.rs:186-204`），与 `clear_finished_tasks` 走同一通路。

## 错误处理

- **失败 toast 是防御性 UX**：`StillRunning` 在正常点击路径下不可达（UI 入口过滤掉），但 race 条件（用户在确认对话框打开期间，任务被其它路径清除 / 状态切换）下可达；覆盖二者对用户都是清晰反馈。
- **磁盘写入失败**：保留 `tracing::warn!`，不弹 toast。与 `clear_finished_tasks` 行为对齐 —— 罕见失败不应持续打扰用户。
- **i18n 缺失**：仅三种语言（zh-CN / zh-TW / en），新增 key 三语全填，无 fallback 风险。

## 闭包捕获 / 类型不变量

`Fn` 闭包约束保持不变：`name_for_ok` 是 `String` clone，`task_id` 是 `u64` Copy，`model_for_ok` 是 `Entity<AppModel>` clone —— 全部按值入闭包，无 `FnOnce` 风险。

`Dialog::confirm` / `ButtonVariant::Danger` 行为不变。

## 通知通路（复习）

```
AppModel::push_success / push_warning (model/mod.rs:186-204)
  └─ push_event(UIEvent::Success/Warning(msg))    // plain enum, 零 GUI 依赖
      └─ pending_ui_events: Vec<UIEvent>          // Model 内字段

RootView::render 每帧                                (root.rs:247-254)
  └─ mem::take(&mut model.pending_ui_events)
      └─ for ev in pending { ui_event_to_notification(ev) }
          └─ window.push_notification(notification, cx)  // 真正弹 toast

ui_event_to_notification (notifications.rs:19-32)
  └─ Notification::success / Notification::warning
```

`AppModel` 字段零改动；新增 toast 链路全部复用。

## 测试

### `src/desktop/model/tasks.rs` 内 `#[cfg(test)] mod tests`

测试模板沿用 `src/core/download_task.rs:280-308` 的 `task_with_rx` 工厂风格：

- `delete_task_returns_Deleted`: 构造 `DownloadTask { ..., finished: Some(Ok(PathBuf::from("/x"))) }` 放进 `model.tasks` → `assert_eq!(m.delete_task(1), DeleteTaskResult::Deleted)` + `assert!(m.tasks.is_empty())`。
- `running_task_returns_StillRunning`: 构造 `finished: None` 的 `DownloadTask` → `assert_eq!(m.delete_task(1), DeleteTaskResult::StillRunning)` + `assert_eq!(m.tasks.len(), 1)`。
- `missing_id_returns_Missing`: 空 `tasks: Vec<DownloadTask>` → `assert_eq!(m.delete_task(999), DeleteTaskResult::Missing)`。
- 顶部 `#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]`。
- `DownloadTask { id, origin: SearchResult::default(), rx: None, cancel: None, cancelling: false, started_at_unix: 0, finished_at_unix: None, book_meta: None, total_chapters: 0, completed: 0, failed: 0, last_chapter_title: String::new(), finished: ..., failures: Vec::new(), version: 0 }` —— 完整 16 字段 struct literal。
- 测试需要 `AppModel` 初始化；最低成本做法是 `AppModel::default_for_test()` 模式（如果存在）或用 `Self::new_with_wakeup()?.0` 真实构造（已有 `src/core/download_task.rs:368-401` 的 integration test 模式可参考）。若两种都太重，**退而求其次**：把 `delete_task` 三个分支抽成 `delete_task_inner(&mut Vec<DownloadTask>, id) -> DeleteTaskResult`，让测试只对纯函数测，避开 `AppModel` 构造。

### `src/desktop/model/ui_event.rs` 已有 enum 测试模板 —— 不动

新增 enum 只跟 `delete_task` 内部使用，UI 层 `use` 进来用。不在 `ui_event` 里加测试。

### i18n key 不单测

跟项目惯例一致（`Toasts.clear_tasks_ok` / `Toasts.delete_source_ok` 都没有专门测试）。

## 文件影响清单

| 文件 | 改动 |
|------|------|
| `locales/app.yml` | 新增 3 个 key（24 行） |
| `src/desktop/model/tasks.rs` | 新增 `DeleteTaskResult` enum + `delete_task` 改签名 + 3 个测试 |
| `src/desktop/pages/tasks/mod.rs` | `on_ok` 闭包改写 + 1 行 `use` |

无新增 crate 依赖。无 `Cargo.toml` 改动。

## 不在范围内

- 失败时的"重试"动作（点 toast 重新尝试）—— 现有 toast 只有文案/颜色，没有嵌入 action button；扩展需要 `UIEvent` 新增 variant，超出本次范围。
- 批量删除 —— Tasks 不计划支持（library / sources 也不支持批量）。
- 撤销删除（undo）—— 项目没有撤销框架，需要引入 timer + patch 状态，复杂度太高。
- 改 `clear_finished_tasks` 的 toast 文案 —— 那个走 `{n}` 占位符，跟单条删除不同语义。
