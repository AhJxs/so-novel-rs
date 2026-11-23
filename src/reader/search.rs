//! 书内搜索：在原始文本上按字节匹配，保留原始字节偏移供高亮使用。
//!
//! ASCII 大小写不敏感（`A` 与 `a` 视为相同），中文按字节精确匹配。UTF-8 的多字节
//! 序列不含 ASCII 字节，所以逐字节比较不会把一个汉字拆开。

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use super::content::ReaderBook;

/// 单章最多返回的命中数。
pub const HITS_PER_CHAPTER: usize = 20;
/// 一次搜索最多返回的命中数。
pub const HITS_TOTAL: usize = 1000;
/// 命中前后各保留的字符数。
const SNIPPET_RADIUS: usize = 24;
/// 可中断扫描每处理多少段检查一次取消信号。
const CANCEL_CHECK_INTERVAL: usize = 64;
/// 永不置位的取消信号：给不需要中断的 [`search_chapter`] 共用。
static NEVER_CANCELLED: AtomicBool = AtomicBool::new(false);

/// 一章内的命中分组。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchGroup {
    pub chapter: usize,
    pub title: String,
    pub hits: Vec<SearchHit>,
}

/// 单条命中。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub paragraph: usize,
    /// 段落内**原始文本**的字节偏移（不含排版层加的首行缩进）。
    ///
    /// 高亮要用 [`SearchHit::snippet`] + [`SearchHit::snippet_match`]，或自己按缩进前缀
    /// 平移偏移；拿它直接切 `ReaderLayout::display_text` 的结果会高亮错位置。
    pub byte_offset: usize,
    pub byte_len: usize,
    /// 展示用上下文。
    pub snippet: String,
    /// `byte_offset` 在 `snippet` 中的区间。
    pub snippet_match: Range<usize>,
}

/// 在文本中找出所有命中，返回 `(字节偏移, 字节长度)`。
///
/// 不 trim；需要忽略首尾空白时由调用方先 `trim()`（[`run`] 已经这么做）。
#[must_use]
pub fn find_hits(text: &str, needle: &str) -> Vec<(usize, usize)> {
    let needle_len = needle.len();
    if needle_len == 0 {
        return Vec::new();
    }
    let haystack = text.as_bytes();
    let target = needle.as_bytes();
    text.char_indices()
        .map(|(offset, _)| offset)
        .filter(|offset| matches_at(haystack, *offset, target))
        .map(|offset| (offset, needle_len))
        .collect()
}

fn matches_at(haystack: &[u8], offset: usize, needle: &[u8]) -> bool {
    let Some(window) = haystack.get(offset..offset + needle.len()) else {
        return false;
    };
    window.iter().zip(needle).all(|(left, right)| {
        if left.is_ascii() && right.is_ascii() {
            left.eq_ignore_ascii_case(right)
        } else {
            left == right
        }
    })
}

/// 取命中附近的上下文，返回 `(片段, 片段内命中区间)`。
///
/// 单次调用方便随手用；同一段里有多条命中时先建一次
/// [`snippet_with_starts`] 要的边界表，别逐条重复扫。
#[must_use]
pub fn snippet(text: &str, offset: usize, len: usize) -> (String, Range<usize>) {
    let starts: Vec<usize> = text.char_indices().map(|(index, _)| index).collect();
    snippet_with_starts(text, &starts, offset, len)
}

/// 同 [`snippet`]，但复用调用方已经建好的字符边界表：`starts` 是
/// `text.char_indices()` 收集的字节位置（升序，首项恒为 0）。
///
/// 空表（空文本）或越界偏移返回整段文本与空区间，与 [`snippet`] 一致。
#[must_use]
pub fn snippet_with_starts(
    text: &str,
    starts: &[usize],
    offset: usize,
    len: usize,
) -> (String, Range<usize>) {
    if starts.is_empty() || offset >= text.len() {
        return (text.to_string(), 0..0);
    }
    let char_index = starts
        .partition_point(|start| *start <= offset)
        .saturating_sub(1);
    let Some(matched) = text.get(offset..offset + len) else {
        return (text.to_string(), 0..0);
    };
    let match_chars = matched.chars().count();
    let from_char = char_index.saturating_sub(SNIPPET_RADIUS);
    let to_char = char_index + match_chars + SNIPPET_RADIUS;
    let start = starts[from_char];
    let end = starts.get(to_char).copied().unwrap_or(text.len());
    let relative = offset - start;
    (text[start..end].to_string(), relative..relative + len)
}

/// 在单章里搜索，`budget` 是本轮搜索剩余的命中额度。
#[must_use]
pub fn search_chapter(
    chapter: usize,
    title: &str,
    paragraphs: &[String],
    needle: &str,
    budget: usize,
) -> Option<SearchGroup> {
    search_chapter_interruptible(chapter, title, paragraphs, needle, budget, &NEVER_CANCELLED)
}

/// 同 [`search_chapter`]，但每 [`CANCEL_CHECK_INTERVAL`] 段（以及开头）检查一次
/// `cancel`；置位时立刻返回 `None`，已经攒下的命中一并丢弃。
///
/// 检查按段做而不是按字节：一段通常只有几十到几百字，取消最多推迟 64 段（大章里
/// 也是毫秒级），换来的是热点循环里没有额外的原子读。
#[must_use]
pub fn search_chapter_interruptible(
    chapter: usize,
    title: &str,
    paragraphs: &[String],
    needle: &str,
    budget: usize,
    cancel: &AtomicBool,
) -> Option<SearchGroup> {
    let limit = budget.min(HITS_PER_CHAPTER);
    if limit == 0 || cancel.load(Ordering::Relaxed) {
        return None;
    }
    let mut hits = Vec::new();
    for (paragraph_index, paragraph) in paragraphs.iter().enumerate() {
        if paragraph_index % CANCEL_CHECK_INTERVAL == 0 && cancel.load(Ordering::Relaxed) {
            return None;
        }
        let paragraph_hits = find_hits(paragraph, needle);
        // 边界表按段建一次、本段所有命中共用；无命中的段落根本不建。
        if paragraph_hits.is_empty() {
            continue;
        }
        let starts: Vec<usize> = paragraph.char_indices().map(|(index, _)| index).collect();
        for (byte_offset, byte_len) in paragraph_hits {
            let (snippet, snippet_match) =
                snippet_with_starts(paragraph, &starts, byte_offset, byte_len);
            hits.push(SearchHit {
                paragraph: paragraph_index,
                byte_offset,
                byte_len,
                snippet,
                snippet_match,
            });
            if hits.len() >= limit {
                break;
            }
        }
        if hits.len() >= limit {
            break;
        }
    }
    (!hits.is_empty()).then(|| SearchGroup {
        chapter,
        title: title.to_string(),
        hits,
    })
}

/// 顺序扫描全书，每章结束后回调 `(已完成章数, 总章数)`；`cancel` 置位时提前结束。
#[must_use]
pub fn run(
    book: &ReaderBook,
    needle: &str,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(usize, usize),
) -> Vec<SearchGroup> {
    let needle = needle.trim();
    if needle.is_empty() {
        return Vec::new();
    }
    let total_chapters = book.chapters.len();
    let mut groups = Vec::new();
    let mut total_hits = 0;
    for (index, chapter) in book.chapters.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) || total_hits >= HITS_TOTAL {
            break;
        }
        if let Some(group) = search_chapter(
            index,
            &chapter.title,
            &chapter.paragraphs,
            needle,
            HITS_TOTAL - total_hits,
        ) {
            total_hits += group.hits.len();
            groups.push(group);
        }
        on_progress(index + 1, total_chapters);
    }
    groups
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::reader::content::{ReaderBook, ReaderChapter};

    fn book() -> ReaderBook {
        ReaderBook {
            title: "书".to_string(),
            author: String::new(),
            encoding: None,
            chapters: vec![
                ReaderChapter {
                    title: "第一章".to_string(),
                    paragraphs: vec!["Hello world hello".to_string(), "无关".to_string()],
                },
                ReaderChapter {
                    title: "第二章".to_string(),
                    paragraphs: vec!["再见 世界".to_string()],
                },
            ],
        }
    }

    #[test]
    fn ascii_match_is_case_insensitive_with_original_offsets() {
        let hits = find_hits("Hello world hello", "hello");
        assert_eq!(hits, vec![(0, 5), (12, 5)]);
    }

    #[test]
    fn chinese_match_keeps_byte_offsets() {
        let hits = find_hits("再见 世界", "世界");
        assert_eq!(hits, vec![(7, 6)]);
    }

    #[test]
    fn search_chapter_groups_hits_and_snippets() {
        let group = search_chapter(0, "第一章", &["Hello world hello".to_string()], "hello", 10)
            .expect("group");
        assert_eq!(group.hits.len(), 2);
        assert_eq!(group.hits[0].paragraph, 0);
        assert_eq!(group.hits[0].byte_offset, 0);
        assert_eq!(group.hits[0].byte_len, 5);
        let (snippet, range) = snippet("你好 Hello 世界", 7, 5);
        assert_eq!(&snippet[range], "Hello");
    }

    #[test]
    fn interruptible_scan_returns_expected_hits_and_honours_preset_cancel() {
        let paragraphs = vec!["Hello world hello".to_string(), "无关".to_string()];
        let never = AtomicBool::new(false);
        let group = search_chapter_interruptible(3, "章", &paragraphs, "hello", 7, &never)
            .expect("未取消且有命中必须返回分组");
        // 明确写死期望值（段落下标 / 字节偏移 / 片段内容），不再拿同实现互相比对。
        assert_eq!(
            group,
            SearchGroup {
                chapter: 3,
                title: "章".to_string(),
                hits: vec![
                    SearchHit {
                        paragraph: 0,
                        byte_offset: 0,
                        byte_len: 5,
                        snippet: "Hello world hello".to_string(),
                        snippet_match: 0..5,
                    },
                    SearchHit {
                        paragraph: 0,
                        byte_offset: 12,
                        byte_len: 5,
                        snippet: "Hello world hello".to_string(),
                        snippet_match: 12..17,
                    },
                ],
            }
        );

        let cancelled = AtomicBool::new(true);
        assert!(
            search_chapter_interruptible(3, "章", &paragraphs, "hello", 7, &cancelled).is_none(),
            "取消信号预置后即使有命中也不应返回结果"
        );
    }

    #[test]
    fn per_chapter_and_total_limits_apply() {
        let long = "a".repeat(40);
        let group = search_chapter(0, "章", &[long], "a", 5).expect("group");
        assert_eq!(group.hits.len(), 5);

        let book = book();
        let groups = run(&book, "hello", &AtomicBool::new(false), |_, _| {});
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].hits.len(), 2);
    }

    #[test]
    fn cancelled_search_returns_nothing() {
        let book = book();
        let cancelled = AtomicBool::new(true);
        assert!(
            run(&book, "hello", &cancelled, |_, _| {}).is_empty(),
            "取消后不应有任何命中"
        );
    }

    #[test]
    fn empty_needle_or_no_match_returns_empty() {
        let book = book();
        assert!(
            run(&book, "   ", &AtomicBool::new(false), |_, _| {}).is_empty(),
            "空白关键词不应有任何命中"
        );
        assert!(
            run(&book, "不存在", &AtomicBool::new(false), |_, _| {}).is_empty(),
            "无匹配关键词不应有任何命中"
        );
    }

    #[test]
    fn snippet_offsets_are_relative_to_the_snippet() {
        let text = format!("{}hello{}", "前缀".repeat(40), "后缀".repeat(40));
        let hits = find_hits(&text, "hello");
        assert_eq!(hits.len(), 1);
        let (byte_offset, byte_len) = hits[0];
        let (snippet, range) = snippet(&text, byte_offset, byte_len);
        assert_eq!(&snippet[range.clone()], "hello");
        assert_ne!(range.start, byte_offset, "range 是相对片段的，不是相对整段");
        assert!(range.end < snippet.len());
    }

    fn char_starts(text: &str) -> Vec<usize> {
        text.char_indices().map(|(index, _)| index).collect()
    }

    #[test]
    fn snippet_with_starts_matches_snippet_on_ascii_and_multibyte() {
        for text in ["Hello world hello", "你好 Hello 世界", "aé中😀b 再见"] {
            let starts = char_starts(text);
            for &offset in &starts {
                let len = text[offset..]
                    .chars()
                    .next()
                    .expect("char boundary")
                    .len_utf8();
                assert_eq!(
                    snippet_with_starts(text, &starts, offset, len),
                    snippet(text, offset, len),
                    "text={text:?} offset={offset} len={len}"
                );
            }
            // 越过末字之后的偏移两边都回落成整段 + 空区间。
            assert_eq!(
                snippet_with_starts(text, &starts, text.len(), 3),
                snippet(text, text.len(), 3)
            );
        }
    }

    #[test]
    fn snippet_with_starts_clamps_at_paragraph_edges() {
        // 24 个字符正好等于一个 SNIPPET_RADIUS：首尾两侧都会被夹到文本边界。
        let text = &format!("{}中段尾字", "前缀".repeat(10));
        let starts = char_starts(text);
        // 段首命中：from_char 夹到 0，片段就是整段开头。
        let (head, head_range) = snippet_with_starts(text, &starts, 0, "前".len());
        assert_eq!(head, *text);
        assert_eq!(head_range, 0.."前".len());
        // 段尾命中：to_char 超出表长，end 收到 `text.len()`，区间相对整段。
        let last = starts[starts.len() - 1];
        let (tail, tail_range) = snippet_with_starts(text, &starts, last, "字".len());
        assert_eq!(tail, *text);
        assert_eq!(tail_range, last..last + "字".len());
        assert_eq!(&tail[tail_range], "字");
        // 比半径短得多的段落：不足上下文时片段就是整段本身。
        let (short, short_range) = snippet_with_starts("ab", &char_starts("ab"), 1, 1);
        assert_eq!((short.as_str(), short_range), ("ab", 1..2));
    }

    #[test]
    fn snippet_with_starts_handles_empty_table() {
        assert_eq!(
            snippet_with_starts("abc", &[], 0, 1),
            ("abc".to_string(), 0..0)
        );
        assert_eq!(snippet_with_starts("", &[], 0, 0), (String::new(), 0..0));
        // 空文本走 `snippet` 也是同一条路径。
        assert_eq!(snippet("", 0, 0), (String::new(), 0..0));
    }

    #[test]
    fn snippet_with_starts_windows_mid_paragraph_multibyte_hits() {
        let text = format!("{}😀中{}", "甲".repeat(30), "乙".repeat(30));
        let starts = char_starts(&text);
        let offset = 30 * "甲".len(); // 90：命中起点在段落中间，前后都超过一个窗口半径
        let len = "😀中".len();
        assert_eq!(&text[offset..offset + len], "😀中");

        let (snippet, range) = snippet_with_starts(&text, &starts, offset, len);
        // 窗口规则：命中起点前后各保留 `SNIPPET_RADIUS`（24）个字符。
        assert_eq!(
            snippet,
            format!("{}😀中{}", "甲".repeat(24), "乙".repeat(24))
        );
        assert_eq!(range, 72..79);
        assert_eq!(&snippet[range], "😀中");

        // 非 char boundary 的偏移：不得 panic，回整段文本 + 空区间。
        assert_eq!(
            snippet_with_starts(&text, &starts, offset + 1, len),
            (text.clone(), 0..0)
        );
        // offset + len 越过文本末尾：同样回整段文本 + 空区间，不 panic。
        let last = *starts.last().expect("char starts");
        assert_eq!(
            snippet_with_starts(&text, &starts, last, 10),
            (text.clone(), 0..0)
        );
    }

    #[test]
    fn search_chapter_wires_snippets() {
        let group =
            search_chapter(0, "章", &["你好 hello 世界".to_string()], "hello", 10).expect("group");
        let hit = &group.hits[0];
        assert_eq!(&hit.snippet[hit.snippet_match.clone()], "hello");
        assert_eq!(hit.paragraph, 0);
    }

    #[test]
    fn progress_reports_every_scanned_chapter_once() {
        let book = book();
        let mut seen = Vec::new();
        let _ = run(&book, "hello", &AtomicBool::new(false), |done, total| {
            seen.push((done, total));
        });
        assert_eq!(seen, vec![(1, 2), (2, 2)]);
    }

    #[test]
    fn caps_come_from_the_constants() {
        let long = "a".repeat(400);
        let group = search_chapter(0, "章", &[long], "a", 100).expect("group");
        assert_eq!(group.hits.len(), HITS_PER_CHAPTER);

        // 每章 20 条上限、共 60 章 = 1200 条，总量必须被 HITS_TOTAL 截住。
        let chapters = (0..60)
            .map(|_| ReaderChapter {
                title: "章".to_string(),
                paragraphs: vec!["a".repeat(400)],
            })
            .collect();
        let book = ReaderBook {
            title: "书".to_string(),
            author: String::new(),
            encoding: None,
            chapters,
        };
        let groups = run(&book, "a", &AtomicBool::new(false), |_, _| {});
        let total: usize = groups.iter().map(|group| group.hits.len()).sum();
        assert_eq!(total, HITS_TOTAL);
    }
}
