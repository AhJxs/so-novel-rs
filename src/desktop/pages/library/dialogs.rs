//! 书架弹窗：书籍操作、编辑信息、书籍信息、移出 / 删除确认、重新定位。
//!
//! 所有弹窗都从 `LibraryPage` 的方法打开；`window.open_dialog` 的 builder 每帧重跑，
//! 因此状态要么放进 entity（`EditState`），要么按帧从 `AppModel` 重读。

use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::AlertDialog;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, ClickEvent, Context, Div, ObjectFit, ParentElement, Styled,
    StyledImage as _, Window, div, img, px,
};
use rust_i18n::t;

use super::library_page::LibraryPage;
use crate::desktop::components::format_size;
use crate::library::write_cover;
use crate::utils::formatting::format_local_unix_secs;
use crate::utils::system;

/// 编辑信息弹窗的临时状态：保存前不改动 store。
pub(super) struct EditState {
    /// 正在编辑的书。
    book_id: String,
    /// store 里当前的封面缓存文件名。
    cover_file: Option<String>,
    /// 新选的封面原图（保存时才写进缓存目录）。
    picked_cover: Option<PathBuf>,
    /// 用户点了「移除封面」。
    remove_cover: bool,
}

impl LibraryPage {
    /// 书籍操作弹窗（「…」按钮）。
    pub(super) fn open_actions_dialog(
        &self,
        book_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(book) = self.model.read(cx).book_snapshot(book_id) else {
            return;
        };
        let page = cx.entity().downgrade();
        let id = book.id.clone();
        let title = book.title.clone();
        let path = PathBuf::from(&book.path);
        let readable = book.format.is_readable();
        let missing = book.missing;
        let finished = book.progress.finished;

        window.open_dialog(cx, move |dialog, _window, _cx| {
            let page = page.clone();
            let id = id.clone();
            let path = path.clone();

            // 「继续阅读」/「从头开始」/「重新定位」共用同一个入口。
            let continue_label = if missing {
                t!("Library.actions.relink")
            } else {
                t!("Library.actions.continue")
            }
            .to_string();
            let continue_button = {
                let page = page.clone();
                let id = id.clone();
                Button::new("book-action-continue")
                    .w_full()
                    .ghost()
                    .label(continue_label)
                    .disabled(!missing && !readable)
                    .on_click(move |_ev, window, cx| {
                        window.close_dialog(cx);
                        let _ = page.update(cx, |page, cx| {
                            if missing {
                                page.pick_relink(&id, cx);
                            } else {
                                page.open_book(&id, window, cx);
                            }
                        });
                    })
            };

            let restart_button = {
                let page = page.clone();
                let id = id.clone();
                Button::new("book-action-restart")
                    .w_full()
                    .ghost()
                    .label(t!("Library.actions.restart").to_string())
                    .on_click(move |_ev, window, cx| {
                        window.close_dialog(cx);
                        let _ = page.update(cx, |page, cx| {
                            page.model.update(cx, |model, _cx| model.restart_book(&id));
                            page.open_book(&id, window, cx);
                        });
                    })
            };

            let info_button = {
                let page = page.clone();
                let id = id.clone();
                Button::new("book-action-info")
                    .w_full()
                    .ghost()
                    .label(t!("Library.actions.info").to_string())
                    .on_click(move |_ev, window, cx| {
                        window.close_dialog(cx);
                        let _ = page.update(cx, |page, cx| page.open_info_dialog(&id, window, cx));
                    })
            };

            let edit_button = {
                let page = page.clone();
                let id = id.clone();
                Button::new("book-action-edit")
                    .w_full()
                    .ghost()
                    .label(t!("Library.actions.edit").to_string())
                    .on_click(move |_ev, window, cx| {
                        window.close_dialog(cx);
                        let _ = page.update(cx, |page, cx| page.open_edit_dialog(&id, window, cx));
                    })
            };

            let reveal_button = {
                let page = page.clone();
                let path = path.clone();
                Button::new("book-action-reveal")
                    .w_full()
                    .ghost()
                    .label(t!("Library.actions.reveal").to_string())
                    .on_click(move |_ev, window, cx| {
                        window.close_dialog(cx);
                        let _ = page.update(cx, |page, cx| {
                            if let Err(error) = system::reveal_in_folder(&path) {
                                page.model
                                    .update(cx, |model, _cx| model.push_error(error.to_string()));
                            }
                        });
                    })
            };

            let open_external_button = {
                let page = page.clone();
                Button::new("book-action-open-external")
                    .w_full()
                    .ghost()
                    .label(t!("Library.actions.open_external").to_string())
                    .on_click(move |_ev, window, cx| {
                        window.close_dialog(cx);
                        let _ = page.update(cx, |page, cx| {
                            if let Err(error) = system::open_path(&path) {
                                page.model
                                    .update(cx, |model, _cx| model.push_error(error.to_string()));
                            }
                        });
                    })
            };

            let hide_button = {
                let page = page.clone();
                let id = id.clone();
                Button::new("book-action-hide")
                    .w_full()
                    .ghost()
                    .label(t!("Library.actions.hide").to_string())
                    .on_click(move |_ev, window, cx| {
                        window.close_dialog(cx);
                        let _ = page.update(cx, |page, cx| page.prompt_hide(&id, window, cx));
                    })
            };

            let delete_button = {
                Button::new("book-action-delete")
                    .w_full()
                    .danger()
                    .label(t!("Library.actions.delete_file").to_string())
                    .on_click(move |_ev, window, cx| {
                        window.close_dialog(cx);
                        let _ = page.update(cx, |page, cx| {
                            page.prompt_delete_file(&id, window, cx);
                        });
                    })
            };

            dialog
                .title(t!("Library.actions.title", title = &title).to_string())
                .w(px(320.))
                .child(
                    v_flex()
                        .gap_1()
                        .child(continue_button)
                        .when(finished && !missing, |this| this.child(restart_button))
                        .child(info_button)
                        .child(edit_button)
                        .child(reveal_button)
                        .child(open_external_button)
                        .child(hide_button)
                        .child(delete_button),
                )
        });
    }

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

    /// 编辑书名 / 作者 / 封面。
    pub(super) fn open_edit_dialog(
        &self,
        book_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(book) = self.model.read(cx).book_snapshot(book_id) else {
            return;
        };
        let model = self.model.clone();
        let covers_dir = self.model.read(cx).paths.covers_dir.clone();

        let title_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("Library.edit.title_placeholder").to_string())
        });
        title_input.update(cx, |state, cx| {
            state.set_value(book.title.clone(), window, cx);
        });
        let author_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("Library.edit.author_placeholder").to_string())
        });
        author_input.update(cx, |state, cx| {
            state.set_value(book.author.clone(), window, cx);
        });

        let state = cx.new(|_cx| EditState {
            book_id: book.id.clone(),
            cover_file: book.cover_file.clone(),
            picked_cover: None,
            remove_cover: false,
        });

        window.open_dialog(cx, move |dialog, _window, cx| {
            // 每帧重跑：先取当前值，再 clone 进各按钮的闭包。
            let (picked, remove_cover, cover_file) = {
                let form = state.read(cx);
                (
                    form.picked_cover.clone(),
                    form.remove_cover,
                    form.cover_file.clone(),
                )
            };
            let preview = cover_preview(
                picked.as_deref(),
                remove_cover,
                cover_file.as_deref(),
                &covers_dir,
                cx,
            );

            let pick_state = state.clone();
            let pick_button = Button::new("edit-cover-pick")
                .ghost()
                .label(t!("Library.edit.pick_cover").to_string())
                .on_click(move |_ev, _window, cx| {
                    let state = pick_state.clone();
                    cx.spawn(async move |async_cx| {
                        let file = rfd::AsyncFileDialog::new()
                            .add_filter(
                                t!("Library.edit.cover_filter").as_ref(),
                                &["png", "jpg", "jpeg", "webp", "gif"],
                            )
                            .set_title(t!("Library.edit.pick_cover").as_ref())
                            .pick_file()
                            .await;
                        let Some(file) = file else {
                            return;
                        };
                        let path = file.path().to_path_buf();
                        state.update(async_cx, |form, cx| {
                            form.picked_cover = Some(path);
                            form.remove_cover = false;
                            cx.notify();
                        });
                    })
                    .detach();
                });

            let remove_state = state.clone();
            let remove_button = Button::new("edit-cover-remove")
                .ghost()
                .label(t!("Library.edit.remove_cover").to_string())
                .on_click(move |_ev, _window, cx| {
                    remove_state.update(cx, |form, cx| {
                        form.picked_cover = None;
                        form.remove_cover = true;
                        cx.notify();
                    });
                });

            let save_model = model.clone();
            let save_covers_dir = covers_dir.clone();
            let save_state = state.clone();
            let save_title = title_input.clone();
            let save_author = author_input.clone();
            let save_button = Button::new("edit-save")
                .primary()
                .label(t!("Library.edit.save").to_string())
                .on_click(move |_ev, window, cx| {
                    let raw_title = save_title.read(cx).value().to_string();
                    let author = save_author.read(cx).value().to_string();
                    let (id, picked, remove, old_cover) = {
                        let form = save_state.read(cx);
                        (
                            form.book_id.clone(),
                            form.picked_cover.clone(),
                            form.remove_cover,
                            form.cover_file.clone(),
                        )
                    };
                    let cover_file = if let Some(path) = picked {
                        let written = std::fs::read(&path)
                            .ok()
                            .and_then(|bytes| write_cover(&save_covers_dir, &id, &bytes));
                        let Some(written) = written else {
                            // 读取 / 转码失败：不关窗，让用户重选，并把原因推给用户。
                            save_model.update(cx, |model, _cx| {
                                model.push_error(t!("Library.error.cover_failed").to_string());
                            });
                            return;
                        };
                        Some(written)
                    } else if remove {
                        None
                    } else {
                        old_cover
                    };
                    // 空书名回退到 store 里的旧标题：卡片不能没有名字。
                    let trimmed = raw_title.trim();
                    let title = if trimmed.is_empty() {
                        save_model
                            .read(cx)
                            .book_snapshot(&id)
                            .map_or_else(|| trimmed.to_string(), |book| book.title)
                    } else {
                        trimmed.to_string()
                    };
                    save_model.update(cx, |model, _cx| {
                        model.update_book_metadata(&id, &title, author.trim(), cover_file);
                    });
                    window.close_dialog(cx);
                });

            let cancel_button = Button::new("edit-cancel")
                .ghost()
                .label(t!("Library.edit.cancel").to_string())
                .on_click(|_ev, window, cx| window.close_dialog(cx));

            dialog
                .title(t!("Library.edit.title").to_string())
                .w(px(460.))
                .child(
                    h_flex()
                        .items_start()
                        .gap_4()
                        .child(
                            v_flex()
                                .gap_2()
                                .child(preview)
                                .child(h_flex().gap_2().child(pick_button).child(remove_button)),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .gap_2()
                                .child(Input::new(&title_input))
                                .child(Input::new(&author_input)),
                        ),
                )
                .footer(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(cancel_button)
                        .child(save_button),
                )
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

/// 编辑弹窗的封面预览：新选的图 > 「已移除」占位 > 现有缓存封面 > 无封面占位。
fn cover_preview(
    picked: Option<&std::path::Path>,
    remove_cover: bool,
    cover_file: Option<&str>,
    covers_dir: &std::path::Path,
    cx: &App,
) -> Div {
    let frame = div()
        .w(px(120.))
        .h(px(180.))
        .rounded_md()
        .overflow_hidden()
        .bg(cx.theme().muted);
    if let Some(path) = picked {
        return frame.child(img(path).size_full().object_fit(ObjectFit::Cover));
    }
    if !remove_cover && let Some(file) = cover_file {
        return frame.child(
            img(covers_dir.join(file))
                .size_full()
                .object_fit(ObjectFit::Cover),
        );
    }
    frame.flex().items_center().justify_center().child(
        div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(t!("Library.edit.no_cover").to_string()),
    )
}

/// 一行「标签：值」。
fn info_row(label: String, value: String, cx: &App) -> Div {
    h_flex()
        .items_start()
        .gap_2()
        .child(
            div()
                .w(px(88.))
                .flex_none()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().flex_1().child(value))
}
