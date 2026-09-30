//! 简繁中文转换的薄包装。底层 `zhconv`（`OpenCC` + `MediaWiki` 词表 + Aho-Corasick，
//! 编译期嵌入，按内容自动判断源语言）；这里只暴露按目标语言转换的入口 + HTML 标签保护。

use zhconv::{Variant, zhconv};

use crate::config::LangType;
use crate::models::Book;

/// 把 `LangType` 映射到 `zhconv` 的目标变体：`ZhCn` → `ZhHans`、`ZhTw` → `ZhTW`
/// （台湾繁体，含用词差异如"软体"）、`ZhHant` → ZhHant。源语言由 zhconv 自行判断。
pub const fn lang_to_variant(target: &LangType) -> Variant {
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

/// 转换书籍元信息（书名 / 作者 / 简介）到目标语言，返回新 `Book`，不修改入参。
///
/// source 解析失败或 source == target 时直接 clone 返回（保守、不误转）。`book_name` /
/// `author` 走纯文本转换，`intro` 走 `convert_html_body` 以保留 `<script>`/`<style>`；
/// category / `cover_url` / `latest_chapter` / status 等含大量非中文，原样保留。
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

/// 对 HTML body 转换中文，**跳过 `<script>` / `<style>` 块**（代码里的中文不该被转）。
/// ASCII（标签、实体名、URL）不会被改 → 标签结构稳定；属性值里的中文跟着转，符合预期。
/// 局限：`<script` / `<style` 出现在属性值 / CDATA 内会把切分搞错，正常书源不会这样。
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
        out.push_str(&convert_text(&rest[..pos], target));
        let is_script = lower[pos..].starts_with(SCRIPT);
        let end_tag = if is_script { SCRIPT_END } else { STYLE_END };
        let Some(rel_close) = lower[pos..].find(end_tag) else {
            // 找不到闭合 → 把剩下的原样追加（保险），不尝试转换
            out.push_str(&rest[pos..]);
            break;
        };
        // 闭标签 </script 含 '>'，位置 = rel_close + end_tag.len() + 1
        let block_end_incl = pos + rel_close + end_tag.len() + 1;
        // 开标签 '>' 的位置。理论上 `<script` / `<style` 后必有 `>`，防御性兜底：
        // 找不到就整块原样追加，避免 panic 杀掉整次转换。
        let Some(open_gt_rel) = rest[pos..].find('>') else {
            out.push_str(&rest[pos..]);
            break;
        };
        let open_gt = pos + open_gt_rel + 1;
        out.push_str(&rest[pos..open_gt]);
        out.push_str(&rest[open_gt..block_end_incl]);
        rest = &rest[block_end_incl..];
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn convert_text_simplified_to_traditional_tw() {
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
        assert!(out.contains(r#"<p class="cls">"#), "tag broken: {out}");
        assert!(out.contains("</p>"), "got: {out}");
        assert!(out.contains("簡體中文"), "got: {out}");
        assert!(
            out.contains(r#"var x = "不转这里";"#),
            "script mutated: {out}"
        );
        assert!(out.contains("</script>"), "got: {out}");
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

    /// 源 `zh_CN` + 目标 `zh_TW`：三个字段全转，简介的 script 块原样保留。
    ///
    /// 注：zhconv 是字符级映射，"发" → "發"（不是上下文感知的"髮"，词表里有但默认不启用）。
    #[test]
    fn convert_book_meta_simplified_to_traditional_tw() {
        let book = sample_book_cn();
        let out = convert_book_meta(&book, "zh_CN", &LangType::ZhTw);
        // 书名：简体"软件"→ 台湾繁体"軟體"（用词差异 + 字形）；"发" → "發"
        assert_eq!(out.book_name, "軟體工程師的發量");
        assert_eq!(out.author, "蘋果");
        // 简介：script 块不动，其它转繁体
        let intro = out.intro.as_deref().unwrap();
        assert!(intro.contains("頭髮的顏色是黃色"), "intro: {intro}");
        assert!(
            intro.contains(r#"var s="不转";"#),
            "script mutated: {intro}"
        );
        assert_eq!(out.url, book.url);
        assert_eq!(out.category, book.category);
    }

    /// 源 `zh_TW` + 目标 `zh_CN`：繁体转简体（含"軟體"→"软体"）。
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
