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

/// 一章内的命中分组。
#[derive(Debug, Clone)]
pub struct SearchGroup {
    pub chapter: usize,
    pub title: String,
    pub hits: Vec<SearchHit>,
}

/// 单条命中。
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub paragraph: usize,
    /// 段落内字节偏移。
    pub byte_offset: usize,
    pub byte_len: usize,
    /// 展示用上下文。
    pub snippet: String,
    /// `byte_offset` 在 `snippet` 中的区间。
    pub snippet_match: Range<usize>,
}

/// 在文本中找出所有命中，返回 `(字节偏移, 字节长度)`。
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
#[must_use]
pub fn snippet(text: &str, offset: usize, len: usize) -> (String, Range<usize>) {
    let starts: Vec<usize> = text.char_indices().map(|(index, _)| index).collect();
    if starts.is_empty() || offset >= text.len() {
        return (text.to_string(), 0..0);
    }
    let char_index = starts
        .partition_point(|start| *start <= offset)
        .saturating_sub(1);
    let match_chars = text[offset..offset + len].chars().count();
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
    let limit = budget.min(HITS_PER_CHAPTER);
    if limit == 0 {
        return None;
    }
    let mut hits = Vec::new();
    for (paragraph_index, paragraph) in paragraphs.iter().enumerate() {
        for (byte_offset, byte_len) in find_hits(paragraph, needle) {
            let (snippet, snippet_match) = snippet(paragraph, byte_offset, byte_len);
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
}
