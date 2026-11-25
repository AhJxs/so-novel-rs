//! 滚动模式的全书几何：每章高度、前缀和，以及「全局偏移 ↔ (章, 章内比例)」映射。
//!
//! 滚动模式把整本书首尾相接：内容总高 = Σ章高，右侧滚动条因此覆盖全书。章高只有在
//! 章节进入视口窗口时才用 `measure_lines` 精确测量；没量过的章按字数估算——几千章的
//! 书一次性测量要秒级（见 `tasks/todo.md` 的基准），而估算误差只影响还没读到的章的
//! 位置，进入视口后立刻被实测值替换。
//!
//! 这里只有纯数据与纯函数：UI 层负责测量与渲染，测试钉住映射与估算。

use std::ops::Range;

use crate::reader::ParagraphMetrics;
use crate::reader::layout::{content_height, count_f32};

/// 章节之间的留白（px，加在每章列顶）。
///
/// 渲染时是章的 `pt`，测量时算进章高；两处共用这一个值，否则章与章的接缝会越滚越偏。
pub(super) const CHAPTER_GAP: f32 = 24.0;

/// 测量与估算共用的章块高度：章顶留白 + 各块（行数 × 行高 + 块后间距）。
///
/// `metrics[0]` 约定为章标题（块后间距 `line_height * 0.75`），其余为正文段落。
/// 渲染时每个段落都有 `pb`，最后一段的段距也算进章高。
pub(super) fn chapter_height(metrics: &[ParagraphMetrics], line_height_px: f32) -> f32 {
    CHAPTER_GAP + content_height(metrics, line_height_px)
}

/// 一个字占的横向单位数（半角为 1，全角为 2）。
pub(super) fn char_units(text: &str) -> u32 {
    text.chars()
        .map(|ch| if ch.is_ascii() { 1 } else { 2 })
        .sum()
}

/// 一章的估算素材：标题与各段的字符单位数。
///
/// 与正文分开存：排版变化（字号 / 页宽）要重新估算，但字符数只随内容变。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ChapterUnits {
    pub title: u32,
    pub paragraphs: Vec<u32>,
}

/// 一段文字在 `chars_per_line` 下估算折几行（至少一行，与 `measure_lines` 一致）。
pub(super) fn estimate_lines(units: u32, chars_per_line: f32) -> usize {
    // `chars_per_line` 是「一行放得下几个全角字」，而 `units` 以半角为单位。
    let per_line = (chars_per_line * 2.0).max(2.0);
    let lines = (count_f32(units as usize) / per_line).ceil();
    // 行数是整数：`f32` 的 `ceil` 在这里不可能为负，直接转换。
    (lines as usize).max(1)
}

/// 一章的估算高度（px）。
///
/// `indent` 为真时每段多算两个全角字（`ReaderLayout::INDENT`）；估算只需要大致对齐
/// 实测高度，误差会随着阅读被实测值替换。
pub(super) fn estimate_chapter_height(
    units: &ChapterUnits,
    chars_per_line: f32,
    line_height_px: f32,
    spacing: f32,
    indent: bool,
) -> f32 {
    let title_lines = estimate_lines(units.title, chars_per_line);
    // 估算与实测共用同一份几何口径：章顶留白 + 标题行 + 标题块后间距。
    let mut height = line_height_px.mul_add(
        0.75,
        count_f32(title_lines).mul_add(line_height_px, CHAPTER_GAP),
    );
    let indent_units = if indent { 4 } else { 0 };
    for &units in &units.paragraphs {
        let lines = estimate_lines(units + indent_units, chars_per_line);
        height += count_f32(lines).mul_add(line_height_px, spacing);
    }
    height
}

/// 全书滚动几何。
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct ScrollMap {
    /// 每章实测高度（px，含章顶留白）；`None` = 还没渲染过，用 `estimates`。
    heights: Vec<Option<f32>>,
    /// 每章估算高度（px）；排版变化时整表重建。
    estimates: Vec<f32>,
    /// `prefix[i]` = 前 i 章的总高（`len = 章数 + 1`），按「实测优先」累计。
    prefix: Vec<f32>,
}

impl ScrollMap {
    /// 用估算高度建图；实测值全部为空。
    #[must_use]
    pub(super) fn new(estimates: Vec<f32>) -> Self {
        let mut map = Self {
            heights: vec![None; estimates.len()],
            prefix: Vec::with_capacity(estimates.len() + 1),
            estimates,
        };
        map.rebuild_prefix();
        map
    }

    #[must_use]
    pub(super) const fn len(&self) -> usize {
        self.heights.len()
    }

    #[must_use]
    pub(super) const fn is_empty(&self) -> bool {
        self.heights.is_empty()
    }

    /// 全书内容总高（px）。
    #[must_use]
    pub(super) fn total(&self) -> f32 {
        self.prefix.last().copied().unwrap_or(0.0)
    }

    /// 第 `chapter` 章的现用高度（实测优先）。
    #[must_use]
    pub(super) fn height(&self, chapter: usize) -> f32 {
        self.heights
            .get(chapter)
            .copied()
            .flatten()
            .or_else(|| self.estimates.get(chapter).copied())
            .unwrap_or(0.0)
    }

    /// 第 `chapter` 章在全书中的纵向起点（px）。
    #[must_use]
    pub(super) fn offset_of(&self, chapter: usize) -> f32 {
        self.prefix
            .get(chapter)
            .copied()
            .unwrap_or_else(|| self.total())
    }

    #[must_use]
    pub(super) fn is_measured(&self, chapter: usize) -> bool {
        self.heights.get(chapter).copied().flatten().is_some()
    }

    /// 记下实测章高；数值没变（半像素内）返回 `false`，调用点据此跳过重绘。
    pub(super) fn set_measured(&mut self, chapter: usize, height: f32) -> bool {
        let Some(slot) = self.heights.get_mut(chapter) else {
            return false;
        };
        if slot.is_some_and(|previous| (previous - height).abs() <= 0.5) {
            return false;
        }
        *slot = Some(height);
        self.rebuild_prefix();
        true
    }

    /// 排版变化后重估：丢掉全部实测值，换一套估算。
    ///
    /// 章数变了说明换了正文，调用方应直接重建 `ScrollMap` 而不是走这里。
    pub(super) fn rescale(&mut self, estimates: Vec<f32>) {
        if estimates.len() != self.estimates.len() {
            *self = Self::new(estimates);
            return;
        }
        self.estimates = estimates;
        self.heights.iter_mut().for_each(|slot| *slot = None);
        self.rebuild_prefix();
    }

    fn rebuild_prefix(&mut self) {
        self.prefix.clear();
        self.prefix.push(0.0);
        let mut cursor = 0.0;
        for chapter in 0..self.len() {
            cursor += self.height(chapter);
            self.prefix.push(cursor);
        }
    }

    /// 纵向偏移 →（章号, 章内比例）。
    ///
    /// 偏移是滚动容器的正向位移（0 = 书首）。章内比例沿用单章滚动时的语义：
    /// 章高不够一屏时整章可见、记为 1.0；否则按「章内可滚动距离」归一，贴底即 1.0。
    /// 贴到全书底部（或整本书一屏放得下）直接返回最后一章 / 1.0，书末那章再短也能
    /// 记为读完。
    #[must_use]
    pub(super) fn locate(&self, offset: f32, viewport: f32) -> (usize, f32) {
        let count = self.len();
        if count == 0 {
            return (0, 0.0);
        }
        let total = self.total();
        let max = (total - viewport).max(0.0);
        let offset = offset.clamp(0.0, max);
        if total <= viewport + 1.0 || offset >= max - 1.0 {
            return (count - 1, 1.0);
        }
        let chapter = self.chapter_at(offset);
        (chapter, self.ratio_in(chapter, offset, viewport))
    }

    /// 章内比例 → 纵向偏移；`locate` 的逆运算（贴底等边界会落在同一位置）。
    #[must_use]
    pub(super) fn offset_at(&self, chapter: usize, ratio: f32, viewport: f32) -> f32 {
        if self.is_empty() {
            return 0.0;
        }
        let total = self.total();
        let max = (total - viewport).max(0.0);
        if total <= viewport + 1.0 {
            return 0.0;
        }
        let chapter = chapter.min(self.len() - 1);
        // 章高不够一屏时章内没有可滚空间：比例恒落在章首（与 `position_ratio` 互逆）。
        let span = (self.height(chapter) - viewport).max(0.0);
        let offset = ratio.clamp(0.0, 1.0).mul_add(span, self.offset_of(chapter));
        offset.clamp(0.0, max)
    }

    /// 位置语义的章内比例（恢复位置用）：章高不够一屏时为 0.0，即章首。
    ///
    /// 与 [`Self::locate`] 的区别只在「章不够一屏」：那种章**进度**记 1.0（已读完），
    /// 但**位置**必须回章首——改小字号让它变得可滚动时，按 1.0 恢复会直接落到章尾。
    /// 与 [`Self::offset_at`] 互逆（见 `ratio_and_offset_round_trip`）。
    #[must_use]
    pub(super) fn position_ratio(&self, chapter: usize, offset: f32, viewport: f32) -> f32 {
        if self.is_empty() {
            return 0.0;
        }
        let chapter = chapter.min(self.len() - 1);
        let span = self.height(chapter) - viewport;
        if span <= 1.0 {
            return 0.0;
        }
        ((offset - self.offset_of(chapter)) / span).clamp(0.0, 1.0)
    }

    /// 包含 `offset` 的章号（落在章边界时归后一章）。
    #[must_use]
    pub(super) fn chapter_at(&self, offset: f32) -> usize {
        if self.is_empty() {
            return 0;
        }
        // `prefix` 有 `len + 1` 项且严格不减：找最后一个 `prefix <= offset`。
        let index = self
            .prefix
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        index.min(self.len() - 1)
    }

    /// 与视口 `[offset, offset + viewport)` 相交、再各带上 `buffer` 章的渲染窗口。
    #[must_use]
    pub(super) fn window(&self, offset: f32, viewport: f32, buffer: usize) -> Range<usize> {
        if self.is_empty() {
            return 0..0;
        }
        let first = self.chapter_at(offset).saturating_sub(buffer);
        let last = self
            .chapter_at(offset + viewport.max(0.0))
            .saturating_add(buffer)
            .min(self.len() - 1);
        first..last + 1
    }

    /// 窗口 `window` 是否完整覆盖视口 `[offset, offset + viewport)`。
    ///
    /// 覆盖时滚动无需重绘（窗口里已有的章都在位）；不覆盖就得重选窗口补章。
    #[must_use]
    pub(super) fn covers(&self, window: Range<usize>, offset: f32, viewport: f32) -> bool {
        if window.is_empty() {
            return false;
        }
        let top = self.offset_of(window.start);
        let bottom = if window.end >= self.len() {
            self.total()
        } else {
            self.offset_of(window.end)
        };
        offset >= top - 0.5 && offset + viewport <= bottom + 0.5
    }

    fn ratio_in(&self, chapter: usize, offset: f32, viewport: f32) -> f32 {
        let span = self.height(chapter) - viewport;
        if span <= 1.0 {
            // 章内不够一屏：滚到章首即整章可见（与单章滚动时的 `scroll_ratio` 一致）。
            return 1.0;
        }
        ((offset - self.offset_of(chapter)) / span).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn map(heights: &[f32]) -> ScrollMap {
        ScrollMap::new(heights.to_vec())
    }

    fn metrics(lines: &[usize], spacing: f32) -> Vec<ParagraphMetrics> {
        lines
            .iter()
            .map(|lines| ParagraphMetrics {
                lines: *lines,
                spacing_after: spacing,
            })
            .collect()
    }

    #[test]
    fn prefix_sums_track_measured_heights() {
        let mut map = map(&[100.0, 200.0, 300.0]);
        assert_eq!(map.total(), 600.0);
        assert_eq!(map.offset_of(0), 0.0);
        assert_eq!(map.offset_of(1), 100.0);
        assert_eq!(map.offset_of(3), 600.0);
        assert!(map.set_measured(1, 250.0));
        assert_eq!(map.total(), 650.0);
        assert_eq!(map.offset_of(2), 350.0);
        // 半像素内的重复测量不触发重建。
        assert!(!map.set_measured(1, 250.3));
        assert!(map.set_measured(2, 0.0));
        assert_eq!(map.total(), 350.0);
    }

    #[test]
    fn locate_maps_offsets_to_chapters() {
        let map = map(&[1000.0, 1000.0, 1000.0]);
        let viewport = 400.0;
        assert_eq!(map.chapter_at(0.0), 0);
        assert_eq!(map.chapter_at(999.0), 0);
        // 章边界归后一章。
        assert_eq!(map.chapter_at(1000.0), 1);
        assert_eq!(map.chapter_at(10_000.0), 2);
        let (chapter, ratio) = map.locate(0.0, viewport);
        assert_eq!(chapter, 0);
        assert!(ratio.abs() < 1e-6);
        // 章内可滚动距离是章高 - 视口：走到 600 即整章读完（章首在视口顶）。
        let (chapter, ratio) = map.locate(600.0, viewport);
        assert_eq!(chapter, 0);
        assert!((ratio - 1.0).abs() < 1e-6);
        // 深处：1100 → 第 1 章第 100px，span = 600 → 1/6。
        let (chapter, ratio) = map.locate(1100.0, viewport);
        assert_eq!(chapter, 1);
        assert!((ratio - (100.0 / 600.0)).abs() < 1e-6);
    }

    #[test]
    fn short_chapters_are_fully_read_once_visible() {
        let map = map(&[300.0, 300.0]);
        // 章高不够一屏：滚到章首就整章可见。
        let (chapter, ratio) = map.locate(300.0, 600.0);
        assert_eq!(chapter, 1);
        assert!((ratio - 1.0).abs() < 1e-6);
        // 整本书一屏放得下：直接算读完。
        let (chapter, ratio) = map.locate(0.0, 600.0);
        assert_eq!(chapter, 1);
        assert!((ratio - 1.0).abs() < 1e-6);
    }

    #[test]
    fn bottom_of_book_is_always_complete() {
        let map = map(&[5000.0, 300.0]);
        let viewport = 600.0;
        let max = map.total() - viewport;
        let (chapter, ratio) = map.locate(max, viewport);
        assert_eq!(chapter, 1);
        assert!((ratio - 1.0).abs() < 1e-6);
        // 超界的偏移（弹性回弹）也夹在书末。
        let (chapter, ratio) = map.locate(max + 400.0, viewport);
        assert_eq!(chapter, 1);
        assert!((ratio - 1.0).abs() < 1e-6);
    }

    #[test]
    fn ratio_and_offset_round_trip() {
        let map = map(&[1200.0, 900.0, 2000.0]);
        let viewport = 500.0;
        for chapter in 0..3 {
            for ratio in [0.0, 0.25, 0.5, 0.9] {
                let offset = map.offset_at(chapter, ratio, viewport);
                let (found, found_ratio) = map.locate(offset, viewport);
                assert_eq!(found, chapter, "章 {chapter} 比例 {ratio}");
                assert!(
                    (found_ratio - ratio).abs() < 1e-4,
                    "章 {chapter} 比例 {ratio} → {found_ratio}"
                );
            }
        }
        // 越界的章号 / 比例被夹住，不会滚出内容。
        let total = map.total();
        assert!(map.offset_at(99, 2.0, viewport) <= total - viewport + 1e-3);
        assert!(map.offset_at(0, -1.0, viewport) >= 0.0);
    }

    #[test]
    fn offset_at_keeps_short_chapters_at_their_top() {
        let map = map(&[200.0, 2000.0]);
        let viewport = 600.0;
        // 一屏放得下的章比例恒为 1.0：恢复时仍落在章首，不是章尾。
        assert!(map.offset_at(0, 1.0, viewport).abs() < 1e-6);
    }

    #[test]
    fn position_ratio_round_trips_with_offset_at() {
        let map = map(&[200.0, 2000.0]);
        let viewport = 600.0;
        // 一屏放得下的章：位置比例是 0.0（章首），不是进度语义的 1.0。
        assert_eq!(map.position_ratio(0, 100.0, viewport), 0.0);
        assert!(map.offset_at(0, 0.0, viewport).abs() < 1e-6);
        // 可滚动的章：位置比例与 `locate` 的比例一致，且能原样还原偏移。
        let offset = 200.0 + 300.0;
        let (chapter, ratio) = map.locate(offset, viewport);
        assert_eq!(chapter, 1);
        assert!((map.position_ratio(chapter, offset, viewport) - ratio).abs() < 1e-6);
        assert!((map.offset_at(chapter, ratio, viewport) - offset).abs() < 1e-3);
        // 空图的兜底。
        assert_eq!(ScrollMap::default().position_ratio(0, 0.0, 600.0), 0.0);
    }

    #[test]
    fn window_covers_visible_band_with_buffer() {
        let map = map(&[1000.0, 1000.0, 1000.0, 1000.0]);
        let viewport = 400.0;
        assert_eq!(map.window(0.0, viewport, 1), 0..2);
        assert_eq!(map.window(1000.0, viewport, 1), 0..3);
        assert_eq!(map.window(2100.0, viewport, 1), 1..4);
        // 书末的窗口不会越界。
        assert_eq!(map.window(3600.0, viewport, 2), 1..4);
        assert!(map.covers(0..2, 0.0, viewport));
        // 视口仍落在窗口内（第 0–1 章，0..2000）：不用补章。
        assert!(map.covers(0..2, 1000.0, viewport));
        // 视口越过窗口下沿（2000）就得重选窗口。
        assert!(!map.covers(0..2, 2000.0, viewport));
        // 底部窗口覆盖到书末。
        assert!(map.covers(1..4, 3000.0, viewport));
    }

    #[test]
    fn rescale_drops_measurements_but_keeps_chapter_count() {
        let mut map = map(&[100.0, 200.0]);
        assert!(map.set_measured(0, 150.0));
        map.rescale(vec![300.0, 400.0]);
        assert_eq!(map.total(), 700.0);
        assert!(!map.is_measured(0));
        // 章数变化时整张表重建。
        map.rescale(vec![50.0]);
        assert_eq!(map.len(), 1);
        assert_eq!(map.total(), 50.0);
    }

    #[test]
    fn chapter_height_matches_render_geometry() {
        // 标题 2 行 + 块后 0.75 行；两段各 3 行，段距 8（末段也有 pb）。
        let mut metrics = metrics(&[2, 3, 3], 8.0);
        metrics[0].spacing_after = 15.0;
        let height = chapter_height(&metrics, 20.0);
        assert!((height - (CHAPTER_GAP + 40.0 + 15.0 + 60.0 + 8.0 + 60.0 + 8.0)).abs() < 1e-4);
    }

    #[test]
    fn estimates_grow_with_text_and_typography() {
        let units = ChapterUnits {
            title: 10,
            paragraphs: vec![800, 400],
        };
        let base = estimate_chapter_height(&units, 35.0, 32.0, 12.0, true);
        let wider = estimate_chapter_height(&units, 50.0, 32.0, 12.0, true);
        let bigger = estimate_chapter_height(&units, 35.0, 40.0, 12.0, true);
        assert!(base > 0.0);
        assert!(wider < base, "每行字多了，行数应变少");
        assert!(bigger > base, "行高变大，章高应变高");
        // 缩进会多算两个全角字。
        let no_indent = estimate_chapter_height(&units, 35.0, 32.0, 12.0, false);
        assert!(no_indent <= base);
    }

    #[test]
    fn estimate_lines_handles_ascii_and_empty_text() {
        assert_eq!(estimate_lines(0, 40.0), 1);
        assert_eq!(estimate_lines(80, 40.0), 1); // 40 个全角字 = 80 半角单位
        assert_eq!(estimate_lines(81, 40.0), 2);
        assert_eq!(char_units("abc"), 3);
        assert_eq!(char_units("中文"), 4);
        assert_eq!(char_units("a中"), 3);
    }

    #[test]
    fn empty_map_is_harmless() {
        let map = map(&[]);
        assert!(map.is_empty());
        assert_eq!(map.total(), 0.0);
        assert_eq!(map.locate(0.0, 600.0), (0, 0.0));
        assert_eq!(map.offset_at(3, 0.5, 600.0), 0.0);
        assert_eq!(map.window(0.0, 600.0, 1), 0..0);
        assert!(!map.covers(0..0, 0.0, 600.0));
    }
}
