//! 「更多」抽屉：书籍信息 + 在文件夹中显示 / 用默认程序打开。

use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::{
    AnyElement, App, Context, Div, FontWeight, InteractiveElement as _, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, div, px,
};
use rust_i18n::t;

use super::reader_page::ReaderPage;
use crate::library::Book;
use crate::utils::formatting::format_local_unix_secs;
use crate::utils::fs::format_size;
use crate::utils::system;

/// 「更多」抽屉宽度（px）。正文列宽要减掉它。
pub(super) const MORE_WIDTH: f32 = 340.0;

impl ReaderPage {
    /// 书在 store 里的最新快照；拿不到就退回打开时的副本。
    fn current_book(&self, cx: &App) -> Book {
        self.model
            .read(cx)
            .book_snapshot(&self.book_id)
            .unwrap_or_else(|| self.book.clone())
    }

    /// 「在文件夹中显示」/「用默认程序打开」共用的执行体。
    fn run_file_action(&self, path: &str, open_with_app: bool, cx: &mut Context<Self>) {
        let target = PathBuf::from(path);
        let result = if open_with_app {
            system::open_path(&target)
        } else {
            system::reveal_in_folder(&target)
        };
        if let Err(message) = result {
            self.model
                .update(cx, |model, _cx| model.push_error(message.to_string()));
        }
    }

    /// 右侧「更多」抽屉。
    pub(super) fn render_more(&self, cx: &Context<Self>) -> AnyElement {
        let book = self.current_book(cx);
        let unknown = t!("Reader.info.unknown").to_string();
        // 时间戳缺失 / 非法 / 格式化失败都显示同一个占位。
        let time_unknown = "Reader.info.unknown";

        let mut rows: Vec<Div> = Vec::new();
        rows.push(more_row(
            t!("Reader.info.author").to_string(),
            filled(&book.author, &unknown),
            cx,
        ));
        let intro = book
            .intro
            .as_deref()
            .map_or_else(|| unknown.clone(), str::to_string);
        rows.push(more_row(t!("Reader.info.intro").to_string(), intro, cx));
        rows.push(more_row(
            t!("Reader.info.format").to_string(),
            book.format.label().to_string(),
            cx,
        ));
        rows.push(more_row(
            t!("Reader.info.size").to_string(),
            format_size(book.size_bytes),
            cx,
        ));
        rows.push(more_row(
            t!("Reader.info.chapters").to_string(),
            book.chapter_count
                .map_or_else(|| unknown.clone(), |count| count.to_string()),
            cx,
        ));
        let percent = book.progress.percent.clamp(0.0, 100.0);
        rows.push(more_row(
            t!("Reader.info.progress").to_string(),
            format!("{percent:.1}%"),
            cx,
        ));
        rows.push(more_row(
            t!("Reader.info.bookmarks").to_string(),
            book.bookmarks.len().to_string(),
            cx,
        ));
        rows.push(more_row(
            t!("Reader.info.added").to_string(),
            format_local_unix_secs(book.added_unix, time_unknown, time_unknown, time_unknown),
            cx,
        ));
        rows.push(more_row(
            t!("Reader.info.last_opened").to_string(),
            book.last_opened_unix.map_or_else(
                || unknown.clone(),
                |secs| format_local_unix_secs(secs, time_unknown, time_unknown, time_unknown),
            ),
            cx,
        ));
        rows.push(more_row(
            t!("Reader.info.path").to_string(),
            book.path.clone(),
            cx,
        ));

        let title = book.title.clone();
        let reveal_path = book.path.clone();
        let open_path = book.path;

        div()
            .w(px(MORE_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .p_3()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("Reader.info.title").to_string()),
                    )
                    .child(
                        Button::new("reader-more-close")
                            .ghost()
                            .compact()
                            .label(t!("Reader.info.close"))
                            .on_click(cx.listener(|this, _event, window, cx| {
                                this.more_open = false;
                                this.focus_body(window, cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("reader-more-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .children(rows)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("reader-file-reveal")
                                    .ghost()
                                    .compact()
                                    .label(t!("Reader.info.reveal"))
                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                        this.run_file_action(&reveal_path, false, cx);
                                    })),
                            )
                            .child(
                                Button::new("reader-file-open")
                                    .ghost()
                                    .compact()
                                    .label(t!("Reader.info.open"))
                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                        this.run_file_action(&open_path, true, cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}

/// 一行「标签：值」。
fn more_row(label: String, value: String, cx: &App) -> Div {
    h_flex()
        .items_start()
        .gap_2()
        .child(
            div()
                .w(px(72.))
                .flex_none()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().flex_1().min_w_0().text_xs().child(value))
}

/// 空字符串换成占位文字。
fn filled(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}
