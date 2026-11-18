//! 书内搜索抽屉：逐章扫描、结果分组、点击跳转与命中高亮。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, FontWeight, HighlightStyle, InteractiveElement as _, IntoElement,
    ParentElement, StatefulInteractiveElement as _, Styled, StyledText, div, px,
};
use rust_i18n::t;

use super::reader_page::ReaderPage;
use super::theme::reader_colors;
use crate::config::ReaderConfig;
use crate::reader::HITS_TOTAL;
use crate::reader::layout::count_f32;
use crate::reader::search::search_chapter;
use crate::reader::{ReaderLayout, SearchGroup, SearchHit};

/// 搜索抽屉宽度（px）。正文列宽要减掉它，与 `toc::TOC_WIDTH` 同理。
pub(super) const SEARCH_WIDTH: f32 = 320.0;

/// 搜索抽屉的全部状态。
pub(super) struct SearchPanel {
    /// 当前生效的关键词（已 trim）。
    query: String,
    groups: Vec<SearchGroup>,
    scanning: bool,
    /// 已扫描章数。
    scanned: usize,
    /// 总章数。
    total: usize,
    /// 累计命中数。
    hits: usize,
    /// 每次新搜索自增；过期任务据此退出。
    generation: u64,
    /// 当前定位的命中 `(章, 段, 字节偏移)`。
    active: Option<(usize, usize, usize)>,
}

impl SearchPanel {
    /// 空状态：还没有关键词、没有结果。
    pub(super) const fn new() -> Self {
        Self {
            query: String::new(),
            groups: Vec::new(),
            scanning: false,
            scanned: 0,
            total: 0,
            hits: 0,
            generation: 0,
            active: None,
        }
    }
}

impl ReaderPage {
    /// 开始一次新搜索：清旧结果、起后台逐章扫描。
    pub(super) fn start_search(&mut self, cx: &mut Context<Self>) {
        self.cancel_search();
        let needle = self.search_input.read(cx).value().trim().to_string();
        self.search.query.clone_from(&needle);
        self.search.groups.clear();
        self.search.scanned = 0;
        self.search.hits = 0;
        self.search.active = None;
        self.search.total = self
            .content
            .as_ref()
            .map_or(0, |content| content.chapters.len());
        if needle.is_empty() || self.search.total == 0 {
            self.search.scanning = false;
            cx.notify();
            return;
        }

        self.search.generation = self.search.generation.wrapping_add(1);
        let generation = self.search.generation;
        self.search.scanning = true;
        let cancel = Arc::new(AtomicBool::new(false));
        self.search_cancel = Some(cancel.clone());
        cx.spawn(async move |this, cx| {
            loop {
                let more = this
                    .update(cx, |page, cx| {
                        page.search_step(generation, &needle, &cancel, cx)
                    })
                    .unwrap_or(false);
                if !more {
                    break;
                }
                // 每章让出一次调度：整书扫描不会把输入框和滚动卡死。
                cx.background_executor()
                    .timer(Duration::from_millis(8))
                    .await;
            }
        })
        .detach();
        cx.notify();
    }

    /// 停掉在跑的扫描任务。
    pub(super) fn cancel_search(&mut self) {
        if let Some(cancel) = self.search_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.search.scanning = false;
    }

    /// 扫一章；返回是否还有下一章要扫。
    fn search_step(
        &mut self,
        generation: u64,
        needle: &str,
        cancel: &AtomicBool,
        cx: &mut Context<Self>,
    ) -> bool {
        if generation != self.search.generation || cancel.load(Ordering::Relaxed) {
            return false;
        }
        let index = self.search.scanned;
        let Some(chapter) = self
            .content
            .as_ref()
            .and_then(|content| content.chapters.get(index))
        else {
            self.search.scanning = false;
            self.search_cancel = None;
            cx.notify();
            return false;
        };
        let budget = HITS_TOTAL.saturating_sub(self.search.hits);
        let group = if budget == 0 {
            None
        } else {
            search_chapter(index, &chapter.title, &chapter.paragraphs, needle, budget)
        };
        if let Some(group) = group {
            self.search.hits += group.hits.len();
            self.search.groups.push(group);
        }
        self.search.scanned += 1;
        self.search.scanning =
            self.search.scanned < self.search.total && self.search.hits < HITS_TOTAL;
        let more = self.search.scanning;
        if !more {
            self.search_cancel = None;
        }
        cx.notify();
        more
    }

    /// 跳到某条命中：章按段落比例定位，并把它设为「当前命中」。
    pub(super) fn goto_hit(
        &mut self,
        chapter: usize,
        paragraph: usize,
        byte_offset: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self
            .content
            .as_ref()
            .and_then(|content| content.chapters.get(chapter))
        else {
            return;
        };
        let paragraphs = target.paragraphs.len();
        // 段落下标 / 段数经 `count_f32` 转换：`as f32` 会触发 `cast_precision_loss`（无全局放行）。
        let ratio = if paragraphs == 0 {
            0.0
        } else {
            (count_f32(paragraph) / count_f32(paragraphs)).clamp(0.0, 1.0)
        };
        // 命中可能在当前章：指纹不变时分页缓存会直接早退、`page_index` 不追随新比例，
        // 所以要作废在途防抖并强制重排一次。
        self.invalidate_scroll_commit();
        self.page_cache = None;
        self.chapter = chapter;
        self.chapter_ratio = ratio;
        self.restore_pending = true;
        self.search.active = Some((chapter, paragraph, byte_offset));
        self.record_progress(cx);
        cx.notify();
    }

    /// 当前章的高亮区间（按段落索引）；没有命中的段落是空表。
    pub(super) fn chapter_highlights(
        &self,
        reader: &ReaderConfig,
        cx: &App,
    ) -> Option<Vec<Vec<(std::ops::Range<usize>, HighlightStyle)>>> {
        let group = self
            .search
            .groups
            .iter()
            .find(|group| group.chapter == self.chapter)?;
        let count = self
            .content
            .as_ref()
            .and_then(|content| content.chapters.get(self.chapter))
            .map_or(0, |chapter| chapter.paragraphs.len());
        if count == 0 {
            return None;
        }
        let colors = reader_colors(reader.theme, cx);
        // 渲染时段落前面会加两个全角空格（6 字节），搜索结果没有这段前缀。
        let indent = if reader.indent {
            ReaderLayout::INDENT.len()
        } else {
            0
        };
        let mut rows: Vec<Vec<(std::ops::Range<usize>, HighlightStyle)>> = vec![Vec::new(); count];
        for hit in &group.hits {
            let active = self.search.active == Some((self.chapter, hit.paragraph, hit.byte_offset));
            let style = HighlightStyle {
                background_color: Some(if active {
                    colors.active_highlight
                } else {
                    colors.highlight
                }),
                ..HighlightStyle::default()
            };
            let start = hit.byte_offset + indent;
            if let Some(row) = rows.get_mut(hit.paragraph) {
                row.push((start..start + hit.byte_len, style));
            }
        }
        Some(rows)
    }

    /// 右侧搜索抽屉。
    pub(super) fn render_search(&self, cx: &Context<Self>) -> AnyElement {
        let status = if self.search.query.is_empty() {
            t!("Reader.search.hint").to_string()
        } else if self.search.scanning {
            t!(
                "Reader.search.scanning",
                done = &self.search.scanned.to_string(),
                total = &self.search.total.to_string()
            )
            .to_string()
        } else {
            t!(
                "Reader.search.results",
                count = &self.search.hits.to_string()
            )
            .to_string()
        };

        let mut results = div()
            .id("reader-search-results")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_2()
            .flex()
            .flex_col()
            .gap_1();
        for group in &self.search.groups {
            results = results.child(
                div()
                    .px_2()
                    .pt_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} · {}", group.title, group.hits.len())),
            );
            for hit in &group.hits {
                results = results.child(self.render_search_hit(group.chapter, hit, cx));
            }
        }

        div()
            .w(px(SEARCH_WIDTH))
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
                    .flex_col()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("Reader.search.title").to_string()),
                            )
                            .child(
                                Button::new("reader-search-close")
                                    .ghost()
                                    .compact()
                                    .label(t!("Reader.action.close"))
                                    .on_click(cx.listener(|this, _event, window, cx| {
                                        this.set_search_open(false, window, cx);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(Input::new(&self.search_input).cleanable(true))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(status),
                    ),
            )
            .child(results)
            .into_any_element()
    }

    fn render_search_hit(
        &self,
        chapter: usize,
        hit: &SearchHit,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let paragraph = hit.paragraph;
        let byte_offset = hit.byte_offset;
        let active = self.search.active == Some((chapter, paragraph, byte_offset));
        let colors = reader_colors(self.reader_config(cx).theme, cx);
        let highlight = if active {
            colors.active_highlight
        } else {
            colors.highlight
        };
        let hover_bg = cx.theme().list_hover;
        let active_bg = cx.theme().selection;
        let snippet = StyledText::new(hit.snippet.clone()).with_highlights([(
            hit.snippet_match.clone(),
            HighlightStyle {
                background_color: Some(highlight),
                ..HighlightStyle::default()
            },
        )]);

        div()
            .id(format!(
                "reader-search-hit-{chapter}-{paragraph}-{byte_offset}"
            ))
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_sm()
            .when(active, |this| this.bg(active_bg))
            .hover(move |style| style.bg(hover_bg))
            .child(snippet)
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.goto_hit(chapter, paragraph, byte_offset, cx);
            }))
    }
}
