//! 阅读排版：分页纯函数与进度比例换算。
//!
//! 分页只依赖「每段显示几行、段后间距」，不碰文本内容，因此可以在 UI 层测量、
//! 在这里用纯函数验证，两边共用同一份行数定义。

use std::borrow::Cow;
use std::ops::Range;

/// 单个段落的测量结果。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ParagraphMetrics {
    /// 该段落换行后占的显示行数（至少 1）。
    pub lines: usize,
    /// 段落之后的间距；最后一段为 0。
    pub spacing_after: f32,
}

/// 当前章节的排版参数。尺寸单位统一用 f32 像素，UI 层再换算成框架的像素类型。
#[derive(Debug, Clone, PartialEq)]
pub struct ReaderLayout {
    pub font_size: f32,
    /// 行高倍数（相对字号）。
    pub line_height: f32,
    pub paragraph_spacing: f32,
    pub content_width: f32,
    pub viewport_height: f32,
    pub indent: bool,
    pub font_family: String,
    /// 正文字重（100–900）；标题固定加粗，不进这项。
    pub font_weight: u16,
}

/// 分页结果何时失效：内容宽度 / 视口高度 / 排版参数 / 章节任一变化。
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutFingerprint {
    pub chapter: usize,
    pub content_width: f32,
    pub viewport_height: f32,
    pub font_size: f32,
    pub line_height: f32,
    pub paragraph_spacing: f32,
    pub indent: bool,
    pub font_family: String,
    pub font_weight: u16,
}

impl ReaderLayout {
    /// 首行缩进前缀（两个全角空格，UTF-8 下 6 字节）。
    pub const INDENT: &str = "　　";

    /// 构造排版参数；`line_height` 传倍数，构造后 `line_height_px()` 得到实际像素。
    #[must_use]
    pub const fn new(
        font_size: f32,
        line_height: f32,
        paragraph_spacing: f32,
        content_width: f32,
        viewport_height: f32,
        indent: bool,
        font_family: String,
        font_weight: u16,
    ) -> Self {
        Self {
            font_size,
            line_height,
            paragraph_spacing,
            content_width,
            viewport_height,
            indent,
            font_family,
            font_weight,
        }
    }

    /// 单行实际高度（px）。
    #[must_use]
    pub const fn line_height_px(&self) -> f32 {
        self.font_size * self.line_height
    }

    /// 渲染 / 测量共用的段落文本（缩进前缀只在这里加，搜索偏移另算）。
    #[must_use]
    pub fn display_text<'a>(&self, paragraph: &'a str) -> Cow<'a, str> {
        if self.indent {
            Cow::Owned(format!("{}{paragraph}", Self::INDENT))
        } else {
            Cow::Borrowed(paragraph)
        }
    }

    /// 当前排版参数的分页指纹。
    #[must_use]
    pub fn fingerprint(&self, chapter: usize) -> LayoutFingerprint {
        LayoutFingerprint {
            chapter,
            content_width: self.content_width,
            viewport_height: self.viewport_height,
            font_size: self.font_size,
            line_height: self.line_height,
            paragraph_spacing: self.paragraph_spacing,
            indent: self.indent,
            font_family: self.font_family.clone(),
            font_weight: self.font_weight,
        }
    }
}

/// 计算每页起始的纵向偏移（首项恒为 0，升序）。
///
/// 页边界永远落在行边界上：某一行放不进当前页时，下一页从这一行的顶部开始，
/// 因此不会出现半行截断。段距只在段落真正结束时计入，页边界处不额外留白。
///
/// # Examples
///
/// ```
/// use so_novel_rs::reader::layout::{ParagraphMetrics, paginate};
///
/// // 一段 3 行、一段 4 行，每页放 4 行（行高 20px、视口 80px）：第二页从第 5 行开始。
/// let paragraphs = [
///     ParagraphMetrics { lines: 3, spacing_after: 0.0 },
///     ParagraphMetrics { lines: 4, spacing_after: 0.0 },
/// ];
/// assert_eq!(paginate(&paragraphs, 20.0, 80.0), vec![0.0, 80.0]);
/// ```
#[must_use]
pub fn paginate(
    paragraphs: &[ParagraphMetrics],
    line_height: f32,
    viewport_height: f32,
) -> Vec<f32> {
    let line_height = line_height.max(1.0);
    let mut tops = Vec::new();
    let mut cursor = 0.0;
    for paragraph in paragraphs {
        for _ in 0..paragraph.lines.max(1) {
            tops.push(cursor);
            cursor += line_height;
        }
        cursor += paragraph.spacing_after.max(0.0);
    }
    let mut starts = vec![0.0];
    if tops.is_empty() {
        return starts;
    }
    let viewport = viewport_height.max(line_height);
    let mut start = 0.0;
    for top in tops {
        if top > start && top + line_height > start + viewport {
            start = top;
            starts.push(top);
        }
    }
    starts
}

/// 每块（章标题 + 各段）起始的纵向偏移（px，首项恒为 0），末项是整章高度。
///
/// `offsets[j + 1]` 即第 j 块的底边，可见区间不用再对最后一块兜底。
/// 累计方式与 `paginate` 逐行相加完全一致：翻页模式要用它把「跳过页前块」的列顶
/// 对齐回整章布局，任何一处舍入差都会让页首那行被裁掉半行。
#[must_use]
pub fn paragraph_offsets(paragraphs: &[ParagraphMetrics], line_height: f32) -> Vec<f32> {
    let line_height = line_height.max(1.0);
    let mut offsets = Vec::with_capacity(paragraphs.len() + 1);
    let mut cursor = 0.0;
    for paragraph in paragraphs {
        offsets.push(cursor);
        for _ in 0..paragraph.lines.max(1) {
            cursor += line_height;
        }
        cursor += paragraph.spacing_after.max(0.0);
    }
    if !paragraphs.is_empty() {
        offsets.push(cursor);
    }
    offsets
}

/// 与 `[page_start, page_start + viewport)` 相交的块下标区间。
///
/// `offsets` 是 `paragraph_offsets` 的结果（末项为内容底边），因此块数是
/// `offsets.len() - 1`。`first` 取「包含 `page_start` 的块」（页起点落在块中间时
/// 从这一块的顶部开始渲染，上半部分由外层裁剪掉）；`end` 是第一个不早于页底的块。
/// 区间至少含一块。
#[must_use]
pub fn visible_paragraphs(page_start: f32, viewport: f32, offsets: &[f32]) -> Range<usize> {
    // 末项是内容底边而不是一块的起点：空输入与只有底边的输入都没有块。
    let Some(block_count) = offsets.len().checked_sub(1).filter(|&count| count > 0) else {
        return 0..0;
    };
    let first = offsets
        .partition_point(|&offset| offset < page_start)
        .saturating_sub(1)
        .min(block_count - 1);
    let end = offsets.partition_point(|&offset| offset < page_start + viewport);
    first..end.clamp(first + 1, block_count)
}

/// 相交块区间 →（是否渲染标题, 段落区间）。
///
/// 块 0 是章标题、块 i+1 是段落 i；`blocks` 来自 [`visible_paragraphs`]，
/// 因此 `blocks.end` 不会超过 `paragraph_count + 1`。
#[must_use]
pub fn block_window(blocks: Range<usize>, paragraph_count: usize) -> (bool, Range<usize>) {
    let show_title = blocks.start == 0;
    let start = blocks.start.saturating_sub(1);
    let end = blocks.end.saturating_sub(1).max(start).min(paragraph_count);
    (show_title, start..end)
}

/// 整章内容高度：行数 × 行高 + 段距总和。
#[must_use]
pub fn content_height(paragraphs: &[ParagraphMetrics], line_height: f32) -> f32 {
    paragraphs.iter().fold(0.0, |sum, paragraph| {
        count_f32(paragraph.lines.max(1)).mul_add(line_height.max(1.0), sum)
            + paragraph.spacing_after.max(0.0)
    })
}

/// 页号 → 章内相对进度；最后一页固定为 1.0，只有一页时为 0。
#[must_use]
pub fn ratio_for_page(page_index: usize, page_count: usize) -> f32 {
    if page_count <= 1 {
        return 0.0;
    }
    count_f32(page_index.min(page_count - 1)) / count_f32(page_count - 1)
}

/// 页号 → 章内进度；只有一页时整章已可见，进度为 1。
#[must_use]
pub fn page_ratio(page_index: usize, page_count: usize) -> f32 {
    if page_count <= 1 {
        return 1.0;
    }
    ratio_for_page(page_index, page_count)
}

/// 章内相对进度 → 最接近的页号。
#[must_use]
pub fn page_for_ratio(ratio: f32, page_count: usize) -> usize {
    if page_count <= 1 {
        return 0;
    }
    let ratio = ratio.clamp(0.0, 1.0);
    ((ratio * count_f32(page_count - 1)).round() as usize).min(page_count - 1)
}

/// 段落下标 + 段内字节偏移 → 章内字符比例。段长差异大时比「段序号 / 段数」准。
#[must_use]
pub fn char_ratio(paragraphs: &[String], index: usize, byte_offset: usize) -> f32 {
    let total: usize = paragraphs.iter().map(|p| p.chars().count() + 1).sum();
    if total == 0 {
        return 0.0;
    }
    let before: usize = paragraphs
        .iter()
        .take(index)
        .map(|p| p.chars().count() + 1)
        .sum();
    let inside = paragraphs.get(index).map_or(0, |p| {
        // 搜索结果给的是字节偏移，可能落在多字节字符中间：退到最近的字符边界。
        let end = (0..=byte_offset.min(p.len()))
            .rev()
            .find(|&offset| p.is_char_boundary(offset))
            .unwrap_or(0);
        p[..end].chars().count()
    });
    count_f32(before + inside) / count_f32(total)
}

/// 章内比例 → 段落下标（书签摘录用）。
///
/// 比例正好落在某段开头时归该段：比较用 `seen > target`（而非 `>=`），
/// 命中段首第一字符也算这一段；比例 1.0 落到最后一段。
#[must_use]
pub fn paragraph_at_ratio(paragraphs: &[String], ratio: f32) -> usize {
    let total: usize = paragraphs.iter().map(|p| p.chars().count() + 1).sum();
    if total == 0 {
        return 0;
    }
    // 比例本身就是 f32，`count_f32(total)` 不额外丢精度；整数转换由全局 allow 放行。
    let target = (ratio.clamp(0.0, 1.0) * count_f32(total)) as usize;
    let mut seen = 0;
    for (index, paragraph) in paragraphs.iter().enumerate() {
        seen += paragraph.chars().count() + 1;
        if seen > target {
            return index;
        }
    }
    paragraphs.len().saturating_sub(1)
}

/// 计数（行数 / 页数）转 f32。
///
/// 一本书的行数、页数以千计，远在 f32 的 2^24 有效位以内；集中在这一处转换，
/// 省得每个调用点各写一个 allow。阅读器正文测量的估算分支
/// （`desktop::pages::reader::content_view::measure_lines`）也走这里。
#[allow(clippy::cast_precision_loss)]
pub(crate) const fn count_f32(count: usize) -> f32 {
    count as f32
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

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
    fn empty_input_has_one_page_at_zero() {
        assert_eq!(paginate(&[], 20.0, 100.0), vec![0.0]);
    }

    #[test]
    fn exact_fit_is_single_page() {
        // 5 行 × 20px = 100px，正好等于视口
        assert_eq!(paginate(&metrics(&[5], 0.0), 20.0, 100.0), vec![0.0]);
    }

    #[test]
    fn breaks_on_line_boundaries() {
        // 10 行、每页 5 行 → 两页，第二页从 100px 开始
        assert_eq!(
            paginate(&metrics(&[10], 0.0), 20.0, 100.0),
            vec![0.0, 100.0]
        );
    }

    #[test]
    fn splits_paragraph_across_pages() {
        // 第 1 段 3 行 + 第 2 段 4 行，每页 4 行（80px）：
        // 第 1 页收 0/20/40/60 四行，第 2 页从 80px 开始
        assert_eq!(
            paginate(&metrics(&[3, 4], 0.0), 20.0, 80.0),
            vec![0.0, 80.0]
        );
    }

    #[test]
    fn paragraph_spacing_counts_only_at_paragraph_end() {
        // 每段 2 行、段距 10px，页高 50px：
        // 第 1 页放 2 行（40px）+ 段距 10px = 50px，第二段从 50px 开始
        assert_eq!(
            paginate(&metrics(&[2, 2], 10.0), 20.0, 50.0),
            vec![0.0, 50.0]
        );
    }

    #[test]
    fn viewport_shorter_than_one_line_still_progresses() {
        let pages = paginate(&metrics(&[3], 0.0), 20.0, 5.0);
        assert_eq!(pages, vec![0.0, 20.0, 40.0]);
    }

    #[test]
    fn content_height_sums_lines_and_spacing() {
        // (2*20 + 8) + (3*20 + 8) = 116
        assert!((content_height(&metrics(&[2, 3], 8.0), 20.0) - 116.0).abs() < 1e-4);
        assert!((content_height(&[], 20.0) - 0.0).abs() < 1e-4);
    }

    #[test]
    fn ratio_and_page_convert_both_ways() {
        assert!((ratio_for_page(0, 5) - 0.0).abs() < f32::EPSILON);
        assert!((ratio_for_page(4, 5) - 1.0).abs() < f32::EPSILON);
        assert!((ratio_for_page(2, 5) - 0.5).abs() < f32::EPSILON);
        assert_eq!(page_for_ratio(1.0, 5), 4);
        assert_eq!(page_for_ratio(0.5, 5), 2);
        assert_eq!(page_for_ratio(0.0, 0), 0);
        assert_eq!(page_for_ratio(9.0, 1), 0);
    }

    #[test]
    fn single_page_chapter_is_complete() {
        assert!((page_ratio(0, 1) - 1.0).abs() < f32::EPSILON);
        assert!((page_ratio(0, 3) - 0.0).abs() < f32::EPSILON);
        assert!((page_ratio(2, 3) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn layout_prefixes_indent_and_fingerprints() {
        let mut layout = ReaderLayout::new(19.0, 1.8, 12.0, 720.0, 600.0, true, String::new(), 400);
        assert_eq!(layout.display_text("正文"), "　　正文");
        let first = layout.fingerprint(3);
        layout.indent = false;
        assert_ne!(first, layout.fingerprint(3));
        assert_eq!(layout.display_text("正文"), "正文");
        assert!((layout.line_height_px() - 34.2).abs() < 0.01);
    }

    #[test]
    fn degenerate_inputs_still_progress() {
        // 段行数为 0、段距为负、行高与视口都是 0：仍然要给出严格递增的页起点。
        let pages = paginate(&metrics(&[0, 0], -4.0), 0.0, 0.0);
        assert_eq!(pages.first().copied(), Some(0.0));
        assert!(pages.windows(2).all(|pair| pair[1] > pair[0]));
        // `lines: 0` 至少按 1 行计，段距负数被夹到 0。
        assert!(content_height(&metrics(&[0], -4.0), 20.0) >= 20.0);
    }

    #[test]
    fn page_for_ratio_rounds_to_nearest() {
        // 0.2 × 4 = 0.8：四舍五入是 1，floor 会得到 0。
        assert_eq!(page_for_ratio(0.2, 5), 1);
    }

    #[test]
    fn ratio_round_trips_for_every_page() {
        for page in 0..5 {
            assert_eq!(page_for_ratio(ratio_for_page(page, 5), 5), page);
        }
    }

    #[test]
    fn char_ratio_weights_long_paragraphs() {
        let paragraphs = vec!["长".repeat(900), "短".to_string(), "尾".to_string()];
        // 第二段开头：901 / (901 + 2 + 2) ≈ 0.99558，而不是 1/3。
        let ratio = char_ratio(&paragraphs, 1, 0);
        assert!(ratio > 0.99, "实际 {ratio}");
        assert_eq!(paragraph_at_ratio(&paragraphs, 0.996), 1);
    }

    #[test]
    fn char_ratio_handles_empty_and_mid_character_offsets() {
        // 空章：没有字符，比例为 0；空章查下标也得 0（调用方另有空段早退）。
        assert!((char_ratio(&[], 0, 0) - 0.0).abs() < f32::EPSILON);
        assert_eq!(paragraph_at_ratio(&[], 0.5), 0);
        // 偏移落在多字节字符中间：退到最近的字符边界，不算半个字。
        let paragraphs = vec!["长".to_string()];
        assert!((char_ratio(&paragraphs, 0, 1) - 0.0).abs() < f32::EPSILON);
        assert!((char_ratio(&paragraphs, 0, 3) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn fingerprint_covers_every_layout_field() {
        let layout =
            ReaderLayout::new(19.0, 1.8, 12.0, 720.0, 600.0, true, "衬线".to_string(), 400);
        assert_eq!(
            layout.fingerprint(2),
            LayoutFingerprint {
                chapter: 2,
                content_width: 720.0,
                viewport_height: 600.0,
                font_size: 19.0,
                line_height: 1.8,
                paragraph_spacing: 12.0,
                indent: true,
                font_family: "衬线".to_string(),
                font_weight: 400,
            }
        );

        // 字重也要进指纹：改字重后折行会变，旧分页缓存必须失效。
        let heavier =
            ReaderLayout::new(19.0, 1.8, 12.0, 720.0, 600.0, true, "衬线".to_string(), 700);
        assert_ne!(layout.fingerprint(2), heavier.fingerprint(2));
    }

    #[test]
    fn visible_range_covers_boundary_paragraphs() {
        let offsets = vec![0.0, 100.0, 200.0, 300.0];
        assert_eq!(visible_paragraphs(150.0, 100.0, &offsets), 1..3);
        // 页起点正好落在块边界时，前一块也算进来（只多渲染一段，几何不受影响）。
        assert_eq!(visible_paragraphs(100.0, 100.0, &offsets), 0..2);
        // 空输入与末页也要给出非空、不越界的区间。
        assert_eq!(visible_paragraphs(0.0, 100.0, &[]), 0..0);
        // 末项是内容底边，因此末页只剩最后一块。
        assert_eq!(visible_paragraphs(300.0, 100.0, &offsets), 2..3);
    }

    #[test]
    fn paragraph_offsets_accumulate_lines_and_spacing() {
        // [2 行、段距 0] → 40；[3 行、段距 8] → 40 + 60 + 8 = 108。
        let paragraphs = [
            ParagraphMetrics {
                lines: 2,
                spacing_after: 0.0,
            },
            ParagraphMetrics {
                lines: 3,
                spacing_after: 8.0,
            },
        ];
        assert_eq!(paragraph_offsets(&paragraphs, 20.0), vec![0.0, 40.0, 108.0]);
        assert_eq!(paragraph_offsets(&[], 20.0), Vec::<f32>::new());
    }

    #[test]
    fn culled_range_keeps_unculled_geometry() {
        // 行数 1..=7 循环，每 4 段一组，最后一段段距为 0（与 `ensure_pagination` 一致）。
        let mut paragraphs = Vec::new();
        for lines in 1..=7 {
            for _ in 0..4 {
                paragraphs.push(ParagraphMetrics {
                    lines,
                    spacing_after: 8.0,
                });
            }
        }
        if let Some(last) = paragraphs.last_mut() {
            last.spacing_after = 0.0;
        }
        let line_height = 20.0;
        let viewport = 60.0;
        let offsets = paragraph_offsets(&paragraphs, line_height);
        // 每块一个起点，末项是整章高度。
        assert_eq!(offsets.len(), paragraphs.len() + 1);
        let pages = paginate(&paragraphs, line_height, viewport);
        assert!(pages.len() > 1);

        for start in pages {
            let range = visible_paragraphs(start, viewport, &offsets);
            assert!(range.start < range.end, "页 {start} 的可见区间为空");
            assert!(range.end <= offsets.len());
            // 首个渲染块必须包含页起点（几何上它就是列顶对齐的那一块）。
            assert!(
                offsets[range.start] <= start,
                "页 {start} 的首块 {} 在页起点之后",
                offsets[range.start]
            );
            // 与 [start, start + viewport) 相交的块一个都不能漏。
            for (index, &top) in offsets.iter().enumerate() {
                let bottom = offsets.get(index + 1).copied().unwrap_or(f32::INFINITY);
                if bottom > start && top < start + viewport {
                    assert!(
                        range.contains(&index),
                        "页 {start}：块 {index}（{top}..{bottom}）与页相交却未渲染"
                    );
                }
            }
            // 跳过页前的块后，pad_top + 列内偏移必须还原成不裁剪时的位置。
            let pad_top = offsets[range.start] - start;
            let column_start = offsets[range.start];
            for &top in &offsets[range.clone()] {
                let rendered = pad_top + (top - column_start);
                let unculled = top - start;
                assert!(
                    (rendered - unculled).abs() < 1e-3,
                    "页 {start}：块 {top} 渲染在 {rendered}，不裁剪时应为 {unculled}"
                );
            }
        }
    }

    #[test]
    fn block_window_maps_blocks_to_paragraphs() {
        // 块 0 是标题，块 i + 1 是段落 i。
        assert_eq!(block_window(0..3, 5), (true, 0..2));
        assert_eq!(block_window(2..5, 10), (false, 1..4));
        // 只渲染标题：不渲染任何段落。
        assert_eq!(block_window(0..1, 5), (true, 0..0));
        // 空块区间也不能倒挂。
        assert_eq!(block_window(5..5, 10), (false, 4..4));
        // 防御：块区间比实际段落多（例如陈旧缓存）时，end 夹到 paragraph_count。
        assert_eq!(block_window(2..7, 3), (false, 1..3));
    }

    #[test]
    fn visible_range_is_page_sized_not_chapter_sized() {
        // 代表值：5000 段、每段 2 行、段距 8px、行高 20px、视口 600px。
        // 每块 2×20+8 = 48px，一页约 12.5 块；裁剪后最多 14 块，而不是整章 5000 段。
        let paragraphs = vec![
            ParagraphMetrics {
                lines: 2,
                spacing_after: 8.0,
            };
            5000
        ];
        let offsets = paragraph_offsets(&paragraphs, 20.0);
        assert_eq!(offsets.len(), 5001);
        let range = visible_paragraphs(offsets[2500], 600.0, &offsets);
        assert!(
            range.len() <= 16,
            "翻页只应渲染当前页附近的块，实际 {}",
            range.len()
        );
    }

    #[test]
    fn page_starts_never_exceed_content_height() {
        let paragraphs = metrics(&[3, 7, 2], 8.0);
        let line_height = 20.0;
        let pages = paginate(&paragraphs, line_height, 60.0);
        let height = content_height(&paragraphs, line_height);
        // 最后一页的起点必须至少留得下一整行，UI 的越界兜底靠这条。
        assert!(
            pages.iter().all(|&top| top <= height - line_height),
            "页起点不能越过内容高度减去一行"
        );
    }
}
