//! 代理页（`Settings` 左侧 sidebar 第 3 项）。
//!
//! 2 个 group：代理（模式下拉 / Host / Port / 系统代理探测结果）与起点 Cookie（多行 textarea）。

use gpui_kit::component::{
    ActiveTheme as _, AxisExt, Sizable as _,
    input::Textarea,
    setting::{NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage},
};
use gpui_kit::{App, Entity, ParentElement, SharedString, Styled, div, px};
use rust_i18n::t;

use crate::config::ProxyMode;
use crate::desktop::model::AppModel;
use crate::http::{AbsentReason, SystemProxy};

use super::ctx::PageCtx;
use super::fields::{dropdown_field, number_field_u16, string_field};

pub(super) fn build(ctx: &PageCtx<'_>, cx: &App) -> SettingPage {
    let m = ctx.model.clone();
    // `build_pages` 每帧被 `SettingsPage::render` 调一次，所以这里的 `disabled` 状态
    // 每帧重算 —— 改完下拉立刻生效（配合 `after_proxy_mode` 的 refresh_windows）。
    let mode = m.read(cx).config.proxy.proxy_mode;
    let system_proxy = m.read(cx).system_proxy.clone();

    let mode_options: Vec<(SharedString, SharedString)> = vec![
        (
            ProxyMode::None.as_str().into(),
            t!("Settings.option.proxy_mode.none").into(),
        ),
        (
            ProxyMode::Manual.as_str().into(),
            t!("Settings.option.proxy_mode.manual").into(),
        ),
        (
            ProxyMode::System.as_str().into(),
            t!("Settings.option.proxy_mode.system").into(),
        ),
    ];

    let mut items = vec![
        SettingItem::new(
            t!("Settings.item.proxy_mode"),
            dropdown_field(
                mode_options,
                &m,
                |model| SharedString::from(model.config.proxy.proxy_mode.as_str()),
                |model, val| model.config.proxy.proxy_mode = ProxyMode::parse(&val),
                Some(after_proxy_mode),
            ),
        )
        .description(t!("Settings.desc.proxy_mode").to_string()),
        SettingItem::new(
            t!("Settings.item.proxy_host"),
            string_field(
                &m,
                move |model| SharedString::from(model.config.proxy.proxy_host.clone()),
                move |model, s| model.config.proxy.proxy_host = s,
            ),
        )
        .description(t!("Settings.desc.proxy_host").to_string())
        .disabled(mode != ProxyMode::Manual),
        SettingItem::new(
            t!("Settings.item.proxy_port"),
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
        .description(t!("Settings.desc.proxy_port").to_string())
        .disabled(mode != ProxyMode::Manual),
    ];

    // 只有选了「使用系统代理」才显示探测结果：它是这个模式唯一的反馈面 ——
    // 探测不到时代理静默退化为直连，不弹对话框、不阻断下载。
    if mode == ProxyMode::System {
        items.push(
            SettingItem::new(
                t!("Settings.item.system_proxy_status"),
                SettingField::render(move |_opts, _window, cx| {
                    let (text, color) = match &system_proxy {
                        SystemProxy::Found(url) => (
                            t!("Settings.proxy_status.detected", url = url.as_str()).to_string(),
                            cx.theme().success,
                        ),
                        SystemProxy::Absent { reason } => (
                            format!(
                                "{}（{}）",
                                t!("Settings.proxy_status.not_detected"),
                                reason_text(*reason)
                            ),
                            cx.theme().muted_foreground,
                        ),
                    };
                    div().text_sm().text_color(color).child(text)
                }),
            )
            .description(t!("Settings.desc.system_proxy_status").to_string()),
        );
    }

    SettingPage::new(t!("Settings.page.proxy"))
        .resettable(false)
        .default_open(true)
        .groups(vec![
            // ============ 代理 ============
            SettingGroup::new()
                .title(t!("Settings.group.proxy"))
                .items(items),
            // ============ Cookie ============
            // 起点 cookie 必须是**多行 textarea**（`Cookie:` 头是一整段多对 `k=v`），
            // 所以走 `SettingField::render` 挂 owner-cached 的 TextareaState。
            SettingGroup::new()
                .title(t!("Settings.group.cookie"))
                .items(vec![
                    SettingItem::new(
                        t!("Settings.item.qidian_cookie"),
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
                    .description(t!("Settings.desc.qidian_cookie").to_string()),
                ]),
        ])
}

/// 代理模式 setter 的副作用。
///
/// `dropdown_field` 内部已经调过 `persist_settings()`（→ `rebuild_proxy`），这里只需要
/// 补两件它做不到的事：刷新 `AppModel.system_proxy` 快照，和强制整页重绘 ——
/// Host / Port 的 `disabled` 与探测结果行都在 `build()` 里算，只有 `SettingsPage::render`
/// 重跑才会更新（`dropdown_field` 只会让 Select 自己重绘，不会带起父级）。
///
/// 用模块内 `fn`（不是闭包）让它能当 `after_set` 的 fn pointer，同
/// `page_general::after_theme_kind`。
fn after_proxy_mode(m: &Entity<AppModel>, cx: &mut App) {
    m.update(cx, |model, _| model.refresh_system_proxy());
    cx.refresh_windows();
}

/// [`AbsentReason`] → 用户可读的一小句「为什么没生效」。
fn reason_text(reason: AbsentReason) -> String {
    match reason {
        AbsentReason::NotEnabled => t!("Settings.proxy_status.reason.not_enabled").to_string(),
        AbsentReason::PacOnly => t!("Settings.proxy_status.reason.pac_only").to_string(),
        AbsentReason::SocksOnly => t!("Settings.proxy_status.reason.socks_only").to_string(),
        AbsentReason::EnvUnset => t!("Settings.proxy_status.reason.env_unset").to_string(),
        AbsentReason::ReadFailed => t!("Settings.proxy_status.reason.read_failed").to_string(),
    }
}
