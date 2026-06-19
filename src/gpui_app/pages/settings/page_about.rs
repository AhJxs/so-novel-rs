//! 关于页（gpui-component `Settings` 左侧 sidebar 第 4 项）。
//!
//! 1 个 group：版本（静态文本）/ 检查更新 / 项目主页。
//!
//! 3 个 item 全部 `SettingField::render` —— 形态各异（裸 div / 带 loading state
//! 切换的 Button / 跳外链的 Button），不抽 helper，详见 plan「不抽的项」。

use gpui::{App, IntoElement, ParentElement, Styled, div};
use gpui_component::{
    ActiveTheme as _, Disableable, Icon, IconName, Sizable as _,
    button::Button,
    setting::{SettingField, SettingGroup, SettingItem, SettingPage},
};

use crate::i18n::{ts, ts_fmt};

use super::ctx::PageCtx;

pub(super) fn build(ctx: &PageCtx<'_>, _cx: &App) -> SettingPage {
    let m = ctx.model.clone();

    SettingPage::new(ts("Settings.page.about"))
        .resettable(false)
        .default_open(true)
        .groups(vec![
            SettingGroup::new()
                .title(ts("Settings.group.info"))
                .items(vec![
                    // -- 版本（静态文本）--
                    SettingItem::new(
                        ts("Settings.item.version"),
                        SettingField::render(|_opts, _window, cx| {
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("v{}", env!("CARGO_PKG_VERSION")))
                        }),
                    )
                    .description(ts("Settings.desc.version")),
                    // -- 检查更新 / 下载新版 --
                    SettingItem::new(
                        ts("Settings.item.check_update"),
                        SettingField::render({
                            let m = m.clone();
                            move |_opts, _window, cx| {
                                // 网络请求在跑时 → Button::loading(true) 自动显示
                                // spinner + 屏蔽 click（gpui-component 0.5.1 button.rs:365：
                                // `!(self.disabled || self.loading) && self.on_click.is_some()`）。
                                let state = m.read(cx);
                                let checking = state.update_state.checking;
                                // 检查完成后若有新版本 → 按钮变"下载新版"跳浏览器。
                                if !checking
                                    && let Some(latest) =
                                        state.update_state.latest_version.as_deref()
                                    && latest.trim_start_matches('v') != env!("CARGO_PKG_VERSION")
                                {
                                    let ver = latest.trim_start_matches('v');
                                    return Button::new("check-update")
                                        .icon(Icon::new(IconName::ExternalLink))
                                        .label(
                                            ts_fmt(
                                                "Settings.download_new_version_button",
                                                &[("ver", ver)],
                                            )
                                            .to_string(),
                                        )
                                        .small()
                                        .on_click(|_ev, _window, cx| {
                                            cx.open_url(
                                            "https://github.com/AhJxs/so-novel-rs/releases/latest",
                                        );
                                        })
                                        .into_any_element();
                                }
                                Button::new("check-update")
                                    .icon(Icon::new(IconName::Loader))
                                    .label(ts("Settings.check_update_button"))
                                    .small()
                                    .disabled(checking)
                                    .loading(checking)
                                    .on_click({
                                        let m = m.clone();
                                        move |_ev, _window, cx| {
                                            m.update(cx, |model, _| {
                                                model.spawn_update_check();
                                            });
                                        }
                                    })
                                    .into_any_element()
                            }
                        }),
                    )
                    .description(ts("Settings.desc.check_update")),
                    // -- 项目主页 --
                    SettingItem::new(
                        ts("Settings.item.open_github"),
                        SettingField::render(|_opts, _window, _cx| {
                            Button::new("open-github")
                                .icon(Icon::new(IconName::Globe))
                                .label(ts("Settings.open_github_button"))
                                .small()
                                .on_click(|_ev, _window, cx| {
                                    cx.open_url("https://github.com/AhJxs/so-novel-rs");
                                })
                        }),
                    )
                    .description(ts("Settings.desc.open_github")),
                ]),
        ])
}
