//! 书架弹窗：书籍信息、移出 / 删除确认、重新定位。
//!
//! 所有弹窗都从 `LibraryPage` 的方法打开；`window.open_dialog` 的 builder 每帧重跑，
//! 因此状态按帧从 `AppModel` 重读，不往 builder 闭包里放可变状态。

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::AlertDialog;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{App, ClickEvent, Context, Div, ParentElement, Styled, Window, div, px};
use rust_i18n::t;

use super::library_page::LibraryPage;
use crate::desktop::components::format_size;
use crate::utils::formatting::format_local_unix_secs;

impl LibraryPage {
    /// 书籍信息（只读）。
    pub(crate) fn open_info_dialog(
        &self,
        book_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(book) = self.model.read(cx).book_snapshot(book_id) else {
            return;
        };
        let unsupported = !book.format.is_readable();
        let missing = book.missing;
        let page = cx.entity();
        let id = book_id.to_string();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let book = book.clone();
            let unknown = t!("Library.info.unknown").to_string();
            let chapters = book
                .chapter_count
                .map_or_else(|| unknown.clone(), |count| count.to_string());
            dialog
                .title(t!("Library.info.title").to_string())
                .w(px(560.))
                .child(
                    v_flex()
                        .w_full()
                        .gap_1()
                        .text_sm()
                        .child(info_row(
                            t!("Library.info.format").to_string(),
                            book.format.label().to_string(),
                            cx,
                        ))
                        .child(info_row(
                            t!("Library.info.path").to_string(),
                            book.path.clone(),
                            cx,
                        ))
                        .child(info_row(
                            t!("Library.info.size").to_string(),
                            format_size(book.size_bytes),
                            cx,
                        ))
                        .child(info_row(
                            t!("Library.info.chapters").to_string(),
                            chapters,
                            cx,
                        ))
                        .child(info_row(
                            t!("Library.info.added").to_string(),
                            format_local_unix_secs(
                                book.added_unix,
                                "Library.info.unknown",
                                "Library.info.unknown",
                                "Library.info.unknown",
                            ),
                            cx,
                        ))
                        .child(info_row(
                            t!("Library.info.progress").to_string(),
                            format!("{} · {:.0}%", t!(book.status_key()), book.progress.percent),
                            cx,
                        ))
                        .child(info_row(
                            t!("Library.info.bookmarks").to_string(),
                            book.bookmarks.len().to_string(),
                            cx,
                        ))
                        .when(unsupported, |this| {
                            this.child(
                                div()
                                    .mt_2()
                                    .text_sm()
                                    .text_color(cx.theme().warning)
                                    .child(t!("Library.status.unsupported").to_string()),
                            )
                        }),
                )
                .when(missing, |this| {
                    let page = page.clone();
                    let id = id.clone();
                    this.child(
                        Button::new("book-info-relink")
                            .w_full()
                            .mt_3()
                            .ghost()
                            .label(t!("Library.actions.relink").to_string())
                            .on_click(move |_ev, window, cx| {
                                window.close_dialog(cx);
                                // 这里是强引用 `Entity`，`update` 直接返回 `()`，不能再套 `let _ =`。
                                page.update(cx, |page, cx| page.pick_relink(&id, cx));
                            }),
                    )
                })
        });
    }

    /// 移出书架（二次确认）。
    pub(super) fn prompt_hide(&self, book_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(book) = self.model.read(cx).book_snapshot(book_id) else {
            return;
        };
        let model = self.model.clone();
        let id = book.id;
        let title = book.title;
        window.open_alert_dialog(cx, move |alert: AlertDialog, _window, _cx| {
            let model = model.clone();
            let id = id.clone();
            alert
                .title(t!("Library.hide_dialog.title"))
                .description(t!("Library.hide_dialog.message", title = &title))
                .ok_text(t!("Library.hide_dialog.confirm_button"))
                .cancel_text(t!("Library.dialog.cancel_button"))
                .confirm()
                .on_ok(move |_ev: &ClickEvent, _window, cx| {
                    model.update(cx, |model, _cx| model.hide_book(&id));
                    true
                })
        });
    }

    /// 删除磁盘文件（二次确认，危险操作）。
    pub(super) fn prompt_delete_file(
        &self,
        book_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(book) = self.model.read(cx).book_snapshot(book_id) else {
            return;
        };
        let model = self.model.clone();
        let id = book.id;
        let title = book.title;
        window.open_alert_dialog(cx, move |alert: AlertDialog, _window, _cx| {
            let model = model.clone();
            let id = id.clone();
            alert
                .title(t!("Library.delete_file_dialog.title"))
                .description(t!("Library.delete_file_dialog.message", title = &title))
                .ok_text(t!("Library.delete_file_dialog.confirm_button"))
                .cancel_text(t!("Library.dialog.cancel_button"))
                .ok_variant(ButtonVariant::Danger)
                .confirm()
                .on_ok(move |_ev: &ClickEvent, _window, cx| {
                    model.update(cx, |model, _cx| model.delete_book_file(&id));
                    true
                })
        });
    }

    /// 重新定位：选一个新文件，格式必须与原书一致。
    pub(super) fn pick_relink(&self, book_id: &str, cx: &Context<Self>) {
        let Some(book) = self.model.read(cx).book_snapshot(book_id) else {
            return;
        };
        let format = book.format;
        let model = self.model.clone();
        let page = cx.entity().downgrade();
        let id = book.id;
        cx.spawn(async move |_weak, async_cx| {
            let file = rfd::AsyncFileDialog::new()
                .add_filter(format.label(), &[format.extension()])
                .set_title(t!("Library.relink.dialog_title").as_ref())
                .pick_file()
                .await;
            let Some(file) = file else {
                return;
            };
            let path = file.path().to_path_buf();
            let _ = page.update(async_cx, |_page, cx| {
                model.update(cx, |model, _cx| {
                    if let Err(error) = model.relink_book(&id, &path) {
                        model.push_error(error.message());
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }
}

/// 一行「标签：值」。
///
/// `min_w_0` 不能省：flex 子项的默认最小宽度是 `auto`（这里等于值的最长不可断片段，
/// 比如整条磁盘路径），不置零它就不肯缩到该宽度以下，长路径会溢出弹窗被裁掉。
fn info_row(label: String, value: String, cx: &App) -> Div {
    h_flex()
        .w_full()
        .items_start()
        .gap_2()
        .child(
            div()
                .w(px(88.))
                .flex_none()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(value))
}
