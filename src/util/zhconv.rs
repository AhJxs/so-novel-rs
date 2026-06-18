//! 简繁中文转换的薄包装。
//!
//! 底层用 [`zhconv`]（OpenCC + MediaWiki 词表 + Aho-Corasick 匹配，编译期嵌入）。
//! 我们只暴露按目标语言转换的入口 + HTML 标签保护。

use zhconv::{Variant, zhconv};

use crate::config::LangType;
use crate::models::Book;

/// 把 `LangType` 映射到 `zhconv` 的目标变体。
///
/// zhconv 会基于文本内容自动判断源（简/繁），无需我们传源；只决定目标。
/// LangType::ZhCn → ZhHans（简体）；ZhTw → ZhTW（台湾繁体，含用词差异，如"软体"）；
/// ZhHant → ZhHant（标准繁体）。
pub fn lang_to_variant(target: &LangType) -> Variant {
    match target {
        LangType::ZhCn => Variant::ZhHans,
        LangType::ZhTw => Variant::ZhTW,
        LangType::ZhHant => Variant::ZhHant,
    }
}

/// 直接对纯文本调用 zhconv。TXT body 用这个。
pub fn convert_text(text: &str, target: &LangType) -> String {
    zhconv(text, lang_to_variant(target))
}

/// 转换书籍元信息（书名 / 作者 / 简介）到目标语言。
///
/// 与 `maybe_convert_chinese` 同语义：source 解析失败 或 source == target 时
/// 直接 clone 原 book 返回（保守、不误转）。其余情况下：
/// - `book_name` / `author` 走 `convert_text`（纯文本，规则里按 TEXT 模式抽）；
/// - `intro` 走 `convert_html_body`（保留 `<script>`/`<style>` 块，对纯文本也安全——
///   zhconv 不改 ASCII，无 script/style 时整串即整串转）；
/// - 其它字段（category / cover_url / latest_chapter / last_update_time / status）含
///   非中文内容（URL、状态枚举、时间戳）多，原样保留。
///
/// 返回新 `Book`（克隆 + 转换字段），不修改入参。
pub fn convert_book_meta(book: &Book, source_lang_raw: &str, target: &LangType) -> Book {
    let Some(source) = LangType::parse(source_lang_raw) else {
        return book.clone();
    };
    if source == *target {
        return book.clone();
    }
    let mut out = book.clone();
    out.book_name = convert_text(&book.book_name, target);
    out.author = convert_text(&book.author, target);
    if let Some(intro) = book.intro.as_deref() {
        out.intro = Some(convert_html_body(intro, target));
    }
    out
}

/// 对 HTML body 转换中文，**跳过 `<script>` 和 `<style>` 块**（代码块里中文不该被转）。
///
/// 其他部分（标签外文本、属性值中的中文、HTML 实体）直接走 zhconv：
/// - ASCII 字符（标签、实体名、URL）不会被改 → 标签结构稳定；
/// - 属性值中的中文会跟着目标变体转（符合"用户想要目标语言"预期）；
/// - 唯一可能误转的是 `<a title="简体中文">` 这种属性里的中文 —— 也符合预期。
///
/// 局限：HTML 中出现 `<script` / `<style` 在属性值 / CDATA 内的极端情况会把
/// 切分搞错；正常书源 HTML 不会这样。
pub fn convert_html_body(body: &str, target: &LangType) -> String {
    const SCRIPT: &str = "<script";
    const STYLE: &str = "<style";
    const SCRIPT_END: &str = "</script";
    const STYLE_END: &str = "</style";

    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    loop {
        // case-insensitive 找下一个 <script / <style
        let lower = rest.to_ascii_lowercase();
        let script_pos = lower.find(SCRIPT);
        let style_pos = lower.find(STYLE);
        let block_pos = match (script_pos, style_pos) {
            (Some(s), Some(t)) => Some(s.min(t)),
            (Some(s), None) => Some(s),
            (None, Some(t)) => Some(t),
            (None, None) => None,
        };
        let Some(pos) = block_pos else {
            out.push_str(&convert_text(rest, target));
            break;
        };
        // 切分前：把 [..pos) 转
        out.push_str(&convert_text(&rest[..pos], target));
        // 找到块结束标签
        let is_script = lower[pos..].starts_with(SCRIPT);
        let end_tag = if is_script { SCRIPT_END } else { STYLE_END };
        let Some(rel_close) = lower[pos..].find(end_tag) else {
            // 找不到闭合 → 把剩下的原样追加（保险），不尝试转换
            out.push_str(&rest[pos..]);
            break;
        };
        // 闭标签 </script 含 '>'，位置 = rel_close + end_tag.len() + 1
        let block_end_incl = pos + rel_close + end_tag.len() + 1;
        // 找到开标签的 '>' 结束
        let open_gt_rel = rest[pos..].find('>').expect("script/style 开标签必有 '>'");
        let open_gt = pos + open_gt_rel + 1;
        // 开标签原样（zhconv 不改 ASCII 字符，但保险起见原样保留）
        out.push_str(&rest[pos..open_gt]);
        // 块内容 + 闭标签原样保留（不转）
        out.push_str(&rest[open_gt..block_end_incl]);
        // 移动到块之后继续
        rest = &rest[block_end_incl..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_text_simplified_to_traditional_tw() {
        // 软体 vs 软件：台湾用"软体"
        let out = convert_text("头发的颜色是黄色", &LangType::ZhTw);
        assert!(out.contains("頭髮"), "got: {out}");
        assert!(out.contains("黃色"), "got: {out}");
        // 简体字应已不存在
        assert!(!out.contains("发"));
        assert!(!out.contains("黄"));
    }

    #[test]
    fn convert_text_traditional_to_simplified() {
        let out = convert_text("頭髮的顏色是黃色", &LangType::ZhCn);
        assert_eq!(out, "头发的颜色是黄色");
    }

    #[test]
    fn convert_html_body_preserves_tags_and_skips_script() {
        let body =
            r#"<p class="cls">简体中文 测试</p><script>var x = "不转这里";</script><p>末段</p>"#;
        let out = convert_html_body(body, &LangType::ZhTw);
        // 标签完整保留
        assert!(out.contains(r#"<p class="cls">"#), "tag broken: {out}");
        assert!(out.contains("</p>"), "got: {out}");
        // 标签外文本转繁体
        assert!(out.contains("簡體中文"), "got: {out}");
        // script 块原样
        assert!(
            out.contains(r#"var x = "不转这里";"#),
            "script mutated: {out}"
        );
        assert!(out.contains("</script>"), "got: {out}");
        // 末段也转
        assert!(out.contains("末段"), "got: {out}");
    }

    #[test]
    fn convert_html_body_no_script_no_style() {
        let body = "<p>简体中文</p>";
        let out = convert_html_body(body, &LangType::ZhHant);
        assert!(out.contains("簡體中文"), "got: {out}");
    }

    #[test]
    fn lang_to_variant_mapping() {
        assert!(matches!(lang_to_variant(&LangType::ZhCn), Variant::ZhHans));
        assert!(matches!(lang_to_variant(&LangType::ZhTw), Variant::ZhTW));
        assert!(matches!(
            lang_to_variant(&LangType::ZhHant),
            Variant::ZhHant
        ));
    }

    // ---------- convert_book_meta ----------

    fn sample_book_cn() -> Book {
        Book {
            url: "https://x".into(),
            book_name: "软件工程师的发量".into(),
            author: "苹果".into(),
            intro: Some("<p>头发的颜色是黄色</p><script>var s=\"不转\";</script>".into()),
            ..Book::default()
        }
    }

    /// 源 zh_CN + 目标 zh_TW：书名 / 作者 / 简介 全部转换，简介的 script 块原样保留。
    ///
    /// 注：zhconv 是字符级映射，"发" → "發"（不是"髮"）；后者是上下文感知结果，
    /// OpenCC 词表里有但 zhconv 默认不启用。本测试只断言字符级转换结果。
    #[test]
    fn convert_book_meta_simplified_to_traditional_tw() {
        let book = sample_book_cn();
        let out = convert_book_meta(&book, "zh_CN", &LangType::ZhTw);
        // 书名：简体"软件"→ 台湾繁体"軟體"（用词差异 + 字形）；"发" → "發"
        assert_eq!(out.book_name, "軟體工程師的發量");
        // 作者纯字面转
        assert_eq!(out.author, "蘋果");
        // 简介：script 块不动，其它转繁体
        let intro = out.intro.as_deref().unwrap();
        assert!(intro.contains("頭髮的顏色是黃色"), "intro: {intro}");
        assert!(
            intro.contains(r#"var s="不转";"#),
            "script mutated: {intro}"
        );
        // 其它字段原样保留
        assert_eq!(out.url, book.url);
        assert_eq!(out.category, book.category);
    }

    /// 源 zh_TW + 目标 zh_CN：繁体转简体（含"軟體"→"软体"）。
    #[test]
    fn convert_book_meta_traditional_to_simplified() {
        let book = Book {
            book_name: "軟體工程師".into(),
            author: "蘋果".into(),
            intro: Some("<p>頭髮的顏色</p>".into()),
            ..sample_book_cn()
        };
        let out = convert_book_meta(&book, "zh_TW", &LangType::ZhCn);
        assert_eq!(out.book_name, "软体工程师");
        assert_eq!(out.author, "苹果");
        assert_eq!(out.intro.as_deref().unwrap(), "<p>头发的颜色</p>");
    }

    /// source == target：跳过转换、返回 clone（与 `maybe_convert_chinese` 同语义）。
    #[test]
    fn convert_book_meta_skips_when_source_equals_target() {
        let book = sample_book_cn();
        let out = convert_book_meta(&book, "zh_CN", &LangType::ZhCn);
        assert_eq!(out.book_name, book.book_name);
        assert_eq!(out.author, book.author);
        assert_eq!(out.intro, book.intro);
    }

    /// source 解析失败：保守、跳过转换。
    #[test]
    fn convert_book_meta_skips_when_source_unparseable() {
        let book = sample_book_cn();
        let out = convert_book_meta(&book, "garbage_lang", &LangType::ZhCn);
        assert_eq!(out.book_name, book.book_name);
        assert_eq!(out.author, book.author);
        assert_eq!(out.intro, book.intro);
    }

    /// intro 为 None 时不动（不能 panic on None）。
    #[test]
    fn convert_book_meta_handles_none_intro() {
        let book = Book {
            book_name: "软件".into(),
            author: "苹果".into(),
            intro: None,
            ..sample_book_cn()
        };
        let out = convert_book_meta(&book, "zh_CN", &LangType::ZhTw);
        assert_eq!(out.book_name, "軟體");
        assert!(out.intro.is_none());
    }
}
