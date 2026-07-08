//! 详情页封面 URL 处理 + `CoverUpdater` 集成
//!
//! 来自原 `parser/book.rs`, 关注"封面 URL":
//! - 单页抽取时的相对→绝对 URL 转换
//! - `parse_book_detail` 末尾的 `CoverUpdater` 集成 (3 站 fan-out)
//!
//! 主流程 [`parse_book_detail`] 在 [`super::meta`]。

use crate::http::abs_url;
use crate::models::Book;
use crate::parser::dom::ContentType;

/// 抽取 coverUrl 字段, 把相对路径按 `base_url` 拼成绝对 (Java 端 jsoup
/// `absUrl("content")` 会自动做这件事)。
pub(super) fn extract_cover_url(raw: String, base_url: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    abs_url(base_url, &raw).or(Some(raw))
}

/// `CoverUpdater` 触发条件 + 替换判断 (与 Java `BookParser.parse()` line 71 一致)。
///
/// 仅 `!rule.need_proxy` 时跑 —— 代理 IP 会被起点等网站屏蔽, 故代理时不使用
/// 源站封面。`cover_updater::fetch_cover` 内部已 soft-skip, 这里只做替换判断。
///
/// # Returns
///
/// `Some(new_url)` — 拿到新封面, 应替换; `None` — 不替换 (原样保留或新值为空)
pub(super) fn maybe_replace_cover(book: &mut Book, new_cover: String) -> bool {
    if !new_cover.is_empty() && book.cover_url.as_deref() != Some(new_cover.as_str()) {
        book.cover_url = Some(new_cover);
        true
    } else {
        false
    }
}

/// `book.cover_url` 字段对应的 `ContentType`: meta 查询走 attr=content,
/// 其他走 text。等价 Java `BookParser#getContentType`。
///
/// book 模块独享, 不进 `dom::selector` (search/toc/chapter 不需要这层判断)。
pub fn content_type_for(query: &str) -> ContentType {
    if query.trim_start().starts_with("meta[") {
        ContentType::AttrContent
    } else {
        ContentType::Text
    }
}
