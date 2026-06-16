//! Library 页面：本地书库（下载目录里的电子书文件）。
//!
//! 行为（与旧 `crate::ui::pages::library::show` 一致，UI 完全重写）：
//! - 进入页面时若 `library.scanned_dir` 为空 / 不匹配 `config.download_path` → 自动扫一次。
//! - 顶部 `PageHeader` + 工具栏：刷新 / 文件名过滤输入 / 扩展名过滤 ButtonGroup。
//! - 列表：按修改时间倒序，文件名 / 格式 / 大小 / 修改时间；每行 3 个动作（打开 / 显示位置 / 删除）。
//! - 删除走 `WindowExt::open_dialog` 二次确认（点删除按钮 → 打开 dialog → on_ok 调 `model.delete_library_entry`）。
//! - 空态用 `EmptyState`（图标 + "本地书库为空" + 副标题）。

use std::path::PathBuf;

use gpui::{
    div, prelude::FluentBuilder as _, px, App, AppContext, ClickEvent, Context, Entity,
    InteractiveElement, IntoElement, ParentElement, Render, ScrollHandle, StatefulInteractiveElement,
    Styled, Window,
};
use gpui_component::{
    button::{Button, ButtonGroup, ButtonVariant, ButtonVariants},
    dialog::{Dialog, DialogButtonProps},
    h_flex, v_flex,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    ActiveTheme as _, Icon, IconName, Selectable, Sizable, WindowExt,
};

use crate::app::{AppModel, LibraryEntry};
use crate::gpui_app::components::{format_size, truncate, EmptyState, PageHeader};
use crate::util::system::{open_path, reveal_in_folder};

/// Library 页面 entity。
pub struct LibraryPage {
    // model 在 new() 时持有；后续 stage 通过 cx.listener 用到。
    // 当前 render 只读 model.library 与 model.config，所以暂时 dead_code。
    // 删 dead_code 后会让 entity 创建不通过所有权校验。保留 + 标注。
    #[allow(dead_code)]
    model: Entity<AppModel>,
    filter_input: Entity<InputState>,
    scroll: ScrollHandle,
}

impl LibraryPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter_input = cx.new(|cx| InputState::new(window, cx).placeholder("按文件名过滤…"));

        // 同步 InputState → model.library.filter_text
        cx.subscribe_in(
            &filter_input,
            window,
            |this, _state, event, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    let v = this.filter_input.read(cx).value();
                    this.model.update(cx, |m, _cx| {
                        m.library.filter_text = v.to_string();
                    });
                    cx.notify();
                }
            },
        )
        .detach();

        Self {
            model,
            filter_input,
            scroll: ScrollHandle::new(),
        }
    }

    /// 首次进入 / 下载目录变化时自动扫一次。
    fn maybe_auto_scan(&mut self, cx: &mut Context<Self>) {
        let (download_path, already_scanned) = {
            let model = self.model.read(cx);
            (
                std::path::PathBuf::from(model.config.download_path.clone()),
                model.library.scanned_dir.clone(),
            )
        };
        let need_scan = match &already_scanned {
            None => true,
            Some(p) => p != &download_path,
        };
        if need_scan {
            self.model.update(cx, |m, _cx| m.refresh_library());
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |m, _cx| m.refresh_library());
        cx.notify();
    }

    fn set_ext_filter(&mut self, ext: Option<String>, cx: &mut Context<Self>) {
        self.model.update(cx, |m, _cx| m.library.filter_ext = ext);
        cx.notify();
    }

    /// 点"删除"按钮 → 弹 Dialog 二次确认。
    fn prompt_delete(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.clone();
        let model_id = model.entity_id();
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("(未知文件名)")
            .to_string();

        window.open_dialog(cx, move |dialog: Dialog, _window, _cx| {
            // 外层 dialog builder 是 Fn — 每次被 open_dialog 复用，闭包要能多次调用。
            // 因此 inner on_ok 也要能 Fn；model / path 都通过引用捕获，避开 FnOnce。
            let model_for_ok = model.clone();
            let path_for_ok = path.clone();
            let model_id_for_ok = model_id;

            dialog
                .title("确认删除")
                .child(div().child(format!(
                    "确定要删除 \"{file_name}\" 吗？此操作无法撤销。"
                )))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("删除")
                        .cancel_text("取消")
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
        v.sort_by(|a, b| b.modified_unix_secs.cmp(&a.modified_unix_secs));
        v
    }
}

impl Render for LibraryPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.maybe_auto_scan(cx);

        let model = self.model.read(cx);
        let entries = Self::filtered_entries(&model);
        let scan_err = model.library.last_error.clone();
        let ext_filter = model.library.filter_ext.clone();
        let download_path = model.config.download_path.clone();
        let _ = model;

        let has_entries = !entries.is_empty();

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(
                PageHeader::new("本地书库")
                    .subtitle(format!(
                        "下载目录: {}",
                        std::path::Path::new(&download_path).display()
                    ))
                    .action(
                        Button::new("refresh")
                            .icon(Icon::new(IconName::Loader))
                            .label("刷新")
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.refresh(cx);
                            })),
                    ),
            )
            // 工具栏
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        Input::new(&self.filter_input)
                            .w(px(280.0))
                            .prefix(
                                Icon::new(IconName::Search)
                                    .small()
                                    .text_color(cx.theme().muted_foreground),
                            ),
                    )
                    .child(ext_filter_group(&ext_filter, cx)),
            )
            // 错误提示
            .when_some(scan_err, |this, err| {
                this.child(
                    div()
                        .p_3()
                        .rounded_md()
                        .bg(cx.theme().danger)
                        .text_color(cx.theme().danger_foreground)
                        .child(format!("扫描失败: {err}")),
                )
            })
            // 列表 / 空态
            .child(if has_entries {
                render_list(entries, cx.entity(), cx, self.scroll.clone()).into_any_element()
            } else {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        EmptyState::new(IconName::BookOpen, "本地书库为空")
                            .subtitle("下载一本书后，它会自动出现在这里。"),
                    )
                    .into_any_element()
            })
    }
}

fn ext_filter_group(
    current: &Option<String>,
    cx: &mut Context<LibraryPage>,
) -> ButtonGroup {
    ButtonGroup::new("ext-filter")
        .child(
            Button::new("all")
                .label("全部")
                .selected(current.is_none())
                .on_click(cx.listener(|this, _, _window, cx| {
                    this.set_ext_filter(None, cx);
                })),
        )
        .child(ext_button("epub", "epub", current, cx))
        .child(ext_button("txt", "txt", current, cx))
        .child(ext_button("zip", "zip", current, cx))
        .child(ext_button("html", "html", current, cx))
        .child(ext_button("pdf", "pdf", current, cx))
}

fn ext_button(
    id: &'static str,
    ext: &'static str,
    current: &Option<String>,
    cx: &mut Context<LibraryPage>,
) -> Button {
    let is_selected = current.as_deref() == Some(ext);
    // 用 &str 形式 id（ElementId: From<&str>）
    Button::new(id)
        .label(ext)
        .selected(is_selected)
        .on_click(cx.listener(move |this, _, _window, cx| {
            this.set_ext_filter(Some(ext.to_string()), cx);
        }))
}

/// 渲染 entry 列表（每行：文件名 / 格式 / 大小 / 修改时间 / 3 个动作按钮）。
fn render_list(
    entries: Vec<LibraryEntry>,
    page: Entity<LibraryPage>,
    cx: &mut App,
    scroll: ScrollHandle,
) -> impl IntoElement {
    v_flex()
        .flex_1()
        .size_full()
        .overflow_hidden()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .child(
            // 表头
            h_flex()
                .px_3()
                .py_2()
                .bg(cx.theme().sidebar)
                .border_b_1()
                .border_color(cx.theme().border)
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(div().w(px(380.0)).child("文件名"))
                .child(div().w(px(60.0)).child("格式"))
                .child(div().w(px(100.0)).child("大小"))
                .child(div().flex_1().child("修改时间"))
                .child(div().w(px(180.0)).justify_end().child("操作")),
        )
        .child(
            div()
                .flex_1()
                .id("library-scroll")
                .track_scroll(&scroll)
                .overflow_y_scrollbar()
                .children(
                    entries
                        .iter()
                        .enumerate()
                        .map(|(idx, e)| render_row(idx, e, &page, cx)),
                ),
        )
}

/// 渲染一行 entry。
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
    let _page_id = page.entity_id(); // 暂留：未来可加 row id 用

    let file_name_display = truncate(&entry.file_name, 50);
    let mod_time = format_unix_secs(entry.modified_unix_secs);

    h_flex()
        .id(("lib-row", index as u64))
        .px_3()
        .py_2()
        .gap_2()
        .items_center()
        .border_b_1()
        .border_color(cx.theme().border)
        .hover(|this| this.bg(cx.theme().list_hover))
        .child(
            div()
                .w(px(380.0))
                .text_sm()
                .text_color(cx.theme().foreground)
                .child(file_name_display),
        )
        .child(
            div()
                .w(px(60.0))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(entry.ext.to_uppercase()),
        )
        .child(
            div()
                .w(px(100.0))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format_size(entry.size_bytes)),
        )
        .child(
            div()
                .flex_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(mod_time),
        )
        .child(
            h_flex()
                .w(px(180.0))
                .gap_1()
                .justify_end()
                .child(
                    Button::new(("lib-open", index as u64))
                        .xsmall()
                        .ghost()
                        .icon(Icon::new(IconName::ExternalLink))
                        .label("打开")
                        .on_click(move |_, _window, _cx| {
                            if let Err(e) = open_path(&path_open) {
                                tracing::warn!("open_path failed: {e:#}");
                            }
                        }),
                )
                .child(
                    Button::new(("lib-reveal", index as u64))
                        .xsmall()
                        .ghost()
                        .icon(Icon::new(IconName::Folder))
                        .label("位置")
                        .on_click(move |_, _window, _cx| {
                            if let Err(e) = reveal_in_folder(&path_reveal) {
                                tracing::warn!("reveal_in_folder failed: {e:#}");
                            }
                        }),
                )
                .child(
                    Button::new(("lib-del", index as u64))
                        .xsmall()
                        .ghost()
                        .danger()
                        .icon(Icon::new(IconName::Delete))
                        .label("删除")
                        .on_click(move |_, window, cx| {
                            page_for_del.update(cx, |p, cx| {
                                p.prompt_delete(path_del.clone(), window, cx);
                            });
                        }),
                ),
        )
}

/// 简单 unix 秒 → "YYYY-MM-DD HH:MM"。本地时区。
fn format_unix_secs(secs: u64) -> String {
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;
    if secs == 0 {
        return "(未知)".to_string();
    }
    let Ok(dt) = OffsetDateTime::from_unix_timestamp(secs as i64) else {
        return "(无效时间)".to_string();
    };
    let local = dt.to_offset(
        time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC),
    );
    local
        .format(&Rfc3339)
        .ok()
        .map(|s| s[..16].replace('T', " "))
        .unwrap_or_else(|| "(格式化失败)".to_string())
}
