//! 常规页（`Settings` 左侧 sidebar 第 1 项）：外观 + 网络 + 下载 3 个 group。
//!
//! 外观 = 主题模式 + 按模式条件渲染的主题 item + 语言 + 字号滑块；
//! 网络 = GitHub 代理 / Cloudflare bypass；下载 = 目录 / 默认格式 / TXT 编码 / 章节缓存。

use gpui_kit::component::{
    ActiveTheme as _, AxisExt as _, IconName, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::AlertDialog,
    input::Input,
    select::Select,
    setting::{SettingField, SettingGroup, SettingItem, SettingPage},
    slider::SliderValue,
};
use gpui_kit::{App, Entity, ParentElement, SharedString, Styled, div};
use rust_i18n::t;
use tracing;

use crate::config::ExportFormat;
use crate::config::{Language, ThemeDynMode, ThemeKind};
use crate::desktop::model::AppModel;
use crate::desktop::themes;

use super::ctx::PageCtx;
use super::fields::{
    TXT_ENCODINGS, bool_field, dropdown_field, ext_from_str, ext_value, string_field,
};

/// 构造 Page 1（常规）= 外观 + 网络 + 下载。
pub(super) fn build(ctx: &PageCtx<'_>, cx: &App) -> SettingPage {
    let m = ctx.model.clone();
    let theme_kind = ctx.model.read(cx).config.global.theme_pref.kind;

    // 存到 TOML `[global].language`，value 由 `Language::as_str()` 给出。
    let language_options: Vec<(SharedString, SharedString)> = vec![
        (
            Language::SimplifiedChinese.as_str().into(),
            t!("Settings.option.language.zh_cn").into(),
        ),
        (
            Language::TraditionalChinese.as_str().into(),
            t!("Settings.option.language.zh_tw").into(),
        ),
        (
            Language::English.as_str().into(),
            t!("Settings.option.language.en").into(),
        ),
    ];

    // 主题模式：动态 / 静态（value_str 与 ThemeKind::as_str 一致）。
    let theme_kind_options: Vec<(SharedString, SharedString)> = vec![
        (
            ThemeKind::Dynamic.as_str().into(),
            t!("Settings.option.theme_kind.dynamic").into(),
        ),
        (
            ThemeKind::Static.as_str().into(),
            t!("Settings.option.theme_kind.static").into(),
        ),
    ];

    // 5 种输出格式 → (value_str, label)
    let ext_options: Vec<(SharedString, SharedString)> = vec![
        (ext_value(ExportFormat::Epub).into(), "epub".into()),
        (ext_value(ExportFormat::Txt).into(), "txt".into()),
        (ext_value(ExportFormat::Html).into(), "html".into()),
        (ext_value(ExportFormat::Pdf).into(), "pdf".into()),
        (ext_value(ExportFormat::Markdown).into(), "md".into()),
    ];

    // 7 种常见 TXT 编码 → (value_str, label)
    let encoding_options: Vec<(SharedString, SharedString)> = TXT_ENCODINGS
        .iter()
        .map(|e| ((*e).into(), (*e).into()))
        .collect();

    SettingPage::new(t!("Settings.page.general"))
        .resettable(false)
        .default_open(true)
        .groups(vec![
            // ============ 外观 ============
            SettingGroup::new()
                .title(t!("Settings.group.appearance"))
                .items(
                    vec![
                        SettingItem::new(
                            t!("Settings.item.theme_kind"),
                            dropdown_field(
                                theme_kind_options,
                                &m,
                                move |model| {
                                    SharedString::from(model.config.global.theme_pref.kind.as_str())
                                },
                                move |model, val| {
                                    let kind = ThemeKind::parse(&val);
                                    model.config.global.theme_pref.kind = kind;
                                },
                                Some(after_theme_kind),
                            ),
                        )
                        .description(t!("Settings.desc.theme_kind").to_string()),
                        // 按当前主题模式条件渲染后续 item（theme_mode_items）。
                    ]
                    .into_iter()
                    .chain(theme_mode_items(ctx, theme_kind, &m))
                    .chain(std::iter::once(
                        // -- 界面语言（应用 UI 语言，同时也是下载目标语言）--
                        SettingItem::new(
                            t!("Settings.item.language"),
                            dropdown_field(
                                language_options,
                                &m,
                                move |model| {
                                    SharedString::from(model.config.global.language.as_str())
                                },
                                move |model, val| {
                                    let Some(lang) = Language::parse(&val) else {
                                        tracing::warn!("language setter: 未知语言值 {val}");
                                        return;
                                    };
                                    // 选的就是当前语言 → no-op。
                                    if model.config.global.language == lang {
                                        tracing::info!(
                                            "language setter: 选回当前语言 {lang:?}, no-op"
                                        );
                                        return;
                                    }
                                    model.config.global.language = lang;
                                },
                                Some(after_language),
                            ),
                        )
                        .description(t!("Settings.desc.language").to_string()),
                    ))
                    .chain(std::iter::once(
                        // -- 字号（滑块 12–24px，实时缩放整个 app）--
                        // SliderState 由 `SettingsPage::new` 建一次缓存，右侧标签实时读当前 px。
                        SettingItem::new(
                            t!("Settings.item.font_size"),
                            SettingField::render({
                                let font_size_state = ctx.font_size_state.clone();
                                move |options, _window, cx| {
                                    use gpui_kit::component::slider::Slider;
                                    let n = match font_size_state.read(cx).value() {
                                        SliderValue::Single(v) => v,
                                        SliderValue::Range(_, end) => end,
                                    };
                                    let mut el = div().flex().items_center().gap_2();
                                    el = if options.layout().is_horizontal() {
                                        el.w_64()
                                    } else {
                                        el.w_full()
                                    };
                                    el.child(Slider::new(&font_size_state).horizontal().flex_1())
                                        .child(
                                            div()
                                                .w_6()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(format!("{n:.0}")),
                                        )
                                }
                            }),
                        )
                        .description(t!("Settings.desc.font_size").to_string()),
                    )),
                ),
            // ============ 网络 ============
            SettingGroup::new()
                .title(t!("Settings.group.network"))
                .items(vec![
                    SettingItem::new(
                        t!("Settings.item.gh_proxy"),
                        string_field(
                            &m,
                            move |model| SharedString::from(model.config.global.gh_proxy.clone()),
                            move |model, s| model.config.global.gh_proxy = s,
                        ),
                    )
                    .description(t!("Settings.desc.gh_proxy").to_string()),
                    SettingItem::new(
                        t!("Settings.item.cf_bypass"),
                        string_field(
                            &m,
                            move |model| SharedString::from(model.config.global.cf_bypass.clone()),
                            move |model, s| model.config.global.cf_bypass = s,
                        ),
                    )
                    .description(t!("Settings.desc.cf_bypass").to_string()),
                ]),
            // ============ 下载 ============
            SettingGroup::new()
                .title(t!("Settings.group.download"))
                .items(vec![
                    SettingItem::new(
                        t!("Settings.item.download_path"),
                        SettingField::render({
                            let download_path_input = ctx.download_path_input.clone();
                            let pick_folder_listener = ctx.pick_folder_listener.clone();
                            move |options, _window, _cx| {
                                // 宽度要手动设：不设的话 input 渲染成 0 大小 → text 被裁切、
                                // suffix button 没 hit area → click 不响应。
                                let mut el = Input::new(&download_path_input)
                                    .with_size(options.size())
                                    .suffix({
                                        // click handler 用 owner-cache 的 `pick_folder_listener`
                                        // （render 闭包拿不到 `Context<Self>`，不能现建 `cx.listener`）；
                                        // `Rc` 要包一层闭包才满足 `on_click` 的 `'static`。
                                        let listener = pick_folder_listener.clone();
                                        Button::new("download-path-pick")
                                            .ghost()
                                            .icon(IconName::FolderOpen)
                                            .xsmall()
                                            .on_click(move |ev, window, app| {
                                                listener(ev, window, app);
                                            })
                                    });
                                // horizontal layout → 固定 256px（与 `SettingField::input` 默认一致）；
                                // 其它 → 占满整行。
                                if options.layout().is_horizontal() {
                                    el = el.w_64();
                                } else {
                                    el = el.w_full();
                                }
                                el
                            }
                        }),
                    )
                    .description(t!("Settings.desc.download_path").to_string()),
                    // -- 默认格式 --
                    SettingItem::new(
                        t!("Settings.item.default_format"),
                        dropdown_field(
                            ext_options,
                            &m,
                            move |model| {
                                SharedString::from(ext_value(model.config.download.ext_name))
                            },
                            move |model, val| {
                                let Some(ext) = ext_from_str(&val) else {
                                    return;
                                };
                                model.config.download.ext_name = ext;
                            },
                            None,
                        ),
                    )
                    .description(t!("Settings.desc.default_format").to_string()),
                    // -- TXT 编码 --
                    SettingItem::new(
                        t!("Settings.item.txt_encoding"),
                        dropdown_field(
                            encoding_options,
                            &m,
                            move |model| {
                                SharedString::from(model.config.download.txt_encoding.clone())
                            },
                            move |model, val| {
                                model.config.download.txt_encoding = val.to_string();
                            },
                            None,
                        ),
                    )
                    .description(t!("Settings.desc.txt_encoding").to_string()),
                    // -- 保留章节缓存 --
                    SettingItem::new(
                        t!("Settings.item.preserve_chapter_cache"),
                        bool_field(
                            &m,
                            move |model| model.config.download.preserve_chapter_cache,
                            move |model, val| model.config.download.preserve_chapter_cache = val,
                        ),
                    )
                    .description(t!("Settings.desc.preserve_chapter_cache").to_string()),
                ]),
        ])
}

/// 按主题模式构建条件渲染的主题 item。整 item 显隐（不是返回空 div 占位）：切模式后下一帧
/// `build_pages` 读到新 `kind`，本函数返回不同 item 集。
fn theme_mode_items(ctx: &PageCtx<'_>, kind: ThemeKind, m: &Entity<AppModel>) -> Vec<SettingItem> {
    match kind {
        ThemeKind::Static => vec![
            SettingItem::new(
                t!("Settings.item.theme_static"),
                SettingField::render({
                    let state = ctx.theme_state_static.clone();
                    move |options, _window, _cx| {
                        let mut el = Select::new(&state).with_size(options.size()).min_w_48();
                        el = if options.layout().is_horizontal() {
                            el.w_64()
                        } else {
                            el.w_full()
                        };
                        el
                    }
                }),
            )
            .description(t!("Settings.desc.theme_static").to_string()),
        ],
        ThemeKind::Dynamic => {
            let dyn_mode_item = SettingItem::new(
                t!("Settings.item.theme_dyn_mode"),
                dropdown_field(
                    vec![
                        (
                            ThemeDynMode::System.as_str().into(),
                            t!("Settings.option.theme_dyn_mode.system").into(),
                        ),
                        (
                            ThemeDynMode::Light.as_str().into(),
                            t!("Settings.option.theme_dyn_mode.light").into(),
                        ),
                        (
                            ThemeDynMode::Dark.as_str().into(),
                            t!("Settings.option.theme_dyn_mode.dark").into(),
                        ),
                    ],
                    m,
                    move |model| {
                        SharedString::from(model.config.global.theme_pref.dyn_mode.as_str())
                    },
                    move |model, val| {
                        let mode = ThemeDynMode::parse(&val);
                        model.config.global.theme_pref.dyn_mode = mode;
                    },
                    Some(after_theme_kind),
                ),
            )
            .description(t!("Settings.desc.theme_dyn_mode").to_string());

            let make_select_item = |title: SharedString, desc: SharedString, state: &Entity<_>| {
                let state = state.clone();
                SettingItem::new(
                    title,
                    SettingField::render(move |options, _window, _cx| {
                        let mut el = Select::new(&state).with_size(options.size()).min_w_48();
                        el = if options.layout().is_horizontal() {
                            el.w_64()
                        } else {
                            el.w_full()
                        };
                        el
                    }),
                )
                .description(desc)
            };

            vec![
                dyn_mode_item,
                make_select_item(
                    t!("Settings.item.theme_light").into(),
                    t!("Settings.desc.theme_light").into(),
                    ctx.theme_state_dyn_light,
                ),
                make_select_item(
                    t!("Settings.item.theme_dark").into(),
                    t!("Settings.desc.theme_dark").into(),
                    ctx.theme_state_dyn_dark,
                ),
            ]
        }
    }
}

/// `theme_kind` / `theme_dyn_mode` setter 写完字段后的副作用：应用主题 + 重应用字号
/// （`apply_config` 会重置字号）。用模块内 `fn` 让它能当 `after_set` 的 fn pointer。
fn after_theme_kind(m: &Entity<AppModel>, cx: &mut App) {
    let pref = m.read(cx).config.global.theme_pref.clone();
    themes::apply_theme_pref(&pref, None, cx);
    themes::apply_font_size(m.read(cx).config.global.font_size, cx);
}

/// language setter 写完字段后的副作用：弹「重启确认」Dialog。
/// setter 只有 `&mut App` 没有 `&mut Window`，且从 dropdown Confirm 同步触发时窗口还在
/// `update_window` 调用栈里（直接 update 会报 "window not found"），故用 `cx.defer` 延后一帧。
fn after_language(_m: &Entity<AppModel>, cx: &mut App) {
    cx.defer(|cx| {
        tracing::info!("language setter: defer 触发, 调 open_dialog");
        if let Some(handle) = cx.windows().into_iter().next() {
            let result = handle.update(cx, |_view, window, cx| {
                window.open_alert_dialog(cx, |alert: AlertDialog, _w, _cx| {
                    alert
                        .title(t!("Settings.language_restart_dialog.title"))
                        .description(t!("Settings.language_restart_dialog.message").to_string())
                        // 单项 builder 取代整包 `DialogButtonProps`。
                        .ok_text(t!("Settings.language_restart_dialog.restart_button"))
                        .cancel_text(t!("Settings.language_restart_dialog.later_button"))
                        .confirm()
                        .on_ok(|_ev, _window, cx| {
                            cx.restart();
                            true
                        })
                });
            });
            tracing::info!("language setter: defer 后 open_dialog 结果 {result:?}");
        } else {
            tracing::warn!("language setter: defer 后无窗口, dialog 没法弹出");
        }
    });
}
