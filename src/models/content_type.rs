use serde::{Deserialize, Serialize};

/// 选择器命中的内容类型。对应 Java `model.ContentType`。
#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum ContentType {
    Text,
    Html,
    AttrSrc,
    AttrHref,
    AttrContent,
    AttrValue,
}

impl ContentType {
    /// 返回该 `ContentType` 关联的 HTML 属性名。
    ///
    /// 非 attr 变体（Text / Html）返回 `""`，调用方需要先用外层 `match` 过滤
    /// 到 attr 变体再使用 —— 这里用空串而不是 `Option` 是为了避免在
    /// `extract_from_elements` 等已知变体子集的上下文中触发 `clippy::expect_used`。
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
