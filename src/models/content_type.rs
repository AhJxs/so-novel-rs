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
    pub fn attr_name(self) -> Option<&'static str> {
        match self {
            ContentType::AttrSrc => Some("src"),
            ContentType::AttrHref => Some("href"),
            ContentType::AttrContent => Some("content"),
            ContentType::AttrValue => Some("value"),
            _ => None,
        }
    }
}
