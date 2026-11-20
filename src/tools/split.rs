//! 按标题行把段落流切成章节。

use std::sync::LazyLock;

use regex::Regex;

use super::ToolError;
use super::source_book::SourceChapter;

/// 内置章节标题规则：第 N 章/节/回/卷，以及序章、楔子、前言、后记、尾声、番外。
pub const DEFAULT_CHAPTER_PATTERN: &str = r"^\s*(?:第[0-9零一二三四五六七八九十百千两]+[章节回卷]|序[章言]|楔子|前[言序]|后记|尾声|番外(?:篇)?)(?:[\s:：、.．—-].*)?$";

/// 编译用户自定义正则；空白或 `None` 回落到内置规则。
///
/// # Errors
///
/// 自定义正则语法错误时返回 [`ToolError::InvalidRegex`]。
pub fn compile_pattern(custom: Option<&str>) -> Result<Regex, ToolError> {
    custom
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(
            || Ok(default_regex()),
            |pattern| {
                Regex::new(pattern).map_err(|error| ToolError::InvalidRegex(error.to_string()))
            },
        )
}

/// 标题必须独占一个段落（单行），正文里的“第一章”不会被误判。
/// 标题章即使没有正文也保留（卷标题），只有首个标题前的前言会因空正文被丢弃。
#[must_use]
pub fn split_chapters(
    paragraphs: &[String],
    regex: &Regex,
    book_title: &str,
) -> Vec<SourceChapter> {
    let mut chapters = Vec::new();
    let mut saw_heading = false;
    let mut current_is_heading = false;
    let mut current = SourceChapter {
        title: "前言".to_string(),
        paragraphs: Vec::new(),
    };
    for paragraph in paragraphs {
        let trimmed = paragraph.trim();
        if trimmed.is_empty() {
            continue;
        }
        let is_heading = !trimmed.contains('\n') && regex.is_match(trimmed);
        if is_heading {
            saw_heading = true;
            if !current.paragraphs.is_empty() || current_is_heading {
                chapters.push(current);
            }
            current = SourceChapter {
                title: trimmed.to_string(),
                paragraphs: Vec::new(),
            };
            current_is_heading = true;
        } else {
            current.paragraphs.push(trimmed.to_string());
        }
    }
    if !current.paragraphs.is_empty() || (saw_heading && current_is_heading) {
        if !saw_heading && chapters.is_empty() {
            current.title = book_title.to_string();
        }
        chapters.push(current);
    }
    if chapters.is_empty() {
        let body: Vec<String> = paragraphs
            .iter()
            .map(|paragraph| paragraph.trim().to_string())
            .filter(|paragraph| !paragraph.is_empty())
            .collect();
        if !body.is_empty() {
            chapters.push(SourceChapter {
                title: book_title.to_string(),
                paragraphs: body,
            });
        }
    }
    normalize_chapters(chapters)
}

/// 对任意来源的章节做统一后处理：超长章切块（EPUB 也走这里）。
pub(crate) fn normalize_chapters(chapters: Vec<SourceChapter>) -> Vec<SourceChapter> {
    split_oversized(chapters)
}

/// 单章字符上限。没有可识别标题的文件会退回成「整本一章」，不切块的话一帧要排整本书；
/// 个别超长章节同理。1 万字约 30–40 行页宽下的十几页，单帧排版可接受。
const MAX_CHAPTER_CHARS: usize = 10_000;

/// 把超长章节按段落切块；第一块沿用原标题，后续块标题追加「（2）」「（3）」。
fn split_oversized(chapters: Vec<SourceChapter>) -> Vec<SourceChapter> {
    let mut out = Vec::with_capacity(chapters.len());
    for chapter in chapters {
        let total: usize = chapter
            .paragraphs
            .iter()
            .map(|paragraph| paragraph.chars().count() + 1)
            .sum();
        if total <= MAX_CHAPTER_CHARS {
            out.push(chapter);
            continue;
        }
        let mut parts: Vec<Vec<String>> = Vec::new();
        let mut current: Vec<String> = Vec::new();
        let mut size = 0_usize;
        for paragraph in chapter.paragraphs {
            for piece in split_paragraph(&paragraph, MAX_CHAPTER_CHARS) {
                let len = piece.chars().count() + 1;
                if size + len > MAX_CHAPTER_CHARS && !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                    size = 0;
                }
                current.push(piece);
                size += len;
            }
        }
        if !current.is_empty() {
            parts.push(current);
        }
        for (index, paragraphs) in parts.into_iter().enumerate() {
            let title = if index == 0 {
                chapter.title.clone()
            } else {
                format!("{}（{}）", chapter.title, index + 1)
            };
            out.push(SourceChapter { title, paragraphs });
        }
    }
    out
}

/// 单段超限时按句末标点切；找不到标点就按第 `limit` 个字符硬切。
/// 切出的每块不超过 `limit` 个字符（单字符 utf8 长度不影响计数）。
fn split_paragraph(paragraph: &str, limit: usize) -> Vec<String> {
    if limit == 0 || paragraph.chars().count() <= limit {
        return vec![paragraph.to_string()];
    }
    let mut out = Vec::new();
    let mut rest = paragraph;
    let mut remaining = paragraph.chars().count();
    while remaining > limit {
        let mut cut = None;
        for (chars, (byte, ch)) in rest.char_indices().enumerate() {
            // 标点后最多带 `limit` 个字符才不超限，因此只在前 `limit` 个字符里找。
            if chars >= limit {
                break;
            }
            if matches!(ch, '。' | '！' | '？' | '…') {
                cut = Some(byte + ch.len_utf8());
            }
        }
        let at = cut.unwrap_or_else(|| {
            rest.char_indices()
                .nth(limit)
                .map_or(rest.len(), |(byte, _)| byte)
        });
        // 切点来自 `char_indices` / `ch.len_utf8()`，必在字符边界上。
        let at = at.clamp(1, rest.len());
        debug_assert!(rest.is_char_boundary(at), "切点必须落在字符边界");
        let piece = &rest[..at];
        remaining -= piece.chars().count();
        out.push(piece.to_string());
        rest = &rest[at..];
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

fn default_regex() -> Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(DEFAULT_CHAPTER_PATTERN)
            .unwrap_or_else(|error| unreachable!("内置章节正则必须可编译: {error}"))
    });
    RE.clone()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::{
        MAX_CHAPTER_CHARS, SourceChapter, compile_pattern, normalize_chapters, split_chapters,
        split_oversized,
    };
    use crate::tools::source_book::SourceBook;

    fn paragraphs(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn splits_default_headings_with_preface() {
        let paragraphs = paragraphs(&["简介", "第一章 起航", "甲", "第2章 风", "乙"]);
        let chapters = split_chapters(&paragraphs, &compile_pattern(None).unwrap(), "书");
        assert_eq!(chapters.len(), 3);
        assert_eq!(chapters[0].title, "前言");
        assert_eq!(chapters[0].paragraphs, vec!["简介"]);
        assert_eq!(chapters[1].title, "第一章 起航");
        assert_eq!(chapters[1].paragraphs, vec!["甲"]);
        assert_eq!(chapters[2].title, "第2章 风");
        assert_eq!(chapters[2].paragraphs, vec!["乙"]);
    }

    #[test]
    fn chapter_at_first_line_has_no_preface() {
        let paragraphs = paragraphs(&["第1章 开始", "甲"]);
        let chapters = split_chapters(&paragraphs, &compile_pattern(None).unwrap(), "书");
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title, "第1章 开始");
    }

    #[test]
    fn keeps_heading_only_chapter() {
        let paragraphs = paragraphs(&["第一卷 风起", "第一章 甲", "正文"]);
        let regex = compile_pattern(None).unwrap();
        let chapters = split_chapters(&paragraphs, &regex, "书");
        let titles: Vec<_> = chapters.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, vec!["第一卷 风起", "第一章 甲"]);
        assert_eq!(chapters[0].paragraphs, Vec::<String>::new());
    }

    #[test]
    fn custom_regex_overrides_default() {
        let paragraphs = paragraphs(&["第一话 初见", "甲", "第二话 重逢", "乙"]);
        let regex = compile_pattern(Some(r"^第.+话(\s+.*)?$")).unwrap();
        let chapters = split_chapters(&paragraphs, &regex, "书");
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "第一话 初见");
        assert_eq!(chapters[1].title, "第二话 重逢");
    }

    #[test]
    fn no_heading_returns_single_chapter_named_after_book() {
        let paragraphs = paragraphs(&["甲", "乙"]);
        let chapters = split_chapters(&paragraphs, &compile_pattern(None).unwrap(), "无名书");
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title, "无名书");
        assert_eq!(chapters[0].paragraphs, vec!["甲", "乙"]);
    }

    #[test]
    fn heading_with_body_in_same_paragraph_is_not_a_heading() {
        let paragraphs = paragraphs(&["第一章 起航\n正文"]);
        let chapters = split_chapters(&paragraphs, &compile_pattern(None).unwrap(), "书");
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title, "书");
    }

    #[test]
    fn oversized_chapter_is_chunked() {
        let long = "字".repeat(6_000);
        let paragraphs = paragraphs(&["第一章 起", &long, &long, &long]);
        let chapters = split_chapters(&paragraphs, &compile_pattern(None).unwrap(), "书");
        // 前言空、第一章 3 段 × 6000 字 → 每块一段，共 3 块。
        assert_eq!(chapters.len(), 3);
        assert_eq!(chapters[0].title, "第一章 起");
        assert_eq!(chapters[1].title, "第一章 起（2）");
        assert_eq!(chapters[2].title, "第一章 起（3）");
        assert!(chapters.iter().all(|c| c.paragraphs.len() == 1));
    }

    #[test]
    fn splits_single_giant_paragraph() {
        let huge = "这是一句话。".repeat(5_000); // 3 万字，单段
        let chapters = split_oversized(vec![SourceChapter {
            title: "甲".to_string(),
            paragraphs: vec![huge],
        }]);
        assert!(chapters.len() >= 3);
        assert!(chapters.iter().all(|c| {
            c.paragraphs
                .iter()
                .map(|p| p.chars().count())
                .sum::<usize>()
                <= MAX_CHAPTER_CHARS
        }));
    }

    #[test]
    fn epub_chapters_are_normalized() {
        let book = SourceBook {
            chapters: vec![SourceChapter {
                title: "整本".to_string(),
                paragraphs: vec!["字".repeat(MAX_CHAPTER_CHARS * 2)],
            }],
            ..SourceBook::default()
        };
        let normalized = normalize_chapters(book.chapters);
        assert!(normalized.len() >= 2);
    }

    #[test]
    fn invalid_custom_regex_returns_error() {
        let result = compile_pattern(Some("("));
        assert!(matches!(result, Err(super::ToolError::InvalidRegex(_))));
    }

    #[test]
    fn blank_custom_regex_falls_back_to_default() {
        let regex = compile_pattern(Some("   ")).unwrap();
        assert!(regex.is_match("第三章 归途"));
    }
}
