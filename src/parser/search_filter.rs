//! 聚合搜索结果的相似度过滤排序。对应 Java `handle.SearchResultsHandler`。
//!
//! 相似度 = 归一化编辑距离与子串包含度的较大值: 关键词是目标子串时保底 0.6, 并按覆盖率
//! 加 0.4, 否则长书名会把精准匹配稀释到阈值以下。用 `strsim::normalized_levenshtein` 按 char 比。
//!
//! 排序: 单条独立算分（书名/作者取 max 为主信号 0.9、min 为微调 0.1，完全匹配直接 1.0），
//! 降序后按书名字典序稳定排序。**不用全列表累加总分**, 否则上百条碰巧相似的噪声会翻盘
//! 正确的单条匹配。阈值 0.25；全被过滤时退回得分 > 0 的结果。

use crate::models::SearchResult;

/// 计算两字符串相似度，结合编辑距离与子串包含度，防止长文本稀释
fn similar(kw: &str, target: &str) -> f64 {
    if kw.is_empty() || target.is_empty() {
        return 0.0;
    }

    let kw_lower = kw.to_lowercase();
    let target_lower = target.to_lowercase();

    let lev_sim = strsim::normalized_levenshtein(&kw_lower, &target_lower);

    if target_lower.contains(&kw_lower) {
        // `chars().count()` 按架构可达 usize::MAX; 用 `u32::try_from` 收敛后再
        // `f64::from`, 避免 `cast_precision_loss`。业务上长度远不到 2^32, 失真风险为 0。
        let kw_len = f64::from(u32::try_from(kw_lower.chars().count()).unwrap_or(u32::MAX));
        let tg_len = f64::from(u32::try_from(target_lower.chars().count()).unwrap_or(u32::MAX));
        // 包含关系保底 0.6, 再按覆盖率给奖励
        let contain_sim = (kw_len / tg_len).mul_add(0.4, 0.6);
        return f64::max(lev_sim, contain_sim);
    }

    lev_sim
}

/// 融合评分模型：不再二选一，而是动态混合书名和作者的匹配贡献
fn calculate_hybrid_score(sr: &SearchResult, kw: &str) -> f64 {
    // 完全匹配特判（最高优先级）。用误差区间比 f64 以规避 clippy::float_cmp。
    const ONE: f64 = 1.0;

    let book_sim = similar(kw, &sr.book_name);
    let author_sim = sr.author.as_deref().map_or(0.0, |a| similar(kw, a));
    if (book_sim - ONE).abs() < f64::EPSILON || (author_sim - ONE).abs() < f64::EPSILON {
        return 1.0;
    }

    // 取最大值为主信号、较小值为微调: 兼顾单字段强匹配与"书名+作者"混合搜索
    let max_sim = f64::max(book_sim, author_sim);
    let min_sim = f64::min(book_sim, author_sim);

    // 主信号 90%, 辅信号 10%
    max_sim * 0.9 + min_sim * 0.1
}

/// 过滤 + 排序聚合搜索结果。
pub fn filter_sort(results: &[SearchResult], kw: &str) -> Vec<SearchResult> {
    if results.is_empty() {
        return Vec::new();
    }
    let kw = kw.trim();
    if kw.is_empty() {
        return results.to_vec();
    }

    let mut scored: Vec<(usize, f64, &SearchResult)> = results
        .iter()
        .enumerate()
        .map(|(i, sr)| (i, calculate_hybrid_score(sr, kw), sr))
        .collect();

    // 排序：综合得分降序 -> 书名字典序 -> 原顺序稳定排序
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.2.book_name.cmp(&b.2.book_name))
            .then_with(|| a.0.cmp(&b.0))
    });

    let filtered: Vec<SearchResult> = scored
        .iter()
        .filter(|(_, s, _)| *s >= 0.25)
        .map(|(_, _, sr)| (*sr).clone())
        .collect();

    if !filtered.is_empty() {
        return filtered;
    }

    // fallback：全部被过滤时退回命中任意字符（得分 > 0）的结果
    scored
        .iter()
        .filter(|(_, s, _)| *s > 0.0)
        .map(|(_, _, sr)| (*sr).clone())
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
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
        let list = vec![
            sr("天龙八部", "金庸", 1),
            sr("诡秘之主", "爱潜水的乌贼", 2),
            sr("诡秘之主续集", "爱潜水的乌贼", 3),
        ];
        let out = filter_sort(&list, "诡秘之主");
        assert!(!out.is_empty());
        assert_eq!(out[0].book_name, "诡秘之主"); // 完全匹配优先
        assert_eq!(out[1].book_name, "诡秘之主续集"); // 包含关系次之
        assert!(out.iter().all(|r| r.book_name != "天龙八部"));
    }

    #[test]
    fn author_search_when_keyword_matches_authors() {
        let list = vec![
            sr("天龙八部", "金庸", 1),
            sr("射雕英雄传", "金庸", 2),
            sr("诡秘之主", "爱潜水的乌贼", 3),
        ];
        let out = filter_sort(&list, "金庸");
        let names: Vec<&str> = out.iter().map(|r| r.book_name.as_str()).collect();
        assert!(names.contains(&"天龙八部"));
        assert!(names.contains(&"射雕英雄传"));
        assert!(!names.contains(&"诡秘之主")); // 乌贼的书被过滤
    }

    #[test]
    fn stable_secondary_sort_by_book_name() {
        let list = vec![sr("B书", "相同作者", 1), sr("A书", "相同作者", 2)];
        let out = filter_sort(&list, "相同作者");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].book_name, "A书");
        assert_eq!(out[1].book_name, "B书");
    }

    /// 混合搜索（同时输入书名和作者）时混合评分模型应发挥作用，避免单字段一刀切
    #[test]
    fn hybrid_search_matches_both_fields() {
        let list = vec![
            sr("红高粱", "莫言", 1),
            sr("红楼梦", "曹雪芹", 2),
            sr("生死疲劳", "莫言", 3),
        ];
        let out = filter_sort(&list, "莫言");
        let names: Vec<&str> = out.iter().map(|r| r.book_name.as_str()).collect();
        assert!(names.contains(&"红高粱"));
        assert!(names.contains(&"生死疲劳"));
        assert!(!names.contains(&"红楼梦"));
    }

    /// 长书名下子串包含关系不能被编辑距离稀释掉而误过滤
    #[test]
    fn long_text_substring_not_filtered() {
        let list = vec![
            sr("史上第一混混之凡人修仙前传", "忘语", 1),
            sr("无关的其他小说", "某作者", 2),
        ];
        let out = filter_sort(&list, "凡人");
        // 纯编辑距离只有 2/14 = 0.14; 包含 "凡人" 后保底 0.6+
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].book_name, "史上第一混混之凡人修仙前传");
    }

    /// 回归：海量作者噪声干扰下，正确的书名匹配绝不能被错杀
    /// （全列表累加总分会被上百条碰巧相似的作者名翻盘，故改为 Per-result 独立评分）
    #[test]
    fn noise_does_not_kill_exact_book_match() {
        let kw = "爱潜水的乌贼";
        let mut list = vec![sr("爱潜水的乌贼", "某作者A", 1)]; // 精准书名匹配

        for i in 0..100 {
            list.push(sr(&format!("噪声书{i}"), "爱潜水的乌贼续", 100 + i));
        }

        let out = filter_sort(&list, kw);
        assert!(!out.is_empty(), "结果不应为空");
        assert_eq!(
            out[0].book_name, "爱潜水的乌贼",
            "精准书名匹配必须排在第一位"
        );
    }

    /// 回归：反过来，海量书名噪声下正确的作者匹配也不能被错杀
    #[test]
    fn noise_does_not_kill_exact_author_match() {
        let kw = "爱潜水的乌贼";
        let mut list = vec![sr("某本毫无关系的书", "爱潜水的乌贼", 1)]; // 精准作者匹配

        for i in 0..100 {
            list.push(sr("爱潜水的乌贼续", &format!("噪声作者{i}"), 100 + i));
        }

        let out = filter_sort(&list, kw);
        assert!(!out.is_empty(), "结果不应为空");
        assert_eq!(
            out[0].author.as_deref(),
            Some("爱潜水的乌贼"),
            "精准作者匹配必须排在第一位"
        );
    }

    #[test]
    fn similar_func_basics() {
        // 断言 float 结果在 EPSILON 内等价，避免 clippy::float_cmp
        let one = 1.0_f64;
        assert!((similar("abc", "abc") - one).abs() < f64::EPSILON);
        assert!(similar("", "abc").abs() < f64::EPSILON);
        assert!(similar("abc", "").abs() < f64::EPSILON);
        assert!((similar("abc", "ABC") - one).abs() < f64::EPSILON);
    }
}
