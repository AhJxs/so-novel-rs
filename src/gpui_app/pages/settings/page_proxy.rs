//! 代理页（gpui-component `Settings` 左侧 sidebar 第 3 项）。
//!
//! 2 个 group：
//! - HTTP 代理：启用（switch）/ Host（input）/ Port（u16，1–65535）
//! - Cookie：起点 Cookie（**多行 textarea + placeholder** —— 详见 page 注释）

use gpui::{App, SharedString, Styled, px};
use gpui_component::{
    AxisExt, Sizable,
    input::Input,
    setting::{NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage},
};

use crate::i18n::ts;

use super::ctx::PageCtx;
use super::fields::{bool_field, number_field_u16, string_field};

pub(super) fn build(ctx: &PageCtx<'_>, _cx: &App) -> SettingPage {
    let m = ctx.model.clone();

    SettingPage::new(ts("Settings.page.proxy"))
        .resettable(false)
        .default_open(true)
        .groups(vec![
            // ============ HTTP 代理 ============
            SettingGroup::new()
                .title(ts("Settings.group.http_proxy"))
                .items(vec![
                    SettingItem::new(
                        ts("Settings.item.proxy_enabled"),
                        bool_field(
                            &m,
                            move |model| model.config.proxy_enabled,
                            move |model, val| model.config.proxy_enabled = val,
                        ),
                    )
                    .description(ts("Settings.desc.proxy_enabled")),
                    SettingItem::new(
                        ts("Settings.item.proxy_host"),
                        string_field(
                            &m,
                            move |model| SharedString::from(model.config.proxy_host.clone()),
                            move |model, s| model.config.proxy_host = s,
                        ),
                    )
                    .description(ts("Settings.desc.proxy_host")),
                    SettingItem::new(
                        ts("Settings.item.proxy_port"),
                        number_field_u16(
                            &m,
                            NumberFieldOptions {
                                min: 1.0,
                                max: 65_535.0,
                                ..Default::default()
                            },
                            move |model| model.config.proxy_port,
                            move |model, v| model.config.proxy_port = v,
                        ),
                    )
                    .description(ts("Settings.desc.proxy_port")),
                ]),
            // ============ Cookie ============
            // 起点 cookie 必须**多行 textarea** —— `Cookie:` 头是多对 `k=v; k=v`
            // 拼起来的整段，单行 input 既放不下又看不到全貌。gpui-component 的
            // `SettingField::input` 只支持单行 Input，改走 `SettingField::render`
            // 挂 owner-cached 的 `InputState`（详见 `SettingsPage::new`）。
            // `Input::h(px(80.))` 给 3 行高度（`InputState::rows(3)` + 内置 padding），
            // 用户可在框内自由换行 / 全选粘贴。
            SettingGroup::new()
                .title(ts("Settings.group.cookie"))
                .items(vec![
                    SettingItem::new(
                        ts("Settings.item.qidian_cookie"),
                        SettingField::render({
                            let qidian_cookie_input = ctx.qidian_cookie_input.clone();
                            move |options, _window, _cx| {
                                let mut el = Input::new(&qidian_cookie_input)
                                    .with_size(options.size)
                                    .h(px(80.));
                                // horizontal layout → 固定 256px；其它 → 占满整行。
                                // 宽度逻辑和 download_path 保持一致 —— 见 page_general.rs
                                // download_path 设置项注释。
                                if options.layout.is_horizontal() {
                                    el = el.w_64();
                                } else {
                                    el = el.w_full();
                                }
                                el
                            }
                        }),
                    )
                    .description(ts("Settings.desc.qidian_cookie")),
                ]),
        ])
}
