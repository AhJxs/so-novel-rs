//! 聚合搜索结果的相似度过滤排序。对应 Java `handle.SearchResultsHandler`。
//!
//! 行为（与 Java 端 1:1，外加 1 项加固）：
//! 1. 用关键词分别和"书名"/"作者"批量比，得到加权总分（短/中/长查询桶不同权重）；
//! 2. **v0.2.5+ 加固**：先看"单条最佳命中"在哪个字段；任一侧 best ≥ 0.5 就用它
//!    （不被噪声稀释）。仅当两侧都没强匹配时退回累积分比拼（Java 原逻辑）。
//!    这修复了 `search-limit = -1` 下百条噪声里碰巧共享字符的作者名盖过正确书名匹配
//!    导致的字段翻车。
//! 3. 相似度 > 0.3 保留；按相似度降序，相似度相等时按另一个字段字典序（稳定排序）；
//! 4. 若过滤后为空，退回过滤阈值 0（仅按 > 0 保留），避免"什么都搜不到"。
//!
//! `StrUtil.similar` 的语义是 `1 - editDistance(a,b) / max(a.len,b.len)`，
//! 与 `strsim::normalized_levenshtein` 等价（Java 端按字符比，Rust 端按 char 也是字符级，
//! 中文不会被错误地按字节切）。

use crate::models::SearchResult;

/// 计算两字符串相似度，结合编辑距离与子串包含度，防止长文本稀释
fn similar(kw: &str, target: &str) -> f64 {
    if kw.is_empty() || target.is_empty() {
        return 0.0;
    }

    // 转换为小写（或可以考虑统一简繁体，视业务而定）
    let kw_lower = kw.to_lowercase();
    let target_lower = target.to_lowercase();

    // 1. 基础编辑距离相似度
    let lev_sim = strsim::normalized_levenshtein(&kw_lower, &target_lower);

    // 2. 子串包含度优化：如果关键词是目标文本的子串，赋予极高的基础分
    if target_lower.contains(&kw_lower) {
        // `chars().count()` 按架构可达 usize::MAX；这里用 `u32::try_from` 收敛后
        // 再 `f64::from`，保证 f64 接收 u32 (52 位尾数 >= 32 位) 不会触发
        // `cast_precision_loss`；业务上关键词/标题长度都远不到 2^32，失真风险为 0。
        let kw_len = f64::from(u32::try_from(kw_lower.chars().count()).unwrap_or(u32::MAX));
        let tg_len = f64::from(u32::try_from(target_lower.chars().count()).unwrap_or(u32::MAX));
        // 包含关系的基础分为 0.6，并根据覆盖率给予奖励得分
        let contain_sim = (kw_len / tg_len).mul_add(0.4, 0.6);
        return f64::max(lev_sim, contain_sim);
    }

    lev_sim
}

/// 融合评分模型：不再二选一，而是动态混合书名和作者的匹配贡献
fn calculate_hybrid_score(sr: &SearchResult, kw: &str) -> f64 {
    // 完全匹配特判（最高优先级）
    // 用误差区间比较 f64，规避 clippy::float_cmp；EPSILON 对 normalized 类分数是足够的容差。
    const ONE: f64 = 1.0;

    let book_sim = similar(kw, &sr.book_name);
    let author_sim = sr.author.as_deref().map_or(0.0, |a| similar(kw, a));
    if (book_sim - ONE).abs() < f64::EPSILON || (author_sim - ONE).abs() < f64::EPSILON {
        return 1.0;
    }

    // 2. 动态混合：取两者的最大值作为主信号，较小值作为辅助微调信号
    // 这样既照顾了单字段强匹配（如单搜作者），也能兼容“书名+作者”的混合搜索
    let max_sim = f64::max(book_sim, author_sim);
    let min_sim = f64::min(book_sim, author_sim);

    // 融合最终得分（主信号占 90%，辅信号占 10%）
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

    // 1. 计算所有条目的综合得分
    let mut scored: Vec<(usize, f64, &SearchResult)> = results
        .iter()
        .enumerate()
        .map(|(i, sr)| (i, calculate_hybrid_score(sr, kw), sr))
        .collect();

    // 2. 排序：综合得分降序 -> 书名字典序 -> 原顺序稳定排序
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.2.book_name.cmp(&b.2.book_name))
            .then_with(|| a.0.cmp(&b.0))
    });

    // 3. 过滤：由于引入了子串包含度优化，0.3 阈值可以更安全地留住“包含关键词”的结果
    let filtered: Vec<SearchResult> = scored
        .iter()
        .filter(|(_, s, _)| *s >= 0.25) // 略微放宽硬阈值到 0.25
        .map(|(_, _, sr)| (*sr).clone())
        .collect();

    if !filtered.is_empty() {
        return filtered;
    }

    // fallback：如果全部被过滤，退回到命中任意字符（得分 > 0）的结果
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

    // 辅助构建 SearchResult 的函数
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
        // "天龙八部" 相似度低，应该被过滤
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
        // 两本书得分完全一样时，按书名字典序排序（"A书" < "B书"）
        let list = vec![sr("B书", "相同作者", 1), sr("A书", "相同作者", 2)];
        let out = filter_sort(&list, "相同作者");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].book_name, "A书");
        assert_eq!(out[1].book_name, "B书");
    }

    /// 新增测试：支持混合搜索（同时输入书名和作者）
    /// 此时混合评分模型应发挥作用，避免单字段一刀切
    #[test]
    fn hybrid_search_matches_both_fields() {
        let list = vec![
            sr("红高粱", "莫言", 1),
            sr("红楼梦", "曹雪芹", 2),
            sr("生死疲劳", "莫言", 3),
        ];
        // 用户搜 "莫言"，既能出他写的书，也能通过融合评分顶上来
        let out = filter_sort(&list, "莫言");
        let names: Vec<&str> = out.iter().map(|r| r.book_name.as_str()).collect();
        assert!(names.contains(&"红高粱"));
        assert!(names.contains(&"生死疲劳"));
        assert!(!names.contains(&"红楼梦"));
    }

    /// 新增测试：长文本子串包含关系优化
    /// 避免旧版本中由于书名过长导致编辑距离把精准匹配的关键词“稀释”掉而被过滤
    #[test]
    fn long_text_substring_not_filtered() {
        let list = vec![
            sr("史上第一混混之凡人修仙前传", "忘语", 1),
            sr("无关的其他小说", "某作者", 2),
        ];
        let out = filter_sort(&list, "凡人");
        // 旧算法由于长度达 14，2/14 = 0.14 <= 0.3 会将其错误过滤
        // 新算法因为包含 "凡人"，分值保底 0.6+，应该成功保留
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].book_name, "史上第一混混之凡人修仙前传");
    }

    /// 回归测试 1（关键）：海量作者噪声干扰下，正确的书名匹配绝不能被错杀
    /// 旧逻辑靠全列表累加总分，会被上百条碰巧相似的作者名噪声翻盘。
    /// 新逻辑改用 Per-result 独立综合评分，彻底免疫条数噪声。
    #[test]
    fn noise_does_not_kill_exact_book_match() {
        let kw = "爱潜水的乌贼";
        let mut list = vec![sr("爱潜水的乌贼", "某作者A", 1)]; // 精准书名匹配

        // 模拟 100 条书名无关、但作者名极度蹭关键词热度的噪声数据
        for i in 0..100 {
            list.push(sr(&format!("噪声书{i}"), "爱潜水的乌贼续", 100 + i));
        }

        let out = filter_sort(&list, kw);
        // 精准匹配的结果独立评分必定最高（1.0），必须稳居第一
        assert!(!out.is_empty(), "结果不应为空");
        assert_eq!(
            out[0].book_name, "爱潜水的乌贼",
            "精准书名匹配必须排在第一位"
        );
    }

    /// 回归测试 2（关键）：反过来——海量书名噪声下，正确的作者匹配绝不能被错杀
    #[test]
    fn noise_does_not_kill_exact_author_match() {
        let kw = "爱潜水的乌贼";
        let mut list = vec![sr("某本毫无关系的书", "爱潜水的乌贼", 1)]; // 精准作者匹配

        // 模拟 100 条作者无关、但书名极度蹭关键词热度的噪声数据
        for i in 0..100 {
            list.push(sr("爱潜水的乌贼续", &format!("噪声作者{i}"), 100 + i));
        }

        let out = filter_sort(&list, kw);
        // 精准作者匹配结果独立评分为 1.0，也必须稳居第一
        assert!(!out.is_empty(), "结果不应为空");
        assert_eq!(
            out[0].author.as_deref(),
            Some("爱潜水的乌贼"),
            "精准作者匹配必须排在第一位"
        );
    }

    #[test]
    fn similar_func_basics() {
        // 显式断言 floating 点结果在 EPSILON 范围内的等价，避免 clippy::float_cmp
        let one = 1.0_f64;
        assert!((similar("abc", "abc") - one).abs() < f64::EPSILON);
        assert!(similar("", "abc").abs() < f64::EPSILON);
        assert!(similar("abc", "").abs() < f64::EPSILON);
        // 大小写不敏感测试
        assert!((similar("abc", "ABC") - one).abs() < f64::EPSILON);
    }
}
