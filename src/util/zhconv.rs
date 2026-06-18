//! 简繁中文转换的薄包装。
//!
//! 底层用 [`zhconv`]（OpenCC + MediaWiki 词表 + Aho-Corasick 匹配，编译期嵌入）。
//! 我们只暴露按目标语言转换的入口 + HTML 标签保护。

use zhconv::{Variant, zhconv};

use crate::config::LangType;

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
        let open_gt_rel = rest[pos..]
            .find('>')
            .expect("script/style 开标签必有 '>'");
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
        assert!(out.contains(r#"var x = "不转这里";"#), "script mutated: {out}");
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
        assert!(matches!(lang_to_variant(&LangType::ZhHant), Variant::ZhHant));
    }
}
