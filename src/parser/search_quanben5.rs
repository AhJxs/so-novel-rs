//! 全本小说网（quanben5）搜索的特殊处理。对应 Java `parse.SearchParserQuanben5`。
//!
//! 与一般书源不同的三点：
//! 1. **URL 含两个 `%s`**：`https://quanben5.com/?...&keywords=%s&b=%s`，
//!    第二个 `%s` 是用 `quanben5.js#getParamB(keyword)` 算出的加密参数；
//! 2. **必须加 `Referer: https://quanben5.com/search.html`**，否则返回空；
//! 3. **响应是 JSONP**：`callback({"content": "<HTML 转义后的字符串>"})`，
//!    需要：剥 `{...}` → 取出 `"content": "..."` → unicode/HTML 实体反转义 →
//!    去 `\r\n\t` 转义 → 去字面 `\/` 与 `\"` → 当 HTML 解析。
//!
//! 我们在 `parser::search::search_one` 入口检测"URL 模板含两个 `%s`"自动派发到这里，
//! 不需要 UI 层做任何配置。其它 quanben5 的字段（result / bookName / author 等）走
//! 同一套 `parse_search_results` 逻辑。

use anyhow::Result;
use reqwest::header::{ACCEPT, COOKIE, REFERER, USER_AGENT};
use reqwest::Client;

use crate::http::ua::random_ua;
use crate::http::{decode_response_bytes, has_cloudflare};
use crate::js::eval_function_returning_string;
use crate::models::{Rule, SearchResult};
use crate::parser::search::{parse_search_results, SearchError};

/// 嵌入的 quanben5.js（Java 端从 classpath:quanben5.js 加载）。
const QUANBEN5_JS: &str = include_str!("../../bundle/web/js/quanben5.js");

const QUANBEN5_REFERER: &str = "https://quanben5.com/search.html";

/// 抓 quanben5 搜索结果。
///
/// `cf_bypass_base` 与一般 search_one 一致：CF 命中且非空时调外部 bypass 服务。
pub async fn search_one_quanben5(
    client: &Client,
    rule: &Rule,
    keyword: &str,
    limit: Option<usize>,
    cf_bypass_base: Option<&str>,
) -> Result<Vec<SearchResult>, SearchError> {
    let s = rule.search.as_ref().ok_or(SearchError::SearchDisabled)?;

    // 1. 算 b 参数
    let b = eval_function_returning_string(QUANBEN5_JS, "getParamB", &[keyword])
        .map_err(|e| SearchError::Http(format!("getParamB JS 失败: {e:#}")))?;

    // 2. 拼 URL（双 %s：先 keyword 再 b；keyword 也用 url-encode 兼容多字节）
    let kw_encoded = url_encode_query_value(keyword);
    let url = s.url.replacen("%s", &kw_encoded, 1).replacen("%s", &b, 1);

    // 3. 发请求（带 Referer、UA、可选 Cookie）
    let cookies = if s.cookies.trim().is_empty() {
        None
    } else {
        Some(s.cookies.as_str())
    };

    let mut builder = client
        .get(&url)
        .header(USER_AGENT, random_ua())
        .header(REFERER, QUANBEN5_REFERER)
        .header(ACCEPT, "application/json,text/javascript,*/*;q=0.8");
    if let Some(c) = cookies {
        builder = builder.header(COOKIE, c);
    }
    if let Some(t) = s.timeout {
        builder = builder.timeout(std::time::Duration::from_secs(t as u64));
    }

    let resp = builder
        .send()
        .await
        .map_err(|e| SearchError::Http(format!("send: {e}")))?;
    let final_url = resp.url().to_string();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| SearchError::Http(format!("read body: {e}")))?;

    let raw_text = decode_response_bytes(&bytes, content_type.as_deref());

    // CF 检测（quanben5 偶尔也有 CF）
    if has_cloudflare(&raw_text) {
        match cf_bypass_base.filter(|s| !s.trim().is_empty()) {
            Some(base) => {
                // 走 bypass：bypass 返回的就是去 CF 后的页面 HTML，
                // 但对 JSONP 接口它通常返回 raw JSONP 字符串本身。
                let bypassed = crate::http::fetch_via_cf_bypass(client, base, &url)
                    .await
                    .map_err(|e| SearchError::Http(format!("cf-bypass: {e:#}")))?;
                return parse_jsonp_and_extract(&bypassed, &final_url, rule, limit);
            }
            None => return Err(SearchError::Cloudflare(final_url)),
        }
    }

    parse_jsonp_and_extract(&raw_text, &final_url, rule, limit)
}

/// 从 JSONP 响应里抽出 HTML 并喂给 `parse_search_results`。
///
/// 行为对齐 Java `SearchParserQuanben5`：先 `UnicodeUtil.toString` 把 `\uXXXX` 还原，
/// 再 `HtmlUtil.unescape` 还原 `&amp;` / `&lt;` 等实体，
/// 然后去掉响应里出现的 `\r` `\n` `\t` `\/` `\"` 字面转义。
fn parse_jsonp_and_extract(
    body: &str,
    base_url: &str,
    rule: &Rule,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, SearchError> {
    let processed = unescape_unicode(body);
    let processed = unescape_html_entities(&processed);
    let processed = processed
        .replace("\\r", "")
        .replace("\\n", "")
        .replace("\\t", "")
        .replace("\\/", "/")
        .replace("\\\"", "'");

    // 找出第一个 `{...}` 对（贪婪到 `}`，与 Java `ReUtil.getGroup0("\\{(.*?)\\}", processedBody)` 同）。
    let json_obj = match extract_first_json_object(&processed) {
        Some(s) => s,
        None => {
            return Err(SearchError::Parse(format!(
                "quanben5 响应未找到 JSON 对象（前 200 字节: {}）",
                truncate_for_log(&processed, 200)
            )))
        }
    };

    // 取 `"content":` 后到下一个 `}` 之间的字符串
    let html = match extract_content_field(json_obj) {
        Some(s) => s,
        None => {
            return Err(SearchError::Parse(
                "quanben5 响应缺少 content 字段".to_string(),
            ))
        }
    };
    // 去除两端引号（Java `StrUtil.strip(content, "\"")`）。我们在前面把 \" 替换成了 '，
    // 实际剩下的可能是 `"...` 或 `'...'`，统一脱一下。
    let html = html
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .to_string();

    parse_search_results(&html, base_url, rule, limit)
}

/// 把 `\uXXXX` 转回字符。其它反斜杠序列保留（后续步骤再处理）。
fn unescape_unicode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 5 < bytes.len() && bytes[i] == b'\\' && bytes[i + 1] == b'u' {
            let hex = &s[i + 2..i + 6];
            if let Ok(code) = u32::from_str_radix(hex, 16) {
                if let Some(c) = char::from_u32(code) {
                    out.push(c);
                    i += 6;
                    continue;
                }
            }
        }
        // 回退原样输出当前字节（已确保 char boundary 时安全）
        let ch_end = next_char_boundary(s, i);
        out.push_str(&s[i..ch_end]);
        i = ch_end;
    }
    out
}

fn next_char_boundary(s: &str, i: usize) -> usize {
    let mut j = i + 1;
    while j < s.len() && !s.is_char_boundary(j) {
        j += 1;
    }
    j
}

/// 简易 HTML 实体反转义。覆盖 `&amp; &lt; &gt; &quot; &#39; &nbsp;` 与
/// `&#NNN;` 数字实体。其它实体原样保留（quanben5 响应里基本就这几种）。
fn unescape_html_entities(s: &str) -> String {
    use once_cell::sync::Lazy;
    use regex::Regex;
    static NUM_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"&#(\d+);").expect("html num entity re"));
    let s = s
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    NUM_RE
        .replace_all(&s, |caps: &regex::Captures<'_>| {
            caps[1]
                .parse::<u32>()
                .ok()
                .and_then(char::from_u32)
                .map(String::from)
                .unwrap_or_else(|| caps[0].to_string())
        })
        .into_owned()
}

/// 从字符串里找出第一个完整的 `{...}` 对（按花括号配对，不是简单贪婪）。
fn extract_first_json_object(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' || b == b'\'' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// 从一个 JSON 对象字面量字符串里抽出 `"content":` 字段的字符串值。
/// 简易实现：找 `"content"` 关键字 → 跳过冒号 → 取到下一个不在嵌套里的 `}`/`,` 前。
/// 我们前面已经把 `\"` 替换成了 `'`，因此 content 值里不会再出现 `"` 转义。
fn extract_content_field(json_obj: &str) -> Option<&str> {
    // 找 `"content"`（也容忍 `'content'`）
    let key_pos = json_obj
        .find("\"content\"")
        .or_else(|| json_obj.find("'content'"))?;
    let after_key = &json_obj[key_pos..];
    let colon = after_key.find(':')?;
    let value_start_rel = colon + 1;
    let value_slice = &after_key[value_start_rel..];

    // 从 value_slice 起，找到第一个不嵌套的 `}` 之前的所有内容
    let bytes = value_slice.as_bytes();
    let mut depth = 0i32;
    let mut end = bytes.len();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                if depth == 0 {
                    end = i;
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    Some(value_slice[..end].trim())
}

/// 与 `http::util::format_url_query` 内的私有函数同。这里复制一份避免改动那边的可见性。
fn url_encode_query_value(s: &str) -> String {
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

fn truncate_for_log(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// 判断 URL 模板是否是"双 `%s`"形式（quanben5 的特征）。
pub fn is_quanben5_pattern(url_template: &str) -> bool {
    url_template.matches("%s").count() >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_double_placeholder() {
        assert!(is_quanben5_pattern(
            "https://quanben5.com/?keywords=%s&b=%s"
        ));
        assert!(!is_quanben5_pattern("https://x/search?q=%s"));
        assert!(!is_quanben5_pattern("https://x/search"));
    }

    #[test]
    fn unescape_unicode_basic() {
        assert_eq!(unescape_unicode(r"中文"), "中文");
        // 保留其它转义字符
        assert_eq!(unescape_unicode(r"\nfoo"), r"\nfoo");
    }

    #[test]
    fn unescape_html_entities_common() {
        assert_eq!(unescape_html_entities("a&amp;b&lt;c"), "a&b<c");
        assert_eq!(unescape_html_entities("&#65;&#20013;"), "A中");
        // 未知实体原样
        assert_eq!(unescape_html_entities("&unknown;"), "&unknown;");
    }

    #[test]
    fn extract_first_json_object_balanced_braces() {
        let s = r#"prefix{"a":1,"b":{"c":2}}suffix"#;
        let obj = extract_first_json_object(s).unwrap();
        assert_eq!(obj, r#"{"a":1,"b":{"c":2}}"#);
    }

    #[test]
    fn extract_content_field_returns_html_string() {
        // 经过我们前面 unescape + replace \" → ' 后的结构
        let obj = r#"{'status':1,'content':'<div class="pic_txt_list"><h3><a href="/n/1/">书</a></h3></div>'}"#;
        let html = extract_content_field(obj).unwrap();
        assert!(html.contains("pic_txt_list"));
        assert!(html.contains("书"));
    }

    #[test]
    fn quanben5_b_param_is_ascii() {
        // 走 boa 跑 quanben5.js
        let b = eval_function_returning_string(QUANBEN5_JS, "getParamB", &["三体"]).unwrap();
        assert!(!b.is_empty());
        assert!(b.bytes().all(|c| c < 128));
    }

    /// 端到端解析（无网络）：构造一个仿真 JSONP 响应，验证整套抽取链路。
    #[test]
    fn end_to_end_parse_jsonp_mock() {
        use crate::config::LangType;
        use crate::rules::apply_default_rule;

        // 仿 JSONP 响应（关键字段：content 是 quanben5 真实结构精简版，含 unicode + 实体）
        // 书 = 书，&amp; 会被还原
        let jsonp = r##"search({"status":1,"content":"<div class=\"pic_txt_list\"><h3><a href=\"/n/sanit/\">三&amp;体</a></h3><p class=\"info\"><span>作者甲</span></p></div><div class=\"pic_txt_list\"><h3><a href=\"/n/abc/\">体体体</a></h3><p class=\"info\"><span>作者乙</span></p></div>"})"##;

        let mut rule: crate::models::Rule = serde_json::from_str(
            r##"{
                "url": "https://quanben5.com/",
                "name": "全本小说网",
                "search": {
                    "url": "https://quanben5.com/?...&keywords=%s&b=%s",
                    "method": "get",
                    "result": ".pic_txt_list",
                    "bookName": "h3 > a",
                    "author": "p.info > span"
                }
            }"##,
        )
        .unwrap();
        rule.id = 99;
        apply_default_rule(&mut rule, LangType::ZhCn);

        let out = parse_jsonp_and_extract(jsonp, "https://quanben5.com/", &rule, None).unwrap();
        assert_eq!(out.len(), 2);
        // 第一条：书名经 unicode 还原 + 实体还原应为 "三&体"
        assert!(
            out[0].book_name.contains("三"),
            "got {:?}",
            out[0].book_name
        );
        assert!(out[0].book_name.contains("体"));
        assert!(out[0].book_name.contains('&'));
        assert_eq!(out[0].author.as_deref(), Some("作者甲"));
        // 相对 href 被解析为绝对
        assert_eq!(out[0].url, "https://quanben5.com/n/sanit/");
        // 第二条
        assert_eq!(out[1].book_name, "体体体");
        assert_eq!(out[1].author.as_deref(), Some("作者乙"));
    }
}
