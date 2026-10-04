//! Library 页面：本地书库（下载目录里的电子书文件）。
//!
//! - 进入页面时 `library.scanned_dir` 与 `config.download.download_path` 不一致 → 自动扫一次。
//! - 工具栏：文件名过滤输入 + 文件类型按钮组（不持 State —— 切语言即时更新）。
//! - 列表：`List`（虚拟滚动）+ `LibraryDelegate`，每页 30 条；分页页脚走 `components::Pagination`，
//!   其可见性由组件自判（不足一页渲染 `Empty`）。
//! - **没有文件 watcher**：只在「首次进入 / 下载目录变化」自动扫，其余靠「刷新」按钮。
//! - 删除走 `WindowExt::open_dialog` 二次确认 → `model.delete_library_entry` → bump
//!   `entries_version` 让 `ListCache` 立即失效。

mod delegate;
mod row;
mod toolbar;

use std::path::PathBuf;

use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, WindowExt,
    button::{Button, ButtonVariant},
    dialog::AlertDialog,
    input::{InputEvent, InputState},
    list::List,
    list::ListState,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext, ClickEvent, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Window, div, px,
};

use crate::desktop::components::{EmptyState, PageHeader, Pagination, compute_page_window};
use crate::desktop::model::{AppModel, LibraryEntry};
use rust_i18n::t;

use self::delegate::LibraryDelegate;

/// Library 页面 entity。
pub struct LibraryPage {
    model: Entity<AppModel>,

    /// `InputState` / `ListState` 由 owner 持有，否则 click / focus 丢失。
    filter_input: Entity<InputState>,
    list_state: Entity<ListState<LibraryDelegate>>,

    /// UI-only，每次路径或过滤变化时重置为 0。
    current_page: usize,
}

impl LibraryPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("Library.filter_placeholder")));
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

        // 文件类型过滤用 render 里的 button group 实现（不持 State → 切语言即时更新）；
        // 扩展名（epub / txt / zip / html / pdf / md）不译，是技术名词。

        // delegate 持有强引用 `Entity<LibraryPage>`（不是 WeakEntity）：Entity 永驻，
        // 且 `render_item` 里 `prompt_delete` 需要 `Context<LibraryPage>`。
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

    /// 设置文件类型过滤（None = "全部"）+ 跳回第 1 页（filter 变化后旧页码可能越界）。
    fn set_ext_filter(&mut self, new_ext: Option<String>, cx: &mut Context<Self>) {
        self.model.update(cx, |m, _cx| {
            m.library.filter_ext = new_ext;
        });
        self.current_page = 0;
        cx.notify();
    }

    /// 首次进入 / 下载目录变化时自动扫一次（filter 变化不走这里，路径没变）。
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

    /// `PageHeader`「刷新」按钮 —— 重扫下载目录（`scan_in_flight` 期间的重复点击会被
    /// `refresh_library_async` 内部 flag 拦掉）。
    fn manual_refresh(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |m, _cx| m.refresh_library_async());
        self.current_page = 0;
        cx.notify();
    }

    /// 点"删除"按钮 → 弹 Dialog 二次确认。
    pub(super) fn prompt_delete(&self, path: PathBuf, window: &mut Window, cx: &mut App) {
        let model = self.model.clone();
        let model_id = model.entity_id();
        // 文件名兜底：空时用 i18n 文案。
        let raw_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let file_name: String = if raw_name.is_empty() {
            t!("Library.fallback_unknown_filename").to_string()
        } else {
            raw_name.to_string()
        };

        window.open_alert_dialog(cx, move |alert: AlertDialog, _window, _cx| {
            // alert builder 与 on_ok 都是 Fn —— 捕获的变量全部 clone，避开 FnOnce。
            let model_for_ok = model.clone();
            let path_for_ok = path.clone();
            let model_id_for_ok = model_id;

            alert
                .title(t!("Library.delete_dialog.title"))
                // 占位符必须走 t! —— 直接 format! 会把占位符也拼进被翻译的字符串。
                .description(t!("Library.delete_dialog.message", file_name = &file_name))
                // 按钮文案 / variant 直接用 AlertDialog 的单项 builder（`ok_text` /
                // `cancel_text` / `ok_variant`）。调用顺序无关：`confirm()` 不会覆盖已设文案。
                .ok_text(t!("Library.delete_dialog.confirm_button"))
                .cancel_text(t!("Library.delete_dialog.cancel_button"))
                .ok_variant(ButtonVariant::Danger)
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

        // placeholder 在 `new()` 一次设好；切语言走重启进程，无需 render 差量刷新。

        // 用 `model.update` 拿 `&mut AppModel`（list_cache 写入需要可变借用）：算 filter
        // signature → 命中缓存则 clone Arc、否则过滤+排序后写回 → 取出展示数据。
        let (entries_arc, total, scan_err, download_path, current_ext, scan_in_flight) =
            self.model.update(cx, |model, _cx| {
                // cache key 含 (entries_version, filter_sig)：filter_text / filter_ext 一改
                // 就失效重算。
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
                // scan_in_flight 给刷新按钮的 loading 用；drain loop 排空 scan channel 时
                // 会清零 + notify，本页作为观察者自动 re-render。
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
        // 每条带全局序号（完整 filtered 列表里的 0-based 位置，跨分页连续）。
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
                PageHeader::new(t!("Library.page_title"))
                    .subtitle(format!(
                        "{}: {}",
                        t!("Library.download_path_label"),
                        std::path::Path::new(&download_path).display()
                    ))
                    .action(
                        Button::new("library-refresh")
                            .icon(Icon::new(IconName::Redo))
                            .label(t!("Library.action_refresh"))
                            // 扫描中：禁用 + spinner，manual_refresh 内部也会拦一次。
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
                        .child(format!("{}: {err}", t!("Library.scan_failed"))),
                )
            })
            .child(if total == 0 {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        EmptyState::new(IconName::BookOpen, t!("Library.empty_title"))
                            .subtitle(t!("Library.empty_subtitle")),
                    )
                    .into_any_element()
            } else {
                // 12px padding 让 ListItem 的选中边框不被滚动条遮住。
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
            // 页脚可见性由 `Pagination` 自己判，caller 不用 `when`。
            .child(Pagination::new(
                self.current_page,
                w.page_count,
                cx.listener(|this, &new_page, _window, cx| {
                    this.current_page = new_page;
                    cx.notify();
                }),
            ))
    }
}
