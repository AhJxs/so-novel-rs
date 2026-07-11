//! Library 页面：本地书库（下载目录里的电子书文件）。
//!
//! 行为：
//! - 进入页面时若 `library.scanned_dir` 为空 / 不匹配 `config.download.download_path` → 自动扫一次。
//! - 工具栏：文件名过滤输入 + 文件类型按钮组（不在 State 里实现 —— 切语言即时更新）。
//! - 列表：gpui-component `List`（虚拟滚动）+ `LibraryDelegate`，每页 30 条（5 列：文件名 /
//!   格式 / 大小 / 修改时间 / 3 动作）。
//! - 分页页脚自写（gpui-component 0.5.1 没 Pagination 组件），≤1 页时整段隐藏。
//! - **没有文件 watcher** —— 列表只在「首次进入 / 下载目录变化」时自动扫一次，
//!   其余情况靠 `PageHeader` 右上角「刷新」按钮手动触发。
//! - 删除走 `WindowExt::open_dialog` 二次确认 → `model.delete_library_entry` → `entries_version`
//!   bump 让 `ListCache` 立即失效，UI 实时反映删除结果。

mod delegate;
mod row;
mod toolbar;

use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, ClickEvent, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Window, div, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, WindowExt,
    button::{Button, ButtonVariant},
    dialog::Dialog,
    dialog::DialogButtonProps,
    input::{InputEvent, InputState},
    list::List,
    list::ListState,
    v_flex,
};

use crate::desktop::components::{EmptyState, PageHeader, Pagination, compute_page_window};
use crate::desktop::model::{AppModel, LibraryEntry};
use crate::i18n::{ts, ts_cached, ts_fmt};

use self::delegate::LibraryDelegate;

/// Library 页面 entity。
pub struct LibraryPage {
    model: Entity<AppModel>,

    /// struct 字段持有（InputState / `ListState`）—— owner 持有避免 click / focus 丢失。
    filter_input: Entity<InputState>,
    list_state: Entity<ListState<LibraryDelegate>>,

    /// UI-only，每次路径或过滤变化时重置为 0。
    current_page: usize,
}

impl LibraryPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::i18n::ts("Library.filter_placeholder"))
        });
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

        // 文件类型过滤在 render 里用 button group 实现（不持 State —— 切语言即时更新）。
        // 扩展名（epub / txt / zip / html / pdf）不译，是技术名词。

        // delegate 持有 `Entity<LibraryPage>`（不是 WeakEntity）—— Entity 永驻
        // (`RootView` 持有)，`render_item` 需要调 `prompt_delete` 拿 `Context<LibraryPage>`。
        let page_handle = cx.entity();
        let delegate = LibraryDelegate::new(page_handle);
        let list_state = cx.new(|cx| ListState::new(delegate, window, cx));

        Self {
            model,
            filter_input,
            list_state,
            current_page: 0,
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

    /// 首次进入 / 下载目录变化时自动扫一次。
    /// `过滤变化（filter_text` / `filter_ext）不走这里` —— 不改变路径。
    fn maybe_auto_scan(&mut self, cx: &mut Context<Self>) {
        let download_path =
            std::path::PathBuf::from(self.model.read(cx).config.download.download_path.clone());
        let already_scanned = self.model.read(cx).library.scanned_dir.clone();
        let need_scan = already_scanned.as_ref().is_none_or(|p| p != &download_path);
        if need_scan {
            self.model.update(cx, |m, _cx| m.refresh_library_async());
            self.current_page = 0;
        }
    }

    /// `PageHeader` 「刷新」按钮 —— 重扫下载目录。`scan_in_flight` 期间点多次会被
    /// `refresh_library_async` 内部的 flag 拦截，重复触发零成本。
    fn manual_refresh(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |m, _cx| m.refresh_library_async());
        self.current_page = 0;
        cx.notify();
    }

    /// 点"删除"按钮 → 弹 Dialog 二次确认。
    pub(super) fn prompt_delete(&self, path: PathBuf, window: &mut Window, cx: &mut App) {
        let model = self.model.clone();
        let model_id = model.entity_id();
        // 文件名兜底：空时用 i18n fallback 替。
        let raw_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let file_name: String = if raw_name.is_empty() {
            ts("Library.fallback_unknown_filename").to_string()
        } else {
            raw_name.to_string()
        };

        window.open_dialog(cx, move |dialog: Dialog, _window, _cx| {
            // dialog builder 是 Fn（被 open_dialog 复用，每次点击都重调）；
            // on_ok 也必须 Fn —— 全部 clone 捕获，避开 FnOnce。
            let model_for_ok = model.clone();
            let path_for_ok = path.clone();
            let model_id_for_ok = model_id;

            dialog
                .title(ts("Library.delete_dialog.title"))
                // 占位符必须走 ts_fmt —— 直接 format! 拼字符串会在切语言时让
                // 占位符翻译也跟着拼，顺序错乱。
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
                    true
                })
        });
    }
}

impl Render for LibraryPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.maybe_auto_scan(cx);

        // placeholder 在 `new()` 一次性设好；language setter 切语言走"重启进程"
        // 路径，新进程重建 InputState 时自然拿到新 locale，无需 render 差量刷新。

        // 1) 通过 `model.update` 拿 `&mut AppModel` —— list_cache 写入需要可
        //    变借用，而 `model.read(cx)` 只能拿 `&AppModel`。闭包内：(a) 算
        //    filter signature；(b) 查 list_cache 命中则 clone Arc、未命中则
        //    走 filtered_entries 后写回；(c) 取出展示数据。闭包返回
        //    `(Arc<Vec<LibraryEntry>>, usize, Option<String>, String, Option<String>, bool)`。
        let (entries_arc, total, scan_err, download_path, current_ext, scan_in_flight) =
            self.model.update(cx, |model, _cx| {
                // 命中 cache = Arc::clone（只增引用计数，零 alloc）；未命中 = 走
                // 完整 filter+sort 然后写回。cache key 包含 (entries_version,
                // filter_sig)，filter_text / filter_ext 变 → signature 变 → 失效。
                let filter_sig = crate::desktop::model::filter_signature(&[
                    model.library.filter_text.as_str(),
                    model.library.filter_ext.as_deref().unwrap_or(""),
                ]);
                let key = crate::desktop::model::ListCacheKey {
                    page: crate::desktop::model::PageKind::Library,
                    data_version: model.library.entries_version,
                    filter_sig,
                    page_index: 0, // 缓存"全表过滤+排序"结果；分页在 Render 末尾 slice
                    elem_type: std::any::TypeId::of::<LibraryEntry>(),
                };
                let entries_arc = if let Some(arc) = model.list_cache.get::<LibraryEntry>(key) {
                    arc
                } else {
                    // miss：跑一遍 filter+sort，写回。
                    let mut v: Vec<LibraryEntry> = model
                        .library
                        .entries
                        .iter()
                        .filter(|e| {
                            if let Some(ext) = &model.library.filter_ext
                                && &e.ext != ext
                            {
                                return false;
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
                    model.list_cache.insert(key, v)
                };
                let total = entries_arc.len();
                let scan_err = model.library.last_error.clone();
                let download_path = model.config.download.download_path.clone();
                let current_ext = model.library.filter_ext.clone();
                // 读 scan_in_flight —— 刷新按钮的 loading 状态用。
                // drain_loop 100ms tick 内排空 scan channel 时会清零 + notify AppModel，
                // LibraryPage 是观察者会自动 re-render，loading 状态随之收敛。
                let scan_in_flight = model.library.scan_in_flight;
                (
                    entries_arc,
                    total,
                    scan_err,
                    download_path,
                    current_ext,
                    scan_in_flight,
                )
            });

        let w = compute_page_window(total, &mut self.current_page);
        // 每条带"全局序号" = 在完整 filtered 列表里的位置（0-based，跨分页连续）。
        // delegate 只看 `(global_ix, entry)`，不依赖 current_page / PAGE_SIZE。
        let page_items: Vec<(usize, LibraryEntry)> = if total == 0 {
            Vec::new()
        } else {
            entries_arc[w.start..w.end]
                .iter()
                .enumerate()
                .map(|(local_ix, e)| (w.start + local_ix, e.clone()))
                .collect()
        };

        self.list_state.update(cx, |state, _cx| {
            state.delegate_mut().page_items = page_items;
        });

        v_flex()
            .size_full()
            .p_6()
            .gap_3()
            .child(
                PageHeader::new(ts("Library.page_title"))
                    .subtitle(format!(
                        "{}: {}",
                        ts("Library.download_path_label"),
                        std::path::Path::new(&download_path).display()
                    ))
                    .action(
                        Button::new("library-refresh")
                            .icon(Icon::new(IconName::Redo))
                            .label(ts_cached("Library.action_refresh"))
                            // scan_in_flight=true 时：禁用 + 显示 spinner，
                            // 视觉上告诉用户"正在扫，不要再点"。
                            // manual_refresh 内部也会被 scan_in_flight 拦截，
                            // 禁用只是双保险（鼠标 / 键盘都可触发 button click）。
                            .loading(scan_in_flight)
                            .disabled(scan_in_flight)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.manual_refresh(cx);
                            })),
                    ),
            )
            .child(toolbar::render(
                &self.filter_input,
                current_ext.as_deref(),
                cx,
            ))
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
                // 水平 12px 留出 ListItem 右侧选中边框不被滚动条遮住的位置；
                // 垂直 4px 给行间呼吸空间但不让间距喧宾夺主。
                // 参考 crates/story/src/stories/list_story.rs:594-602。
                div()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_md()
                    .child(List::new(&self.list_state).p(px(12.)).size_full())
                    .into_any_element()
            })
            .when(total > 0, |this| {
                // 空态不挂分页（避免"第 1 页 / 共 0 条"无意义提示）。
                this.child(Pagination::new(
                    self.current_page,
                    w.page_count,
                    cx.listener(|this, &new_page, _window, cx| {
                        this.current_page = new_page;
                        cx.notify();
                    }),
                ))
            })
    }
}
