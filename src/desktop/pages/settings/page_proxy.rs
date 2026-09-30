//! 代理页（`Settings` 左侧 sidebar 第 3 项）。
//!
//! 2 个 group：HTTP 代理（启用 / Host / Port）与起点 Cookie（多行 textarea）。

use gpui_kit::component::{
    AxisExt, Sizable as _,
    input::Textarea,
    setting::{NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage},
};
use gpui_kit::{App, SharedString, Styled, px};

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
                            move |model| model.config.proxy.proxy_enabled,
                            move |model, val| model.config.proxy.proxy_enabled = val,
                        ),
                    )
                    .description(ts("Settings.desc.proxy_enabled")),
                    SettingItem::new(
                        ts("Settings.item.proxy_host"),
                        string_field(
                            &m,
                            move |model| SharedString::from(model.config.proxy.proxy_host.clone()),
                            move |model, s| model.config.proxy.proxy_host = s,
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
                            move |model| model.config.proxy.proxy_port,
                            move |model, v| model.config.proxy.proxy_port = v,
                        ),
                    )
                    .description(ts("Settings.desc.proxy_port")),
                ]),
            // ============ Cookie ============
            // 起点 cookie 必须是**多行 textarea**（`Cookie:` 头是一整段多对 `k=v`），
            // 所以走 `SettingField::render` 挂 owner-cached 的 TextareaState。
            SettingGroup::new()
                .title(ts("Settings.group.cookie"))
                .items(vec![
                    SettingItem::new(
                        ts("Settings.item.qidian_cookie"),
                        SettingField::render({
                            let qidian_cookie_input = ctx.qidian_cookie_input.clone();
                            move |options, _window, _cx| {
                                let mut el = Textarea::new(&qidian_cookie_input)
                                    // 传 `options.size()` 让字号 / 内边距跟同页其它设置项对齐；
                                    // 高度仍由 `.h(px(80.))` 固定。
                                    .with_size(options.size())
                                    .h(px(80.));
                                // horizontal layout → 固定 256px；其它 → 占满整行
                                // （与 dl 设置项一致，见 page_general.rs）。
                                if options.layout().is_horizontal() {
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
