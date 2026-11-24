//! 关于页（`Settings` 左侧 sidebar 第 4 项）：版本 / 检查更新与自更新 / 项目主页。
//!
//! 「检查更新」一个 item 承载完整状态机：`Idle`（检查 / 下载）、`Downloading`（进度 + 取消）、
//! `Ready`（重启确认）、`Installing`、`Failed`（错误 + 重试）。状态都在 `AppModel::update_state`，
//! 这里只读 + 派发动作，离开设置页下载也会继续。

use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::AlertDialog;
use gpui_kit::component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};
use gpui_kit::component::{
    ActiveTheme as _, Disableable, Icon, IconName, Sizable as _, WindowExt as _,
};
use gpui_kit::{App, ClickEvent, IntoElement, ParentElement, Styled, Window, div};
use rust_i18n::t;

use super::ctx::{PageContext, RestartHandler};
use crate::desktop::model::DownloadPhase;
use crate::utils::fs::format_size;

const RELEASES_URL: &str = "https://github.com/AhJxs/so-novel-rs/releases/latest";

pub(super) fn build(ctx: &PageContext<'_>, _cx: &App) -> SettingPage {
    let m = ctx.model.clone();
    let restart = ctx.restart_handler.cloned();

    SettingPage::new(t!("Settings.page.about"))
        .resettable(false)
        .default_open(true)
        .groups(vec![
            SettingGroup::new()
                .title(t!("Settings.group.info"))
                .items(vec![
                    // 版本（静态文本）
                    SettingItem::new(
                        t!("Settings.item.version"),
                        SettingField::render(|_opts, _window, cx| {
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("v{}", env!("CARGO_PKG_VERSION")))
                        }),
                    )
                    .description(t!("Settings.desc.version").to_string()),
                    // 检查更新 / 下载 / 安装 / 重启
                    SettingItem::new(
                        t!("Settings.item.check_update"),
                        SettingField::render({
                            let m = m;
                            let restart = restart;
                            move |_opts, _window, cx| render_update_row(&m, restart.as_ref(), cx)
                        }),
                    )
                    .description(t!("Settings.desc.check_update").to_string()),
                    // 项目主页
                    SettingItem::new(
                        t!("Settings.item.open_github"),
                        SettingField::render(|_opts, _window, _cx| {
                            Button::new("open-github")
                                .icon(Icon::new(IconName::Globe))
                                .label(t!("Settings.open_github_button"))
                                .small()
                                .on_click(|_ev, _window, cx| {
                                    cx.open_url("https://github.com/AhJxs/so-novel-rs");
                                })
                        }),
                    )
                    .description(t!("Settings.desc.open_github").to_string()),
                ]),
        ])
}

/// 「检查更新」这一行：按 `DownloadPhase` 切换按钮 / 进度条 / 重启确认。
fn render_update_row(
    m: &gpui_kit::Entity<crate::desktop::model::AppModel>,
    restart: Option<&RestartHandler>,
    cx: &App,
) -> gpui_kit::AnyElement {
    let state = m.read(cx);
    let update = &state.update_state;
    let checking = update.checking;
    let phase = update.download.phase.clone();
    let has_new_version = update.is_new_version_available();
    let version = update
        .latest_version
        .as_deref()
        .map(|v| v.trim_start_matches('v').to_string())
        .unwrap_or_default();
    let has_asset = update.has_downloadable_asset();
    let cancelling = update.download.cancelling;

    match phase {
        DownloadPhase::Downloading => {
            let m_cancel = m.clone();
            let received = update.download.received;
            let total = update.download.total;
            // 紧凑写法：`8.0 KB / 24.0 MB`；大小未知时只显示已下载。
            let text = if total > 0 {
                format!("{} / {}", format_size(received), format_size(total))
            } else {
                format_size(received)
            };
            return div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(text),
                )
                .child(
                    Button::new("update-cancel")
                        .label(if cancelling {
                            t!("Settings.update_cancelling_button")
                        } else {
                            t!("Settings.update_cancel_button")
                        })
                        .small()
                        .disabled(cancelling)
                        .on_click(move |_ev, _window, cx| {
                            m_cancel.update(cx, |model, _| model.cancel_update_download());
                        }),
                )
                .into_any_element();
        }
        DownloadPhase::Ready => {
            let restart = restart.cloned();
            let m_not_ready = m.clone();
            return Button::new("update-restart")
                .icon(Icon::new(IconName::RefreshCw))
                .label(t!("Settings.update_restart_button"))
                .small()
                .on_click(move |_ev, window, cx| {
                    let Some(restart) = restart.as_ref() else {
                        // 理论上 RootView 一定会注入；走到这里说明装配错了，别让按钮静默失效。
                        m_not_ready.update(cx, |model, _| {
                            model.push_error(t!("Toasts.update_not_ready").to_string());
                        });
                        return;
                    };
                    open_restart_confirm(window, cx, &version, restart.clone());
                })
                .into_any_element();
        }
        DownloadPhase::Installing => {
            return Button::new("update-installing")
                .icon(Icon::new(IconName::Loader))
                .label(t!("Settings.update_installing_button"))
                .small()
                .loading(true)
                .disabled(true)
                .into_any_element();
        }
        DownloadPhase::Failed(err) => {
            let m_retry = m.clone();
            return div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(t!("Settings.update_failed_text", err = &err).to_string()),
                )
                .child(download_button(m_retry, &version, true))
                .into_any_element();
        }
        DownloadPhase::Idle => {}
    }

    if checking {
        return Button::new("check-update")
            .icon(Icon::new(IconName::Loader))
            .label(t!("Settings.check_update_button"))
            .small()
            .disabled(true)
            .loading(true)
            .into_any_element();
    }

    if has_new_version && has_asset {
        return download_button(m.clone(), &version, false);
    }

    if has_new_version {
        // 该平台没有发布包（如 Intel macOS）：保留跳浏览器。
        return Button::new("update-open-release")
            .icon(Icon::new(IconName::ExternalLink))
            .label(t!("Settings.download_new_version_button", ver = &version))
            .small()
            .on_click(|_ev, _window, cx| cx.open_url(RELEASES_URL))
            .into_any_element();
    }

    let m_check = m.clone();
    Button::new("check-update")
        .icon(Icon::new(IconName::Loader))
        .label(t!("Settings.check_update_button"))
        .small()
        .on_click(move |_ev, _window, cx| {
            m_check.update(cx, |model, _| model.spawn_update_check());
        })
        .into_any_element()
}

fn download_button(
    m: gpui_kit::Entity<crate::desktop::model::AppModel>,
    version: &str,
    retry: bool,
) -> gpui_kit::AnyElement {
    let label = if retry {
        t!("Settings.update_retry_button").to_string()
    } else {
        t!("Settings.download_new_version_button", ver = version).to_string()
    };
    Button::new("update-download")
        .icon(Icon::new(IconName::ArrowDown))
        .label(label)
        .small()
        .on_click(move |_ev, _window, cx| {
            m.update(cx, |model, _| model.spawn_update_download());
        })
        .into_any_element()
}

fn open_restart_confirm(window: &mut Window, cx: &mut App, version: &str, restart: RestartHandler) {
    let version = version.to_string();
    window.open_alert_dialog(cx, move |alert: AlertDialog, _window, _cx| {
        let version = version.clone();
        let restart = restart.clone();
        alert
            .title(t!("Settings.update_dialog.title"))
            .description(t!("Settings.update_dialog.message", ver = &version))
            .ok_text(t!("Settings.update_dialog.confirm_button"))
            .cancel_text(t!("Settings.update_dialog.cancel_button"))
            .confirm()
            .on_ok(move |_ev: &ClickEvent, window, cx| {
                // 安装 + 重启收敛在 RootView：先换二进制、再关库、再拉新进程。
                // 失败时 RootView 已推 notification，这里只关弹窗、停在 Failed 状态。
                restart(window, cx);
                true
            })
    });
}
