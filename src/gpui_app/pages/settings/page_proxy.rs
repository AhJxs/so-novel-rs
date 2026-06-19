//! 代理页（gpui-component `Settings` 左侧 sidebar 第 3 项）。
//!
//! 2 个 group：
//! - HTTP 代理：启用（switch）/ Host（input）/ Port（u16，1–65535）
//! - Cookie：起点 Cookie（input）

use gpui::{App, SharedString};
use gpui_component::setting::{NumberFieldOptions, SettingGroup, SettingItem, SettingPage};

use crate::gpui_app::i18n::ts;

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
            SettingGroup::new()
                .title(ts("Settings.group.cookie"))
                .items(vec![
                    SettingItem::new(
                        ts("Settings.item.qidian_cookie"),
                        string_field(
                            &m,
                            move |model| SharedString::from(model.config.qidian_cookie.clone()),
                            move |model, s| model.config.qidian_cookie = s,
                        ),
                    )
                    .description(ts("Settings.desc.qidian_cookie")),
                ]),
        ])
}
