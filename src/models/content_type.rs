//! 选择器取值的类型标记：文本、HTML 或某个属性。

use serde::{Deserialize, Serialize};

/// 选择器命中的内容类型。对应 Java `model.ContentType`。
#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum ContentType {
    /// 节点文本。
    Text,
    /// 节点内部 HTML。
    Html,
    /// `src` 属性（图片、脚本等）。
    AttrSrc,
    /// `href` 属性（链接）。
    AttrHref,
    /// `content` 属性（多为 meta 标签）。
    AttrContent,
    /// `value` 属性（表单控件）。
    AttrValue,
}

impl ContentType {
    /// 返回该 `ContentType` 关联的 HTML 属性名。
    ///
    /// 非 attr 变体（Text / Html）返回 `""`，调用方需要先用外层 `match` 过滤
    /// 到 attr 变体再使用 —— 这里用空串而不是 `Option` 是为了避免在
    /// `extract_from_elements` 等已知变体子集的上下文中触发 `clippy::expect_used`。
    #[must_use]
    pub const fn attr_name(self) -> &'static str {
        match self {
            Self::AttrSrc => "src",
            Self::AttrHref => "href",
            Self::AttrContent => "content",
            Self::AttrValue => "value",
            _ => "",
        }
    }
}
