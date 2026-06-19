//! Library 页面：本地书库（下载目录里的电子书文件）。
//!
//! 行为：
//! - 进入页面时若 `library.scanned_dir` 为空 / 不匹配 `config.download_path` → 自动扫一次。
//! - 工具栏：文件名过滤输入 + 文件类型下拉（gpui-component `Select` + `SearchableVec`）。
//! - 列表：用 gpui-component `List`（虚拟滚动），通过 `LibraryDelegate` 把当前页（30 条）
//!   的 `LibraryEntry` 切片渲染成 `ListItem` 行（5 列：文件名 / 格式 / 大小 / 修改时间 / 3 动作）。
//! - 每页 30 条；分页页脚自写（gpui-component 0.5.1 没有 Pagination 组件）—— prev / 数字按钮
//!   + 省略号 / next，≤1 页时整段隐藏。
//! - 文件系统 watcher：long-lived `cx.spawn` 任务持有 `notify::RecommendedWatcher`，监听
//!   `config.download_path` 增量（`Create`/`Modify`/`Remove`），300 ms debounce 后触发
//!   `model.refresh_library()` + `cx.notify()`。`SetPath` 命令让任务内部 drop 旧 watcher
//!   并 arm 到新路径上。取消靠 `watcher_cmd_tx: Sender` 在 `LibraryPage` 析构时释放，
//!   任务的 `recv()` 收到 `None` → 循环退出。
//! - 删除走 `WindowExt::open_dialog` 二次确认（点删除按钮 → 打开 dialog → on_ok 调
//!   `model.delete_library_entry`，删完再触发一次 refresh）。
//! - 空态用 `EmptyState`（图标 + "本地书库为空" + 副标题）。

mod ctx;
mod row;
mod toolbar;
mod watcher;

use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, ClickEvent, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Window, div, px,
};
use gpui_component::{
    ActiveTheme as _, IconName, WindowExt, button::ButtonVariant, dialog::Dialog,
    dialog::DialogButtonProps, list::List, list::ListState, v_flex,
};

use crate::app::{AppModel, LibraryEntry};
use crate::gpui_app::components::{EmptyState, PageHeader, Pagination, compute_page_window};
use crate::i18n::{ts, ts_fmt};

use self::ctx::{WatcherCmd, WatcherCmdTx};
pub use self::row::LibraryDelegate;

/// Library 页面 entity。
pub struct LibraryPage {
    model: Entity<AppModel>,

    /// 文件名过滤 Input。沿用旧代码模式（struct 字段持有避免 click / focus 丢失）。
    filter_input: Entity<gpui_component::input::InputState>,

    /// gpui-component 虚拟列表。同理必须在 `new()` 里缓存。
    list_state: Entity<ListState<LibraryDelegate>>,

    /// 当前 0-based 页码。UI-only，每次路径或过滤变化时重置为 0。
    current_page: usize,

    /// 驱动的 watcher 任务命令通道。`LibraryPage` 析构 → sender drop → 任务 `try_recv()`
    /// 收到 `Err(Closed)` → 退出。
    ///
    /// 用 `smol::channel` 而非 `tokio::sync::mpsc` —— `cx.spawn` 跑在 smol executor 上，
    /// 那边没有 tokio reactor。smol 的 `Sender`/`Receiver` 都基于 `async-channel`，
    /// 跟 `tokio::sync::mpsc` 接口很像，但底层调度走 smol。
    watcher_cmd_tx: WatcherCmdTx,
}

impl LibraryPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 1. 文件名过滤 Input（沿用旧逻辑）。
        let filter_input = cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx)
                .placeholder(crate::i18n::ts("Library.filter_placeholder"))
        });
        cx.subscribe_in(&filter_input, window, |this, _state, event, _window, cx| {
            if matches!(event, gpui_component::input::InputEvent::Change) {
                let v = this.filter_input.read(cx).value();
                this.model.update(cx, |m, _cx| {
                    m.library.filter_text = v.to_string();
                });
                // 关键字变了 → 跳回第 1 页（避免卡在已不存在的页码上）。
                this.current_page = 0;
                cx.notify();
            }
        })
        .detach();

        // 2. 文件类型下拉 —— **不在 State 里实现**，改在 render 里用 button group
        // （见 `Render for LibraryPage` 里 toolbar 段的注释）。这里只持有过滤输入
        // 的 InputState（名字过滤），placeholder 在 state 上设初值。
        //
        // 第一个选项 "全部" 走 `ts()` 翻译，扩展名（epub / txt / zip / html / pdf）
        // **不译**——是技术名词，译成"电子出版物"反而看不懂。

        // 3. List + Delegate。
        //
        // delegate 必须能在 `ListDelegate::render_item` 里调 `LibraryPage::prompt_delete`
        // （拿 `&mut Window` + `Context<LibraryPage>` 打开 dialog），所以持有
        // `Entity<LibraryPage>` handle 而不是 WeakEntity —— Entity 永驻（`RootView` 持有），
        // 不会失效。
        let page_handle = cx.entity().clone();
        let delegate = LibraryDelegate::new(page_handle);
        let list_state = cx.new(|cx| ListState::new(delegate, window, cx));

        // 4. Watcher 任务。
        //
        // 用 `notify::recommended_watcher` 监听 `config.download_path`（非递归）——
        // 只关心下载目录第一层的 epub/txt/zip/html/pdf 文件变动。回调只做一件事：
        // `AtomicU64::fetch_add(1)`，回调所在线程（notify 自己的 OS 事件线程）开销
        // 接近 0。
        //
        // 主任务用 300 ms `background_executor().timer()` 轮询：
        // 每次醒来 → 1) 先 `try_recv()` drain 掉所有 cmd → 2) 看一眼 counter
        // 变化 → 有变就 `refresh_library` + `cx.notify()`。
        //
        // 具体实现见 `watcher::spawn`。详见 watcher.rs 顶部注释。
        let (watcher_cmd_tx, watcher_cmd_rx) = smol::channel::bounded::<WatcherCmd>(8);
        let initial_path = std::path::PathBuf::from(model.read(cx).config.download_path.clone());
        let page_weak = cx.entity().downgrade();
        let model_for_watcher = model.clone();

        cx.spawn(async move |_entity, async_cx| {
            watcher::run(
                initial_path,
                page_weak,
                model_for_watcher,
                watcher_cmd_rx,
                async_cx,
            )
            .await;
        })
        .detach();

        Self {
            model,
            filter_input,
            list_state,
            current_page: 0,
            watcher_cmd_tx,
        }
    }

    /// 设置文件类型过滤（None = "全部"，Some("epub") / Some("txt") / ...）。
    /// 跳回第 1 页（filter 变化后旧页码可能越界）。
    fn set_ext_filter(&mut self, new_ext: Option<String>, cx: &mut Context<Self>) {
        self.model.update(cx, |m, _cx| {
            m.library.filter_ext = new_ext;
        });
        self.current_page = 0;
        cx.notify();
    }

    /// 首次进入 / 下载目录变化时自动扫一次 + 通知 watcher 切目标。
    ///
    /// `set_ext_filter` / `refresh` 不走这里 —— 它们不改变路径。
    fn maybe_auto_scan(&mut self, cx: &mut Context<Self>) {
        let download_path =
            std::path::PathBuf::from(self.model.read(cx).config.download_path.clone());
        let already_scanned = self.model.read(cx).library.scanned_dir.clone();
        let need_scan = match &already_scanned {
            None => true,
            Some(p) => p != &download_path,
        };
        if need_scan {
            self.model.update(cx, |m, _cx| m.refresh_library_async());
            // 路径变了 → 让 watcher 任务重建监听目标。`try_send` 不阻塞，cap=8 不会满；
            // 失败（任务已退出）忽略。
            let _ = self
                .watcher_cmd_tx
                .try_send(WatcherCmd::SetPath(download_path));
            self.current_page = 0;
        }
    }

    /// 点"删除"按钮 → 弹 Dialog 二次确认。
    pub(super) fn prompt_delete(&self, path: PathBuf, window: &mut Window, cx: &mut App) {
        let model = self.model.clone();
        let model_id = model.entity_id();
        // 先取原始文件名（可能为空），空时下面用 i18n fallback 替。
        let raw_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let file_name: String = if raw_name.is_empty() {
            ts("Library.fallback_unknown_filename").to_string()
        } else {
            raw_name.to_string()
        };

        window.open_dialog(cx, move |dialog: Dialog, _window, _cx| {
            // 外层 dialog builder 是 Fn — 每次被 open_dialog 复用，闭包要能多次调用。
            // 因此 inner on_ok 也要能 Fn；model / path 都通过引用捕获，避开 FnOnce。
            let model_for_ok = model.clone();
            let path_for_ok = path.clone();
            let model_id_for_ok = model_id;

            dialog
                .title(ts("Library.delete_dialog.title"))
                // {file_name} 占位符由 ts_fmt 替换 —— 不能直接 `format!("...")` 拼字符串，
                // 否则切语言后占位符翻译也跟着拼，顺序会乱。
                .child(div().child(ts_fmt(
                    "Library.delete_dialog.message",
                    &[("file_name", &file_name)],
                )))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(ts("Library.delete_dialog.confirm_button"))
                        .cancel_text(ts("Library.delete_dialog.cancel_button"))
                        .ok_variant(ButtonVariant::Danger),
                )
                .confirm()
                .on_ok(move |_ev: &ClickEvent, _window, cx| {
                    model_for_ok.update(cx, |m, _cx| {
                        m.delete_library_entry(&path_for_ok);
                    });
                    cx.notify(model_id_for_ok);
                    true // 关闭 dialog
                })
        });
    }

    /// 过滤 + 排序后的 entry 列表（按修改时间倒序）。
    fn filtered_entries(model: &AppModel) -> Vec<LibraryEntry> {
        let mut v: Vec<LibraryEntry> = model
            .library
            .entries
            .iter()
            .filter(|e| {
                if let Some(ext) = &model.library.filter_ext {
                    if &e.ext != ext {
                        return false;
                    }
                }
                let kw = model.library.filter_text.trim();
                if kw.is_empty() {
                    return true;
                }
                let kw = kw.to_lowercase();
                e.file_name.to_lowercase().contains(&kw)
            })
            .cloned()
            .collect();
        v.sort_by_key(|e| std::cmp::Reverse(e.modified_unix_secs));
        v
    }
}

impl Render for LibraryPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.maybe_auto_scan(cx);

        // placeholder 在 `new()` 里建 InputState 时一次性设好（`Library.filter_placeholder`）。
        // 语言切换走重启生效（见 settings language setter），新进程重建 InputState 时
        // 拿到新 locale 的 placeholder，无需 render 里差量刷新。

        let model = self.model.read(cx);
        let entries = Self::filtered_entries(model);
        let total = entries.len();
        let scan_err = model.library.last_error.clone();
        let download_path = model.config.download_path.clone();
        let current_ext = model.library.filter_ext.clone();
        // 提前释放 `model` 的不可变借用，避免后面 `self.list_state.update(cx, ...)` 和
        // `render_pagination(... cx)` 的 borrow checker 打架（`update` 要 `&mut App`）。
        // 用 `let _ = model` 而不是 `drop(&model)` —— 后者对引用是 no-op。
        let _ = model;

        // 分页切片 + 兜底（如果外部清了 entries 导致 current_page 越界 → 回卷）。
        let w = compute_page_window(total, &mut self.current_page);
        // 每条带一个"全局序号" = 在完整 filtered 列表里的位置（0-based）。
        // 跨分页连续：page 0 → 0..29，page 1 → 30..59，等等。显示时 +1 变 1-based。
        // 存 (global_ix, entry) 而不是单存 entry，是为了让 delegate 不依赖
        // current_page / PAGE_SIZE —— 它只看到"这一行在完整列表里是第 N 个"。
        let page_items: Vec<(usize, LibraryEntry)> = if total == 0 {
            Vec::new()
        } else {
            entries[w.start..w.end]
                .iter()
                .enumerate()
                .map(|(local_ix, e)| (w.start + local_ix, e.clone()))
                .collect()
        };

        // 把当前页切片推给 delegate，List 渲染时会读到。
        self.list_state.update(cx, |state, _cx| {
            state.delegate_mut().page_items = page_items;
        });

        v_flex()
            .size_full()
            .p_6()
            .gap_3()
            // ---- 顶部 PageHeader：标题 + 下载目录副标题 ----
            // 右侧不带 action 按钮（用户偏好）—— watcher 已经实时刷新，不需要手动"刷新"按钮。
            // 副标题展示下载目录绝对路径，给用户"当前在看哪个文件夹"的视觉锚点，
            // 也是上面那个空旷区域的有用信息填充。
            .child(PageHeader::new(ts("Library.page_title")).subtitle(format!(
                "{}: {}",
                ts("Library.download_path_label"),
                std::path::Path::new(&download_path).display()
            )))
            // ---- toolbar: 文件名过滤 + 类型下拉 ----
            .child(toolbar::render(&self.filter_input, current_ext.as_deref(), cx))
            // ---- 错误提示 ----
            .when_some(scan_err, |this, err| {
                this.child(
                    div()
                        .p_3()
                        .rounded_md()
                        .bg(cx.theme().danger)
                        .text_color(cx.theme().danger_foreground)
                        .child(format!("{}: {err}", ts("Library.scan_failed"))),
                )
            })
            // ---- list / 空态 ----
            .child(if total == 0 {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        EmptyState::new(IconName::BookOpen, ts("Library.empty_title"))
                            .subtitle(ts("Library.empty_subtitle")),
                    )
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_md()
                    // List 整体 padding：`px(12.)` 水平。
                    // - 水平 12px：让 ListItem 行的右边不贴到滚动条，否则最右侧一行
                    //   选中时 ListItem 内部的 `list_active_border`（绝对定位 1px
                    //   边框）会被滚动条遮住一截，看起来选中框"缺了右侧"。
                    // - 垂直 4px：每行之间留点呼吸空间，但不要太大（避免行间距
                    //   喧宾夺主，文件列表本身紧凑更好读）。
                    // 参考 `crates/story/src/stories/list_story.rs:594-602`。
                    .child(List::new(&self.list_state).p(px(12.)).size_full())
                    .into_any_element()
            })
            // ---- 分页页脚（仅在列表非空时渲染 —— 空态不显示，避免无意义的"第 1 页 / 共 0 条"）----
            // 通用组件 `components::Pagination`，on_change 回调把 `current_page`
            // 写回 LibraryPage 字段 + cx.notify() 触发重渲染。
            .when(total > 0, |this| {
                this.child(Pagination::new(
                    self.current_page,
                    w.page_count,
                    cx.listener(|this, &new_page, _window, _cx| {
                        this.current_page = new_page;
                        _cx.notify();
                    }),
                ))
            })
    }
}
