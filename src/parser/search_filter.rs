//! 聚合搜索结果的相似度过滤排序。对应 Java `handle.SearchResultsHandler`。
//!
//! 行为（与 Java 端 1:1）：
//! 1. 用关键词分别和"书名"/"作者"批量比，得到加权总分（短/中/长查询桶不同权重）；
//! 2. 谁分高就认为"用户在搜书名 / 作者"，再用相应字段计算每条结果的相似度；
//! 3. 相似度 > 0.3 保留；按相似度降序，相似度相等时按另一个字段字典序（稳定排序）；
//! 4. 若过滤后为空，退回过滤阈值 0（仅按 > 0 保留），避免"什么都搜不到"。
//!
//! `StrUtil.similar` 的语义是 `1 - editDistance(a,b) / max(a.len,b.len)`，
//! 与 `strsim::normalized_levenshtein` 等价（Java 端按字符比，Rust 端按 char 也是字符级，
//! 中文不会被错误地按字节切）。

use crate::models::SearchResult;

/// 计算两字符串相似度（0..=1）。空串视作 0。
fn similar(a: &str, b: &str) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    strsim::normalized_levenshtein(a, b)
}

/// 字段相似度，bookName 字段从结果取 bookName，author 字段同理；缺失字段算 0。
fn field_similar(sr: &SearchResult, kw: &str, by_book_name: bool) -> f64 {
    let target = if by_book_name {
        Some(sr.book_name.as_str())
    } else {
        sr.author.as_deref()
    };
    target.map(|t| similar(kw, t)).unwrap_or(0.0)
}

/// 计算加权总分；对应 Java `getSimilarity(data, kw, type)`。
fn aggregate_score(list: &[SearchResult], kw: &str, by_book_name: bool) -> f64 {
    let n = kw.chars().count();
    let is_short = n <= 4;
    let is_long = n >= 10;

    list.iter()
        .map(|sr| {
            let s = field_similar(sr, kw, by_book_name);
            if is_short {
                if s == 1.0 {
                    12.0
                } else if s >= 0.8 {
                    s.powi(3) * 8.0
                } else if s >= 0.7 {
                    s * 5.0
                } else {
                    0.0
                }
            } else if is_long {
                if s == 1.0 {
                    10.0
                } else if s >= 0.85 {
                    s.powi(3) * 8.0
                } else if s >= 0.7 {
                    s.powi(2) * 5.0
                } else if s >= 0.5 {
                    s * 3.0
                } else {
                    s * 1.2
                }
            } else if s == 1.0 {
                10.0
            } else if s >= 0.85 {
                s.powi(3) * 8.0
            } else if s >= 0.7 {
                s.powi(2) * 5.0
            } else if s >= 0.5 {
                s * 3.0
            } else {
                0.0
            }
        })
        .sum()
}

/// 过滤 + 排序聚合搜索结果。等价 Java `SearchResultsHandler#filterSort`。
///
/// 入参 `results` 不被消耗：返回新列表。
pub fn filter_sort(results: &[SearchResult], kw: &str) -> Vec<SearchResult> {
    if results.is_empty() {
        return Vec::new();
    }
    let kw = kw.trim();
    if kw.is_empty() {
        return results.to_vec();
    }

    let book_score = aggregate_score(results, kw, true);
    let author_score = aggregate_score(results, kw, false);
    // 谁分高用谁；并列时偏向书名（与 Java `<` 判断一致：相等时不算 author 搜索）。
    let by_book_name = book_score >= author_score;

    // 缓存每条的相似度（含 idx 用于稳定排序）
    let mut scored: Vec<(usize, f64, &SearchResult)> = results
        .iter()
        .enumerate()
        .map(|(i, sr)| (i, field_similar(sr, kw, by_book_name), sr))
        .collect();

    // 排序：相似度降序；相等时按"另一字段"字典序（与 Java 一致）；再相等按原顺序保稳定。
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let ka = if by_book_name {
                    a.2.author.as_deref().unwrap_or("")
                } else {
                    a.2.book_name.as_str()
                };
                let kb = if by_book_name {
                    b.2.author.as_deref().unwrap_or("")
                } else {
                    b.2.book_name.as_str()
                };
                ka.cmp(kb)
            })
            .then_with(|| a.0.cmp(&b.0))
    });

    let above_03: Vec<SearchResult> = scored
        .iter()
        .filter(|(_, s, _)| *s > 0.3)
        .map(|(_, _, sr)| (*sr).clone())
        .collect();
    if !above_03.is_empty() {
        return above_03;
    }
    // fallback：阈值降到 > 0（Java 同样兜底）
    scored
        .iter()
        .filter(|(_, s, _)| *s > 0.0)
        .map(|(_, _, sr)| (*sr).clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sr(book: &str, author: &str, source_id: i32) -> SearchResult {
        SearchResult {
            source_id,
            source_name: format!("源{source_id}"),
            url: format!("https://x/{source_id}/{book}"),
            book_name: book.to_string(),
            author: Some(author.to_string()),
            ..SearchResult::default()
        }
    }

    #[test]
    fn empty_input_returns_empty() {
        let out = filter_sort(&[], "三体");
        assert!(out.is_empty());
    }

    #[test]
    fn empty_keyword_returns_input_as_is() {
        let list = vec![sr("天龙八部", "金庸", 1), sr("射雕英雄传", "金庸", 2)];
        let out = filter_sort(&list, "");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].book_name, "天龙八部");
        assert_eq!(out[1].book_name, "射雕英雄传");
    }

    #[test]
    fn book_name_search_sorts_by_similarity_desc() {
        // 关键词与"诡秘之主"相似度高 → 排前面
        let list = vec![
            sr("天龙八部", "金庸", 1),
            sr("诡秘之主", "爱潜水的乌贼", 2),
            sr("诡秘之主续集", "爱潜水的乌贼", 3),
        ];
        let out = filter_sort(&list, "诡秘之主");
        assert!(!out.is_empty());
        assert_eq!(out[0].book_name, "诡秘之主"); // 完全匹配优先
        assert_eq!(out[1].book_name, "诡秘之主续集"); // 部分匹配次之
                                                      // "天龙八部"相似度 0，应被过滤
        assert!(out.iter().all(|r| r.book_name != "天龙八部"));
    }

    #[test]
    fn author_search_when_keyword_better_matches_authors() {
        // 关键词"金庸"和作者完全匹配；和书名几乎不像 → 应识别为作者搜索
        let list = vec![
            sr("天龙八部", "金庸", 1),
            sr("射雕英雄传", "金庸", 2),
            sr("诡秘之主", "爱潜水的乌贼", 3),
        ];
        let out = filter_sort(&list, "金庸");
        // 两本金庸的书都该出现，乌贼那本被过滤
        let names: Vec<&str> = out.iter().map(|r| r.book_name.as_str()).collect();
        assert!(names.contains(&"天龙八部"));
        assert!(names.contains(&"射雕英雄传"));
        assert!(!names.contains(&"诡秘之主"));
    }

    #[test]
    fn falls_back_when_all_filtered_out() {
        // 阈值 > 0.3 都过不了；fallback 应返回相似度 > 0 的
        let list = vec![sr("一本书", "作者甲", 1), sr("其他书", "作者乙", 2)];
        let out = filter_sort(&list, "一");
        // 至少返回 1 条（按 fallback 规则）
        assert!(!out.is_empty());
    }

    #[test]
    fn stable_secondary_sort_by_other_field() {
        // 两本同名书；book_name 模式下，相似度并列时按作者字符串排序。
        // Rust `&str::cmp` 用 UTF-8 字节序，"乙"(0xE4..) < "甲"(0xE7..) → "乙作者"排前。
        // 与 Java `String.compareTo` 用 UTF-16 codeunit 比同样得到 乙(U+4E59) < 甲(U+7532)。
        let list = vec![sr("同名书", "甲作者", 1), sr("同名书", "乙作者", 2)];
        let out = filter_sort(&list, "同名书");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].author.as_deref(), Some("乙作者"));
        assert_eq!(out[1].author.as_deref(), Some("甲作者"));
    }

    #[test]
    fn similar_func_basics() {
        assert_eq!(similar("abc", "abc"), 1.0);
        assert_eq!(similar("", "abc"), 0.0);
        assert_eq!(similar("abc", ""), 0.0);
        // 完全不同
        assert!(similar("abc", "xyz") < 0.5);
    }
}
