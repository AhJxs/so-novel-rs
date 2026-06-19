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

use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, ClickEvent, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Window, div, px,
};
use gpui_component::StyledExt;
use gpui_component::{
    ActiveTheme as _, Icon, IconName, IndexPath, Selectable, Sizable, WindowExt,
    button::{Button, ButtonVariant, ButtonVariants as _},
    dialog::{Dialog, DialogButtonProps},
    h_flex,
    input::{Input, InputEvent, InputState},
    list::{List, ListDelegate, ListItem, ListState},
    tag::Tag,
    v_flex,
};

use crate::app::{AppModel, LibraryEntry};
use crate::gpui_app::components::{
    EmptyState, PageHeader, Pagination, compute_page_window, format_size, truncate,
};
use crate::gpui_app::i18n::{ts, ts_fmt};
use crate::util::system::{open_path, reveal_in_folder};

/// Watcher 任务命令：让任务内部 drop 旧 watcher 并 arm 到新路径上。
///
/// 当前只有 `SetPath` 一个调用方（`maybe_auto_scan` 检测到 `download_path` 变了 → 发），
/// `Stop` 预留未来"暂停监听"开关使用。
#[derive(Debug, Clone)]
enum WatcherCmd {
    SetPath(PathBuf),
    #[allow(dead_code)]
    Stop,
}

/// Library 页面 entity。
pub struct LibraryPage {
    model: Entity<AppModel>,

    /// 文件名过滤 Input。沿用旧代码模式（struct 字段持有避免 click / focus 丢失）。
    filter_input: Entity<InputState>,

    /// 文件类型下拉 —— **不用** SelectState（SelectState 持有 options 翻译字段，
    /// 切语言不会自动更新）。改用**自定义按钮组**：6 个 Button，`selected` 状态从
    /// `model.library.filter_ext` 读（`None` = "全部"），label 在 render 里现取 `ts(...)`。
    /// 切语言自动同步。详见 `Render for LibraryPage` 里 toolbar 段的注释。

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
    watcher_cmd_tx: smol::channel::Sender<WatcherCmd>,
}

impl LibraryPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 1. 文件名过滤 Input（沿用旧逻辑）。
        let filter_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(ts("Library.filter_placeholder")));
        cx.subscribe_in(&filter_input, window, |this, _state, event, _window, cx| {
            if matches!(event, InputEvent::Change) {
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
        // **不能用** `tokio::time::sleep` / `tokio::sync::mpsc` —— `cx.spawn` 内部跑在
        // gpui 的 smol-based executor 上，那边没有 tokio reactor，会 panic
        // "there is no reactor running"。通道用 `smol::channel`（基于 `async-channel`），
        // 定时器走 `async_cx.background_executor().timer(...)`（参考
        // `src/app/events.rs:97-119 spawn_drain_loop`）。这跟 gpui-component 内部
        // `ThemeRegistry::watch_dir` 的选型一致（`registry.rs:171-206`）。
        //
        // **debounce**：300 ms 心跳周期天然就是 debounce —— 一次写文件触发 3~4
        // 个事件（Create + Modify + 2× Rename），间隔 <100 ms，300 ms 内会全部累计
        // 进 counter，下一次心跳一次 rescan 把它们压平。
        //
        // **取消**：`LibraryPage` 析构 → `watcher_cmd_tx: Sender` drop →
        // `watcher_cmd_rx.try_recv()` 返回 `Err(Closed)` → break 退出循环。
        // `notify::Watcher` 随 `_watcher` 局部变量 drop → 释放 OS handle。
        // 本项目 `LibraryPage` 在 `RootView` 整个 app 寿命常驻，正常不会触发。
        let (watcher_cmd_tx, watcher_cmd_rx) = smol::channel::bounded::<WatcherCmd>(8);
        let initial_path = std::path::PathBuf::from(model.read(cx).config.download_path.clone());
        let page_weak = cx.entity().downgrade();
        let model_for_watcher = model.clone();

        cx.spawn(async move |_weak, async_cx| {
            use std::sync::Arc;
            use std::sync::atomic::{AtomicU64, Ordering};
            use std::time::Duration;

            use notify::{RecommendedWatcher, RecursiveMode, Watcher};

            // 事件计数器（每个 fs 事件 +1）。回调线程写入，主任务读取 → Relaxed 即可。
            let counter = Arc::new(AtomicU64::new(0));
            let mut _watcher: Option<RecommendedWatcher> = None;

            // helper：arm 当前路径的 watcher。失败仅 warn，不 panic。
            let arm = |path: PathBuf, counter: Arc<AtomicU64>| -> Option<RecommendedWatcher> {
                let counter_for_cb = counter.clone();
                let mut w = match notify::recommended_watcher(move |_res| {
                    counter_for_cb.fetch_add(1, Ordering::Relaxed);
                }) {
                    Ok(w) => w,
                    Err(e) => {
                        tracing::warn!("create watcher failed: {e:#}");
                        return None;
                    }
                };
                if let Err(e) = w.watch(&path, RecursiveMode::NonRecursive) {
                    tracing::warn!("watch {:?} failed: {e:#}", path);
                    return None;
                }
                Some(w)
            };

            // 初始 arm。
            _watcher = arm(initial_path.clone(), counter.clone());
            let debounce = Duration::from_millis(300);
            let mut last_seen = 0u64;

            loop {
                // smol-based timer：smol executor 提供的 timer future，
                // 不需要 tokio reactor。在 smol runtime（= gpui 的 `cx.spawn` 内部）
                // 上 await 直接走 smol reactor，没问题。
                async_cx.background_executor().timer(debounce).await;

                // 1) drain cmd channel（处理所有待发的 SetPath / Stop）—— try_recv 非阻塞。
                loop {
                    match watcher_cmd_rx.try_recv() {
                        Ok(WatcherCmd::SetPath(new_path)) => {
                            // drop 旧 watcher → 释放 OS handle → arm 新 watcher。
                            _watcher = None;
                            _watcher = arm(new_path.clone(), counter.clone());
                            // 切路径后立即 rescan（用户在 Settings 切完路径想马上看到新目录内容）。
                            // 改用 async 版本：read_dir / metadata 阻塞 IO 不再卡 UI 帧。
                            let _ = page_weak.update(async_cx, |_p, cx| {
                                model_for_watcher.update(cx, |m, cx| {
                                    m.refresh_library_async(cx);
                                });
                                cx.notify();
                            });
                        }
                        Ok(WatcherCmd::Stop) => {
                            _watcher = None;
                        }
                        Err(smol::channel::TryRecvError::Empty) => break, // 队列空，跳出内层循环
                        Err(smol::channel::TryRecvError::Closed) => return, // sender drop → 整个任务退出
                    }
                }

                // 2) 检查 counter：有新事件 → rescan + notify。
                let now = counter.load(Ordering::Relaxed);
                if now != last_seen {
                    last_seen = now;
                    // 如果刚发生删除（delete_library_entry 置了 1s skip 窗口），
                    // 跳过此次 rescan —— 避免 entries.clear() + 后台 fill 制造的
                    // "empty → 重新加载" 闪一下。1s 后窗口过期，正常的 add/modify
                    // 事件仍会触发 rescan。
                    let skip_due_to_delete = page_weak
                        .update(async_cx, |_p, _cx| {
                            model_for_watcher
                                .read(_cx)
                                .library
                                .watcher_skip_until_unix_ms
                        })
                        .unwrap_or(None)
                        .map(|until| {
                            let now_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0);
                            now_ms < until
                        })
                        .unwrap_or(false);
                    if !skip_due_to_delete {
                        let _ = page_weak.update(async_cx, |_p, cx| {
                            model_for_watcher.update(cx, |m, cx| {
                                m.refresh_library_async(cx);
                            });
                            cx.notify();
                        });
                    }
                }
            }
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
            self.model.update(cx, |m, cx| m.refresh_library_async(cx));
            // 路径变了 → 让 watcher 任务重建监听目标。`try_send` 不阻塞，cap=8 不会满；
            // 失败（任务已退出）忽略。
            let _ = self
                .watcher_cmd_tx
                .try_send(WatcherCmd::SetPath(download_path));
            self.current_page = 0;
        }
    }

    /// 点"删除"按钮 → 弹 Dialog 二次确认。
    fn prompt_delete(&self, path: PathBuf, window: &mut Window, cx: &mut App) {
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let page_handle = cx.entity().clone();
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
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        // 名字过滤 Input —— placeholder **在 state 上**（gpui-component
                        // API 限制），render 顶部用 sentinel 检测切语言。
                        Input::new(&self.filter_input).w(px(280.0)).prefix(
                            Icon::new(IconName::Search)
                                .small()
                                .text_color(cx.theme().muted_foreground),
                        ),
                    )
                    .child(
                        // **文件类型过滤改用 button group** —— 不用 SelectState：
                        // SelectState 持有 options 翻译字段，切语言不会自动更新。
                        // Button group 在 render 里现取 `ts(...)`，6 个 Button + selected
                        // 状态从 `model.library.filter_ext` 读（`None` = "全部"）。
                        // 切语言自动同步，不需要任何 sentinel。
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                Button::new("ext-all")
                                    .small()
                                    .ghost()
                                    .selected(current_ext.is_none())
                                    .label(ts("Library.filter_option_all"))
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.set_ext_filter(None, cx);
                                    })),
                            )
                            .child(
                                Button::new("ext-epub")
                                    .small()
                                    .ghost()
                                    .selected(current_ext.as_deref() == Some("epub"))
                                    .label("epub")
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.set_ext_filter(Some("epub".to_string()), cx);
                                    })),
                            )
                            .child(
                                Button::new("ext-txt")
                                    .small()
                                    .ghost()
                                    .selected(current_ext.as_deref() == Some("txt"))
                                    .label("txt")
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.set_ext_filter(Some("txt".to_string()), cx);
                                    })),
                            )
                            .child(
                                Button::new("ext-zip")
                                    .small()
                                    .ghost()
                                    .selected(current_ext.as_deref() == Some("zip"))
                                    .label("zip")
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.set_ext_filter(Some("zip".to_string()), cx);
                                    })),
                            )
                            .child(
                                Button::new("ext-html")
                                    .small()
                                    .ghost()
                                    .selected(current_ext.as_deref() == Some("html"))
                                    .label("html")
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.set_ext_filter(Some("html".to_string()), cx);
                                    })),
                            )
                            .child(
                                Button::new("ext-pdf")
                                    .small()
                                    .ghost()
                                    .selected(current_ext.as_deref() == Some("pdf"))
                                    .label("pdf")
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.set_ext_filter(Some("pdf".to_string()), cx);
                                    })),
                            ),
                    ),
            )
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
            // 防止 window 参数未使用警告 —— `cx.entity().clone()` 已经借用了 cx，
            // 这里其实没用 window，留着以备未来扩展（例如 List::focus）。
            .map(|this| {
                let _ = (window, &page_handle);
                this
            })
    }
}

/// `gpui-component::List` 的 delegate —— 把当前页的 `LibraryEntry` 切片渲染成行。
///
/// 设计要点（参考 `docs/docs/components/list.md`）：
/// - `page_items` 由 `LibraryPage::render` 在每帧 render 前写入；`render_item` 直接
///   从这个 Vec 里取，不重新计算过滤 / 排序 / 分页。
/// - 持有 `Entity<LibraryPage>` handle 以便 row 的删除按钮 → `LibraryPage::prompt_delete`
///   打开 dialog。
/// - **选中态完全交给 `ListItem::selected(...)` + `set_selected_index` 配对管理**：
///   - `set_selected_index(ix)` 是 List 在用户点击 / 键盘上下移动时回调的方法，
///     我们存到 `selected_index` 字段 + `cx.notify()` 触发 List 重渲染。
///   - `render_item` 通过 `ListItem::selected(Some(ix) == self.selected_index)` 把选中
///     状态交给 ListItem 内置样式（hover 背景 + selected 背景 + 边框都来自 `list_item.rs`
///     paint 逻辑，不要自己叠 h_flex + .hover + .border_b_1）。
/// - **不要**在 row 上叠自定义 hover / selected —— ListItem 已经提供
///   `cx.theme().list_hover` / `list_active` / `list_active_border` 三套样式。
pub struct LibraryDelegate {
    page: Entity<LibraryPage>,
    /// 当前页要展示的条目，每条带"全局序号"（在完整 filtered 列表里的 0-based 位置）。
    /// 由 `LibraryPage::render` 在每帧 render 前写入；`render_item` 直接读取。
    /// 跨分页连续：page 0 → 0..29，page 1 → 30..59，etc. —— 显示时 +1 变 1-based 给人看。
    page_items: Vec<(usize, LibraryEntry)>,
    /// 当前选中项。`None` = 未选中。`set_selected_index` 写入，
    /// `render_item` 读出来给 `ListItem::selected(...)` 用。
    selected_index: Option<IndexPath>,
}

impl LibraryDelegate {
    fn new(page: Entity<LibraryPage>) -> Self {
        Self {
            page,
            page_items: Vec::new(),
            selected_index: None,
        }
    }
}

impl ListDelegate for LibraryDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.page_items.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let (global_index, entry) = self.page_items.get(ix.row)?.clone();
        Some(
            // `ListItem::new` 接受 `impl Into<ElementId>` —— 用 `IndexPath` 直接做 id，
            // 跟 docs `list.md` 里所有示例一致（`ListItem::new(ix)`）。
            ListItem::new(ix)
                // 选中态交给 ListItem：它内部根据 `self.selected` 决定画
                // `cx.theme().list_active` 背景 + `list_active_border` 边框。
                .selected(Some(ix) == self.selected_index)
                // 圆角：参考 `crates/story/src/stories/list_story.rs:88-93` 的
                // CompanyListItem。ListItem 内部选中边框是 absolute 定位（覆盖整个
                // ListItem 矩形），加 `.rounded(...)` 后通过 `overflow: hidden` 把
                // 选中边框裁成圆角，否则选中样式是方角。
                .rounded(cx.theme().radius)
                // 行间距：gpui-component 0.5.1 的 `v_virtual_list` 内部**不支持** row gap
                // （源码 `list.rs` paint 处注释："would not able to have gap_y,
                // because the section header, footer is always have rendered as a
                // empty child item"）。替代方案：每个 ListItem 自己 `.mb(px(4.))`，
                // flex 自然按 margin 撑开。最后一行多 4px 底 margin 跟 List 的
                // `py(px(4.))` 重叠（外间距相加），视觉上等价于统一 4px，不影响。
                .mb(px(4.))
                // 用 global_index 而不是 ix.row 作为 row 内部 id 的序号后缀 —— 跨页
                // 连续（page 0 的最后一行 id=(lib-open, 29) 和 page 1 的第一行
                // id=(lib-open, 30) 不会撞）。render_row 内部再用 global_index + 1
                // 渲染给人看的 1-based 序号列。
                .child(render_row(global_index, &entry, &self.page, &mut *cx)),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected_index = ix;
        // 必须 notify —— List 才会调 `render_item` 重绘新选中的 ListItem 的高亮。
        // 文档里 MyListDelegate 的 set_selected_index 也调了 cx.notify()。
        cx.notify();
    }
}

/// 渲染一行 entry（5 列：文件名 / 格式 / 大小 / 修改时间 / 3 动作）。
///
/// h_flex 5 个固定宽度的子节点，第 4 列 `flex_1` 占满剩余。
/// 删除按钮走 `LibraryPage::prompt_delete`（page 通过 Entity handle 转发）。
fn render_row(
    index: usize,
    entry: &LibraryEntry,
    page: &Entity<LibraryPage>,
    cx: &mut App,
) -> impl IntoElement {
    let path_open = entry.path.clone();
    let path_reveal = entry.path.clone();
    let path_del = entry.path.clone();
    let page_for_del = page.clone();

    // 书名去掉扩展名 —— 扩展名在后面用 tag 显示，避免 "三体.epub" 这种冗余。
    // `strip_suffix(".epub")` 拿不到时（理论上 scan_library_dir 一定 set ext，但兜底）
    // 回退原 file_name，不显示 tag。
    let stem = entry
        .file_name
        .strip_suffix(&format!(".{}", entry.ext))
        .unwrap_or(&entry.file_name)
        .to_string();
    let stem_display = truncate(&stem, 30);
    let ext_upper = entry.ext.to_uppercase();
    let mod_time = format_unix_secs(entry.modified_unix_secs);

    h_flex()
        // 不要 `.id(...)`：外层 `ListItem::new(ix)` 已经给了 id，自己再加会和 List 的
        // 虚拟滚动 hit-test 冲突。
        // 不要 `.hover(|this| this.bg(list_hover))` / `.border_b_1()`：ListItem 的 paint
        // 逻辑已经根据 `selected` / hover 状态画 `list_hover` / `list_active` /
        // `list_active_border` 三套样式（见 `list_item.rs` paint body）。
        .px_2()
        .py_2()
        .gap_2()
        .rounded(cx.theme().radius)
        .items_center()
        // ---- 序号列：跨分页连续（global_index 是 0-based 全局位置，+1 给用户看）----
        // 宽度 48px 装 "#100" / "#999" 这种 4 字符号（"#xxx"）。
        // 右对齐 + muted 颜色，跟"大小 / 时间"列的视觉权重一致。
        .child(
            div()
                .w(px(48.0))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("#{}", index + 1)),
        )
        // ---- 书名 + 类型 tag ----
        // 书名去掉 `.epub`/`.txt` 等扩展名（用户已经在 tag 看到类型了）；tag 紧贴
        // 书名右侧，显示大写扩展名（EPUB / TXT / ZIP / HTML / PDF）。
        //
        // 用 gpui-component `Tag::secondary().small()` —— 主题色 secondary bg + 圆角 +
        // border，跟周围元素视觉权重区分；小号 (px_1p5 py_0p5) 不抢书名焦点。
        //
        // **布局**：整个 h_flex 列 `flex_1()` 占满 row 减去其他固定列（序号 48px /
        // 大小 80px / 时间 140px / 操作 200px）后的**全部剩余宽度**。短书名时列本身
        // 撑到右边，tag 不会飘到中间或靠左错位。
        //   - 内层 book div `flex_1()` 占满父列（380→可变）**减去 tag 后**的剩余宽度，
        //     不管书名多短，book 部分本身都撑到右边。
        //   - `min_w(0)` 让 flex 子项可以收缩到内容以下（默认 min-width = auto → 子项
        //     内容比容器宽时 flex 不收缩），配合 `overflow_x_hidden` + `text_ellipsis`
        //     才能正确触发文本省略号截断。
        //   - `whitespace_nowrap()` 强制单行（默认 flex 容器会让文本 wrap 撑高）。
        .child(
            h_flex()
                .flex_1()
                .min_w(px(0.))
                .items_center()
                .gap_1()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .overflow_x_hidden()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(
                            div()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(stem_display),
                        )
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .child(Tag::secondary().small().child(ext_upper))
                                .child(
                                    Tag::secondary()
                                        .small()
                                        .child(format_size(entry.size_bytes)),
                                ),
                        ),
                ),
        )
        .child(
            // 时间列：固定 140px 装 "2026-01-15 12:34" 这种 16 字符。
            // 不再用 flex_1 —— 否则会跟书名列抢剩余宽度（两个 flex_1 会平分）。
            div()
                .w(px(140.0))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(mod_time),
        )
        .child(
            // 操作列：固定 200px 装 3 个 xsmall 按钮 + gap，跟其他列一致。
            h_flex()
                .w(px(240.0))
                .gap_1()
                .justify_end()
                .child(
                    Button::new(("lib-open", index as u64))
                        .small()
                        .outline()
                        .icon(Icon::new(IconName::ExternalLink))
                        .label(ts("Library.action_open"))
                        // 「打开」按钮：用系统默认程序打开文件本身（如 .epub → ebook reader）。
                        // 对应 `util/system.rs::open_path(path)`。
                        .on_click(move |_, _window, _cx| {
                            if let Err(e) = open_path(&path_open) {
                                tracing::warn!("open_path failed: {e:#}");
                            }
                        }),
                )
                .child(
                    Button::new(("lib-reveal", index as u64))
                        .small()
                        .outline()
                        .icon(Icon::new(IconName::Folder))
                        .label(ts("Library.action_reveal"))
                        // 「位置」按钮：OS 文件管理器打开文件**所在目录**，
                        // 高亮显示该文件（Windows `explorer /select,<file>` /
                        // macOS Finder reveal / Linux fallback 到打开父目录 —— 见 util/system.rs）。
                        .on_click(move |_, _window, _cx| {
                            if let Err(e) = reveal_in_folder(&path_reveal) {
                                tracing::warn!("reveal_in_folder failed: {e:#}");
                            }
                        }),
                )
                .child(
                    Button::new(("lib-del", index as u64))
                        .small()
                        .danger()
                        .icon(Icon::new(IconName::Delete))
                        .label(ts("Library.action_delete"))
                        .on_click(move |_, window, cx| {
                            // `Button::on_click` 的 handler 是 `Fn`（点击可多次触发），所以
                            // 外层 closure 不能 move `path_del` 进内层 closure。每次点击
                            // 重新 clone 一份给内层 closure（prompt_delete 内部需要 owned）。
                            let path_for_click = path_del.clone();
                            page_for_del.update(cx, move |p, cx| {
                                p.prompt_delete(path_for_click, window, cx);
                            });
                        }),
                ),
        )
}

/// 简单 unix 秒 → "YYYY-MM-DD HH:MM"。本地时区。
fn format_unix_secs(secs: u64) -> String {
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;
    if secs == 0 {
        return ts("Library.time.unknown").to_string();
    }
    let Ok(dt) = OffsetDateTime::from_unix_timestamp(secs as i64) else {
        return ts("Library.time.invalid").to_string();
    };
    let local =
        dt.to_offset(time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC));
    local
        .format(&Rfc3339)
        .ok()
        .map(|s| s[..16].replace('T', " "))
        .unwrap_or_else(|| ts("Library.time.format_failed").to_string())
}
