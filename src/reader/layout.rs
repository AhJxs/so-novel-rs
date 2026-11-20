//! 阅读排版：分页纯函数与进度比例换算。
//!
//! 分页只依赖「每段显示几行、段后间距」，不碰文本内容，因此可以在 UI 层测量、
//! 在这里用纯函数验证，两边共用同一份行数定义。

use std::borrow::Cow;

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
