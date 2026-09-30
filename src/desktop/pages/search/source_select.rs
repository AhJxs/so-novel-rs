//! 选书源下拉的自定义 `SelectItem`。
//!
//! 必须自定义：组件内置的 `SelectItem` impl（`String` / `SharedString` / `&'static str`）
//! 强制 `value() == title()`，没法让 value 是 `"rule:1"`、title 是 `"起点 (ZH_CN)"`。
//!
//! - `value`: `"all"` = 聚合搜索（= `None`），`"rule:{id}"` = 单源。
//! - `title`: 用户可见文本（聚合标题或 `format!("{name} ({LANG})")`）。
//! - `Value` = `SharedString`，`Confirm(Some(value))` 的解析逻辑见 `mod.rs` 的订阅。

use gpui_kit::SharedString;
use gpui_kit::component::select::SelectItem;

#[derive(Clone, Debug)]
pub(super) struct SourceSelectItem {
    pub(super) value: SharedString,
    pub(super) title: SharedString,
}

impl SelectItem for SourceSelectItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }
}
