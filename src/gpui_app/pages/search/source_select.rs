//! 选书源下拉的自定义 SelectItem。
//!
//! 为什么需要：gpui-component 0.5.1 的内置 `SelectItem` impls（`String` / `SharedString` /
//! `&'static str`）都强制 `value() == title() == self`，无法让 value 是 `"rule:1"`、
//! title 是 `"起点 (ZH_CN)"`。手写小 struct 是最简方案。
//!
//! - `value`: 内部 id —— `"all"` 表示"聚合搜索"（= `None`），`"rule:{id}"` 表示单源。
//! - `title`: 给用户看的文本 —— 聚合搜索时是 `ts("Search.source.aggregate")`；
//!   单源时是 `format!("{name} ({LANG})")`。
//! - `Value` 关联类型 = `SharedString`：`Confirm(Some(value))` 拿到的还是
//!   `SharedString`，解析逻辑 `v == "all" → None;
//!   v.strip_prefix("rule:").and_then(parse) → Some(id)`。

use gpui::SharedString;
use gpui_component::select::SelectItem;

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
    // 默认 `matches` 按 title 匹配 —— 下拉里搜"起点"或"ZH"能筛到 rule。
    // 默认 `render` 显示 title —— 弹层列表项正确。
    // 默认 `display_title` 返回 `None`，折叠态显示 `title()` —— Select 关闭时
    // 显示书源名称（不带语言后缀），行为对。
}
