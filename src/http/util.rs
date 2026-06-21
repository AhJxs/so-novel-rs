//! 杂项工具。对应 Java `util.CrawlUtils` 中除 `hasCf`/`request` 之外的部分。

use once_cell::sync::Lazy;
use rand::RngExt;
use regex::Regex;

use crate::config::AppConfig;
use crate::rules::EffectiveCrawl;

/// 解析 hutool 风格的"宽松 JSON"字符串到键值对列表。
///
/// 规则文件里 `Search.data` 长这样：
/// ```text
/// {searchkey: %s, searchtype: all}
/// ```
/// 注意：键和字符串值都没引号，是 hutool `JSONUtil.parseObj` 接受但
/// `serde_json::from_str` 不接受的形式。这里用一个简单的正则做提取。
///
/// 调用时按出现顺序把每个值里的 `%s` 替换成 `args` 中下一个元素。
/// 与 Java 端 `CrawlUtils#buildData` 行为一致。
pub fn build_form_data(template: &str, args: &[&str]) -> Vec<(String, String)> {
    static KV: Lazy<Regex> = Lazy::new(|| {
        // 形如 `key: value`，value 直到下一个 `,` 或 `}`。
        // 容忍 key/value 两侧可选的引号，以及 value 内部的空白。
        Regex::new(r#"([\w\-]+)\s*:\s*("([^"]*)"|'([^']*)'|([^,}]*))"#).unwrap()
    });

    let mut arg_iter = args.iter();
    let mut out = Vec::new();
    for cap in KV.captures_iter(template) {
        let key = cap.get(1).unwrap().as_str().trim().to_string();
        let raw = cap
            .get(3)
            .or_else(|| cap.get(4))
            .or_else(|| cap.get(5))
            .map(|m| m.as_str().trim())
            .unwrap_or("");
        let value = if raw == "%s" {
            arg_iter
                .next()
                .map(|s| (*s).to_string())
                .unwrap_or_default()
        } else {
            raw.to_string()
        };
        out.push((key, value));
    }
    out
}

/// 把 GET 搜索 url 里的 `%s` 占位符替换成关键字。
///
/// 不直接用 `String::replace`，因为关键字可能含 `%`、`#` 等需要 URL 编码的字符。
/// 规则模板里 `%s` 习惯写在 query string 里，这里对值做最小限度的 url-encode。
pub fn format_url_query(url_template: &str, keyword: &str) -> String {
    let encoded = url_encode_query_value(keyword);
    url_template.replacen("%s", &encoded, 1)
}

fn url_encode_query_value(s: &str) -> String {
    // 与 Java URLUtil.encode 兼容的子集：保留 `A-Za-z0-9-_.~`，其余按 UTF-8 字节
    // 转 `%XX`。空格转 `+` 与 `%20` 都被多数书源接受，这里选 `%20`，因为
    // 规则里 GET 模板大多用空格直写。
    const HEX: &[u8] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            _ => {
                out.push('%');
                out.push(HEX[(*b >> 4) as usize] as char);
                out.push(HEX[(*b & 0x0F) as usize] as char);
            }
        }
    }
    out
}

/// 随机抓取间隔（毫秒）。对应 Java `CrawlUtils.randomInterval(config, false)`。
pub fn random_interval_ms(eff: &EffectiveCrawl) -> u64 {
    let lo = eff.min_interval_ms as u64;
    let hi = eff.max_interval_ms.max(eff.min_interval_ms + 1) as u64;
    rand::rng().random_range(lo..hi)
}

/// 重试随机间隔（毫秒）。对应 Java `CrawlUtils.randomInterval(config, true)`。
pub fn random_retry_interval_ms(eff: &EffectiveCrawl) -> u64 {
    let lo = eff.retry_min_interval_ms as u64;
    let hi = eff.retry_max_interval_ms.max(eff.retry_min_interval_ms + 1) as u64;
    rand::rng().random_range(lo..hi)
}

/// 也提供一个直接吃 `AppConfig` 的版本，便于不需要 EffectiveCrawl 的调用方。
pub fn random_interval_from_cfg(cfg: &AppConfig) -> u64 {
    let lo = cfg.min_interval as u64;
    let hi = cfg.max_interval.max(cfg.min_interval + 1) as u64;
    rand::rng().random_range(lo..hi)
}

/// 清理不可见字符：控制字符、格式控制符、PUA、零宽字符等
/// （这些往往是页面反爬手段，留在文本里会导致中文显示错乱）。
///
/// 对应 Java `util.CrawlUtils.cleanInvisibleChars`。Rust 端不能直接套
/// `[\\p{C}...]` 正则（regex crate 默认禁用 Unicode 大类），改为白名单字符。
pub fn clean_invisible_chars(s: &str) -> String {
    s.chars()
        .filter(|c| {
            // 保留：换行符、制表符、回车（这些是正常排版的一部分）
            if *c == '\n' || *c == '\r' || *c == '\t' {
                return true;
            }
            // 移除：所有控制字符（包含 C0 与 C1 控制区）
            if c.is_control() {
                return false;
            }
            // 显式黑名单：零宽空格、字节序标记、行/段分隔
            matches!(
                *c,
                '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2028}' | '\u{2029}' | '\u{FEFF}'
            )
            .not_then()
        })
        .collect()
}

// 给 bool 一个语义化的"取反返回"小工具，让 filter 闭包更清楚。
trait BoolExt {
    fn not_then(self) -> bool;
}
impl BoolExt for bool {
    #[inline]
    fn not_then(self) -> bool {
        !self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_form_data_basic() {
        let kv = build_form_data("{searchkey: %s, searchtype: all}", &["三体"]);
        assert_eq!(
            kv,
            vec![
                ("searchkey".to_string(), "三体".to_string()),
                ("searchtype".to_string(), "all".to_string()),
            ]
        );
    }

    #[test]
    fn build_form_data_two_placeholders() {
        // 含两个 %s 占位符的 data 模板，顺序消费保持稳定。
        let kv = build_form_data("{a: %s, b: %s}", &["x", "y"]);
        assert_eq!(
            kv,
            vec![
                ("a".to_string(), "x".to_string()),
                ("b".to_string(), "y".to_string()),
            ]
        );
    }

    #[test]
    fn build_form_data_handles_quoted_values() {
        let kv = build_form_data(r#"{Submit: "Search", searchKey: %s}"#, &["三体"]);
        assert_eq!(
            kv,
            vec![
                ("Submit".to_string(), "Search".to_string()),
                ("searchKey".to_string(), "三体".to_string()),
            ]
        );
    }

    #[test]
    fn build_form_data_real_69shuba() {
        // proxy-required.json 第一条
        let kv = build_form_data("{submit: Search, searchKey: %s}", &["三体"]);
        assert_eq!(kv.len(), 2);
        assert_eq!(kv[0].0, "submit");
        assert_eq!(kv[0].1, "Search");
        assert_eq!(kv[1].0, "searchKey");
        assert_eq!(kv[1].1, "三体");
    }

    #[test]
    fn format_url_query_encodes_chinese() {
        let url = format_url_query("https://www.sososhu.com/?q=%s&site=xtxxs", "天才");
        assert!(url.contains("q=%E5%A4%A9%E6%89%8D"), "got {url}");
    }

    #[test]
    fn random_interval_within_bounds() {
        let cfg = AppConfig::default();
        for _ in 0..100 {
            let v = random_interval_from_cfg(&cfg);
            assert!((cfg.min_interval as u64..cfg.max_interval as u64).contains(&v));
        }
    }

    #[test]
    fn clean_invisible_chars_keeps_chinese_and_basic_whitespace() {
        let raw = "中\u{200B}文\n第\u{FEFF}1章\t正文";
        let cleaned = clean_invisible_chars(raw);
        assert_eq!(cleaned, "中文\n第1章\t正文");
    }

    #[test]
    fn clean_invisible_chars_removes_controls() {
        let raw = "abc\u{0001}def\u{007F}";
        assert_eq!(clean_invisible_chars(raw), "abcdef");
    }
}
