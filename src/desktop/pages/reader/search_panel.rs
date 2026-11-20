//! 书内搜索抽屉：逐章扫描、结果分组、点击跳转与命中高亮。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, FontWeight, HighlightStyle, InteractiveElement as _, IntoElement,
    ParentElement, ScrollHandle, StatefulInteractiveElement as _, Styled, StyledText, div, px,
};
use rust_i18n::t;
use smol::channel;

use super::reader_page::ReaderPage;
use super::theme::{ReaderColors, reader_colors};
use crate::config::ReaderConfig;
use crate::reader::content::ReaderBook;
use crate::reader::search::search_chapter_interruptible;
use crate::reader::{HITS_TOTAL, ReaderLayout, SearchGroup, SearchHit};

/// 搜索抽屉宽度（px）。正文列宽要减掉它，与 `toc::TOC_WIDTH` 同理。
pub(super) const SEARCH_WIDTH: f32 = 320.0;

/// 后台扫描每累计多少章回送一批；整批只触发一次重绘。
const BATCH_CHAPTERS: usize = 32;

/// 后台扫描回送的事件。
enum SearchEvent {
    /// 一批命中；可能为空（这 [`BATCH_CHAPTERS`] 章没有匹配），但仍代表进度。
    Batch(Vec<SearchGroup>),
    /// 扫描结束（扫完或命中额度耗尽）。
    Done,
}

/// 段落内的一段命中：字节区间 + 是否为当前定位的命中。
type HitRange = (std::ops::Range<usize>, bool);

/// 同一段落里的命中区间整理成「升序、互不重叠、active 单独一段」。
///
/// `find_hits` 在每个 char 边界都试匹配，自重叠的 needle（「哈哈」于「哈哈哈」）
/// 会给出互相重叠的区间；而 `StyledText` 的 run 不做重叠补偿（每个 run 从上一个
/// run 的末尾起算），重叠区间直接传进去会把后面几个字染错。
fn merge_hit_ranges(hits: &[HitRange]) -> Vec<HitRange> {
    let mut plain: Vec<std::ops::Range<usize>> = hits
        .iter()
        .filter(|(_, active)| !active)
        .map(|(range, _)| range.clone())
        .collect();
    plain.sort_by_key(|range| range.start);
    let mut merged: Vec<std::ops::Range<usize>> = Vec::new();
    for range in plain {
        match merged.last_mut() {
            // 相邻（`start == end`）也合并：连续染成一整段与分段染在视觉上一致。
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }

    let Some(active) = hits
        .iter()
        .find(|(_, active)| *active)
        .map(|(range, _)| range.clone())
    else {
        return merged.into_iter().map(|range| (range, false)).collect();
    };

    // active 命中要保住自己的底色：把压到它的普通区间切成左右两段。
    let mut out: Vec<(std::ops::Range<usize>, bool)> = Vec::new();
    for range in merged {
        if range.end <= active.start || range.start >= active.end {
            out.push((range, false));
            continue;
        }
        if range.start < active.start {
            out.push((range.start..active.start, false));
        }
        if range.end > active.end {
            out.push((active.end..range.end, false));
        }
    }
    out.push((active, true));
    out.sort_by_key(|(range, _)| range.start);
    out
}

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
    /// 每次新搜索 / 取消都自增；过期任务的回包据此丢弃。
    generation: u64,
    /// 当前定位的命中 `(章, 段, 字节偏移)`。
    active: Option<(usize, usize, usize)>,
    /// 结果列表的滚动位置。
    pub(super) scroll: ScrollHandle,
}

impl SearchPanel {
    /// 空状态：还没有关键词、没有结果。
    pub(super) fn new() -> Self {
        Self {
            query: String::new(),
            groups: Vec::new(),
            scanning: false,
            scanned: 0,
            total: 0,
            hits: 0,
            generation: 0,
            active: None,
            scroll: ScrollHandle::new(),
        }
    }
}

/// 后台扫描整本书：每 [`BATCH_CHAPTERS`] 章回送一批，扫完回送 [`SearchEvent::Done`]。
///
/// 只碰 `Arc<ReaderBook>` 快照、取消标志和 channel，不碰任何 `Entity`。取消置位
/// 或接收端消失（抽屉已关）时立即返回；命中额度跨章累计，与旧的逐章 UI 扫描一致。
fn scan_book(
    book: &ReaderBook,
    needle: &str,
    cancel: &AtomicBool,
    tx: &channel::Sender<SearchEvent>,
) {
    let mut total_hits = 0_usize;
    let mut chapters_done = 0_usize;
    let mut batch = Vec::new();
    for (index, chapter) in book.chapters.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        if total_hits >= HITS_TOTAL {
            break;
        }
        if let Some(group) = search_chapter_interruptible(
            index,
            &chapter.title,
            &chapter.paragraphs,
            needle,
            HITS_TOTAL - total_hits,
            cancel,
        ) {
            total_hits += group.hits.len();
            batch.push(group);
        }
        chapters_done += 1;
        if chapters_done.is_multiple_of(BATCH_CHAPTERS)
            && tx
                .try_send(SearchEvent::Batch(std::mem::take(&mut batch)))
                .is_err()
        {
            return;
        }
    }
    if !batch.is_empty() && tx.try_send(SearchEvent::Batch(batch)).is_err() {
        return;
    }
    let _ = tx.try_send(SearchEvent::Done);
}

impl ReaderPage {
    /// 开始一次新搜索：快照全书、起后台扫描，前台只负责收批。
    pub(super) fn start_search(&mut self, cx: &mut Context<Self>) {
        // 顺带停掉旧任务并推进 generation：旧任务还在路上的回包一律作废。
        self.cancel_search();
        let needle = self.search_input.read(cx).value().trim().to_string();
        self.search.query.clone_from(&needle);
        let Some(content) = self.content.as_ref() else {
            self.search.total = 0;
            self.search.scanning = false;
            cx.notify();
            return;
        };
        self.search.total = content.chapters.len();
        if needle.is_empty() || self.search.total == 0 {
            self.search.scanning = false;
            cx.notify();
            return;
        }

        // 整本书只快照一次：后台拿到 `Arc` 后逐章扫描，UI 线程一次也不进扫描循环。
        let book = Arc::new(content.clone());
        let generation = self.search.generation;
        let cancel = Arc::new(AtomicBool::new(false));
        self.search_cancel = Some(cancel.clone());
        self.search.scanning = true;

        let (tx, rx) = channel::unbounded::<SearchEvent>();
        cx.background_executor()
            .spawn(async move {
                scan_book(&book, &needle, &cancel, &tx);
            })
            .detach();
        // 前台接收循环：每批回写一次结果并重绘；generation 变了（取消 / 新搜索）就退出。
        cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv().await {
                let done = matches!(event, SearchEvent::Done);
                let current = this
                    .update(cx, |page, cx| {
                        if page.search.generation != generation {
                            return false;
                        }
                        match event {
                            SearchEvent::Batch(groups) => page.apply_search_batch(groups),
                            SearchEvent::Done => page.finish_search(),
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if done || !current {
                    break;
                }
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
        // 后台批次可能已经在 channel 里排队：只翻转标志不够，接收循环还要靠
        // generation 认出这些回包属于被取消的旧搜索，而不是下一次搜索。
        self.search.generation = self.search.generation.wrapping_add(1);
        self.search.scanning = false;
        // 关掉抽屉 / 重载后不能把高亮留在正文上：没有可见的结果列表时，满屏黄块既无法
        // 解释也无法清除。查询词保留，重新按回车可以再扫一遍（关闭抽屉时由 `set_search_open` 清空输入）。
        self.search.groups.clear();
        self.search.hits = 0;
        self.search.active = None;
        self.search.scanned = 0;
    }

    /// 收下一批后台命中；一批覆盖 [`BATCH_CHAPTERS`] 章，进度按批推进（末批按总章数截断）。
    fn apply_search_batch(&mut self, groups: Vec<SearchGroup>) {
        self.search.scanned = self
            .search
            .scanned
            .saturating_add(BATCH_CHAPTERS)
            .min(self.search.total);
        for group in groups {
            self.search.hits += group.hits.len();
            self.search.groups.push(group);
        }
    }

    /// 后台扫描结束：停掉进度提示，取消句柄可以丢掉了。
    fn finish_search(&mut self) {
        self.search.scanning = false;
        self.search_cancel = None;
    }

    /// 跳到某条命中：按命中在章内的字符偏移定位，并把它设为「当前命中」。
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
        let ratio = crate::reader::layout::char_ratio(&target.paragraphs, paragraph, byte_offset);
        // 命中可能在当前章：指纹不变时分页缓存会直接早退、`page_index` 不追随新比例，
        // 所以要作废在途防抖并强制重排一次。
        self.reset_chapter_navigation(false);
        self.chapter = chapter;
        self.chapter_ratio = ratio;
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
        let mut per_paragraph: Vec<Vec<(std::ops::Range<usize>, bool)>> = vec![Vec::new(); count];
        for hit in &group.hits {
            let start = hit.byte_offset + indent;
            let active = self.search.active == Some((self.chapter, hit.paragraph, hit.byte_offset));
            if let Some(row) = per_paragraph.get_mut(hit.paragraph) {
                row.push((start..start + hit.byte_len, active));
            }
        }
        let mut rows: Vec<Vec<(std::ops::Range<usize>, HighlightStyle)>> =
            Vec::with_capacity(count);
        for hits in per_paragraph {
            rows.push(
                merge_hit_ranges(&hits)
                    .into_iter()
                    .map(|(range, active)| {
                        let style = HighlightStyle {
                            background_color: Some(if active {
                                colors.active_highlight
                            } else {
                                colors.highlight
                            }),
                            ..HighlightStyle::default()
                        };
                        (range, style)
                    })
                    .collect(),
            );
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

        // 配色一帧只取一次：`reader_config` 会 clone `font_family`，逐行取在上千条结果时是纯浪费。
        let colors = reader_colors(self.reader_config(cx).theme, cx);

        let mut results = div()
            .id("reader-search-results")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.search.scroll)
            .vertical_scrollbar(&self.search.scroll)
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
                results = results.child(self.render_search_hit(group.chapter, hit, colors, cx));
            }
        }

        div()
            .w(px(SEARCH_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            // 抽屉属于应用外壳：底色 / 前景跟应用主题，别让浅色纸面把按钮字吞掉。
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
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
                                    .icon(IconName::Close)
                                    .tooltip(t!("Reader.action.close").to_string())
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
        colors: ReaderColors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let paragraph = hit.paragraph;
        let byte_offset = hit.byte_offset;
        let active = self.search.active == Some((chapter, paragraph, byte_offset));
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
            .on_click(cx.listener(move |this, _event, window, cx| {
                this.goto_hit(chapter, paragraph, byte_offset, cx);
                // 跳转后把焦点从输入框收回正文，方向键才不会继续编辑查询词。
                this.focus_body(window, cx);
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::merge_hit_ranges;

    #[test]
    fn overlapping_hits_merge_into_one_span() {
        // 「哈哈」在「哈哈哈」里给出 (0..6) 与 (3..9)。
        let merged = merge_hit_ranges(&[(0..6, false), (3..9, false)]);
        assert_eq!(merged, vec![(0..9, false)]);
    }

    #[test]
    fn adjacent_hits_merge_and_gaps_stay_separate() {
        let merged = merge_hit_ranges(&[(0..2, false), (2..4, false), (10..12, false)]);
        assert_eq!(merged, vec![(0..4, false), (10..12, false)]);
    }

    #[test]
    fn active_hit_is_cut_out_of_the_merged_span() {
        // 普通命中 (0..9) 与 active (3..6)：active 保住自己的底色，左右各留一段。
        let merged = merge_hit_ranges(&[(0..9, false), (3..6, true)]);
        assert_eq!(merged, vec![(0..3, false), (3..6, true), (6..9, false)]);
        // active 覆盖整段时不留空片段。
        let merged = merge_hit_ranges(&[(0..9, false), (0..9, true)]);
        assert_eq!(merged, vec![(0..9, true)]);
        // active 落在普通命中之外时只按起点排好序。
        let merged = merge_hit_ranges(&[(0..3, false), (8..10, true)]);
        assert_eq!(merged, vec![(0..3, false), (8..10, true)]);
    }
}
