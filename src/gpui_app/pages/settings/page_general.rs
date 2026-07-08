//! 常规页（gpui-component `Settings` 左侧 sidebar 第 1 项）。
//!
//! 3 个 group：
//! - 外观：主题模式（dropdown） / 按模式条件渲染的主题 `item（theme_mode_items`）/
//!   `语言（dropdown，after_set` 弹重启 dialog）/ 字号（Slider）
//! - 网络：GitHub 代理 / Cloudflare bypass（Input）
//! - 下载：下载目录（Input + 「浏览」suffix Button，调 rfd）/ 默认格式（dropdown）/
//!   TXT 编码（dropdown）/ 保留章节缓存 / 启用下载进度条（switch）
//!
//! `theme_mode_items` 之前在 `SettingsPage` impl 内（settings.rs:381），是 100 行的
//! 闭包工厂。拆到本文件 —— 只服务「外观」组，留 `pub(super)` 即可。

use gpui::{App, Entity, ParentElement, SharedString, Styled, div};
use gpui_component::{
    ActiveTheme as _, AxisExt as _, IconName, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{Dialog, DialogButtonProps},
    input::Input,
    select::Select,
    setting::{SettingField, SettingGroup, SettingItem, SettingPage},
    slider::SliderValue,
};
use tracing;

use crate::app::AppModel;
use crate::config::ExportFormat;
use crate::config::{Language, ThemeDynMode, ThemeKind};
use crate::gpui_app::themes;
use crate::i18n::ts;

use super::ctx::PageCtx;
use super::fields::{
    TXT_ENCODINGS, bool_field, dropdown_field, ext_from_str, ext_value, string_field,
};

/// 构造 Page 1（常规）= 外观 + 网络 + 下载。
pub(super) fn build(ctx: &PageCtx<'_>, cx: &App) -> SettingPage {
    let m = ctx.model.clone();
    let theme_kind = ctx.model.read(cx).config.global.theme_pref.kind;

    // 3 种应用语言 → (value_str, label)，存到 TOML `[global].language`，
    // 由 `Language::as_str()` 给出（"zh-CN" / "zh-TW" / "en"）。
    // label 走 i18n：切到 English 时显示 "Simplified Chinese" / "Traditional Chinese" / "English"。
    let language_options: Vec<(SharedString, SharedString)> = vec![
        (
            Language::SimplifiedChinese.as_str().into(),
            ts("Settings.option.language.zh_cn"),
        ),
        (
            Language::TraditionalChinese.as_str().into(),
            ts("Settings.option.language.zh_tw"),
        ),
        (
            Language::English.as_str().into(),
            ts("Settings.option.language.en"),
        ),
    ];

    // 主题模式：动态 / 静态（value_str 与 ThemeKind::as_str 一致）。
    let theme_kind_options: Vec<(SharedString, SharedString)> = vec![
        (
            ThemeKind::Dynamic.as_str().into(),
            ts("Settings.option.theme_kind.dynamic"),
        ),
        (
            ThemeKind::Static.as_str().into(),
            ts("Settings.option.theme_kind.static"),
        ),
    ];

    // 4 种输出格式 → (value_str, label)
    let ext_options: Vec<(SharedString, SharedString)> = vec![
        (ext_value(ExportFormat::Epub).into(), "epub".into()),
        (ext_value(ExportFormat::Txt).into(), "txt".into()),
        (ext_value(ExportFormat::Html).into(), "html".into()),
        (ext_value(ExportFormat::Pdf).into(), "pdf".into()),
    ];

    // 7 种常见 TXT 编码 → (value_str, label)
    let encoding_options: Vec<(SharedString, SharedString)> = TXT_ENCODINGS
        .iter()
        .map(|e| ((*e).into(), (*e).into()))
        .collect();

    SettingPage::new(ts("Settings.page.general"))
        .resettable(false)
        .default_open(true)
        .groups(vec![
            // ============ 外观 ============
            SettingGroup::new()
                .title(ts("Settings.group.appearance"))
                .items(
                    vec![
                        // -- 主题模式：动态 / 静态 --
                        SettingItem::new(
                            ts("Settings.item.theme_kind"),
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
                        .description(ts("Settings.desc.theme_kind")),
                        // 按当前主题模式条件渲染后续 item（theme_mode_items）。
                    ]
                    .into_iter()
                    .chain(theme_mode_items(ctx, theme_kind, &m))
                    .chain(std::iter::once(
                        // -- 界面语言（Language：应用 UI 语言；同时也是下载目标语言）--
                        SettingItem::new(
                            ts("Settings.item.language"),
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
                        .description(ts("Settings.desc.language")),
                    ))
                    .chain(std::iter::once(
                        // -- 字号（滑块，12–24px，实时缩放整个 app）--
                        // SliderState 由 `SettingsPage::new` 缓存（同 theme_state），
                        // 闭包里只复用。右侧小标签实时显示当前 px（从 SliderState 读，
                        // 拖拽过程中也跟着变）。`.flex_1()` 让滑块填满剩余宽度。
                        SettingItem::new(
                            ts("Settings.item.font_size"),
                            SettingField::render({
                                let font_size_state = ctx.font_size_state.clone();
                                move |options, _window, cx| {
                                    use gpui_component::slider::Slider;
                                    let n = match font_size_state.read(cx).value() {
                                        SliderValue::Single(v) => v,
                                        SliderValue::Range(_, end) => end,
                                    };
                                    let mut el = div().flex().items_center().gap_2();
                                    el = if options.layout.is_horizontal() {
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
                        .description(ts("Settings.desc.font_size")),
                    ))
                    .collect::<Vec<_>>(),
                ),
            // ============ 网络 ============
            SettingGroup::new()
                .title(ts("Settings.group.network"))
                .items(vec![
                    SettingItem::new(
                        ts("Settings.item.gh_proxy"),
                        string_field(
                            &m,
                            move |model| SharedString::from(model.config.global.gh_proxy.clone()),
                            move |model, s| model.config.global.gh_proxy = s,
                        ),
                    )
                    .description(ts("Settings.desc.gh_proxy")),
                    SettingItem::new(
                        ts("Settings.item.cf_bypass"),
                        string_field(
                            &m,
                            move |model| SharedString::from(model.config.global.cf_bypass.clone()),
                            move |model, s| model.config.global.cf_bypass = s,
                        ),
                    )
                    .description(ts("Settings.desc.cf_bypass")),
                ]),
            // ============ 下载 ============
            SettingGroup::new()
                .title(ts("Settings.group.download"))
                .items(vec![
                    // -- 下载目录（带「浏览…」图标，点击调 rfd 选目录）--
                    // gpui-component 0.5.1 的 `SettingField::input` 只能给裸 Input
                    // 没法挂 suffix icon。改走 `SettingField::render` + 原生
                    // `Input::new(&ctx.download_path_input).suffix(Button::...)`。
                    // InputState 缓存到 `SettingsPage` struct（和 theme_state 同理，
                    // 避免 click / focus / 输入内容在每次 render 后丢失），rfd 选
                    // 完目录回写 model + notify，下一次 render 走 `sync_download_path`
                    // 把 model 的新值推回 InputState。
                    SettingItem::new(
                        ts("Settings.item.download_path"),
                        SettingField::render({
                            let download_path_input = ctx.download_path_input.clone();
                            let pick_folder_listener = ctx.pick_folder_listener.clone();
                            move |options, _window, _cx| {
                                // 宽度要手动设：SettingField::input 内部 `.w_64()` /
                                // `.w_full()` 依 layout，不设的话 input 渲染成 0
                                // 大小 → text 被裁切看不见、suffix button 没 hit area
                                // → click 不响应。详见 `string.rs:76-86`。
                                let mut el = Input::new(&download_path_input)
                                    .with_size(options.size)
                                    .suffix({
                                        // ghost + xsmall 让 button 视觉上就是 icon，
                                        // 不抢 input 焦点、看起来像 input 的一部分。
                                        // input_story.rs:240 用的就是这个 pattern。
                                        //
                                        // **click handler**用 owner-cache 的
                                        // `pick_folder_listener`（见 SettingsPage struct
                                        // 注释）—— render 闭包拿不到 `Context<Self>`，
                                        // 在这里现建 `cx.listener` 不可行；早先尝试
                                        // 「`page_handle.update(cx, |_page, ctx| cx.spawn(...))`」
                                        // 双层套娃在 GPUI 0.2.2 下 click 不触发。
                                        //
                                        // `Rc<dyn Fn + 'static>::as_ref()` 拿到的是
                                        // `&'a Rc<dyn Fn>`，**不是 `'static`** —— `Button::on_click`
                                        // 要 `impl Fn + 'static`，传引用被拒。包一层
                                        // `move |...| listener(...)` 转成新的
                                        // `impl Fn + 'static`：捕获 Rc（'static），
                                        // 内部走 Rc::deref 调底层闭包。
                                        let listener = pick_folder_listener.clone();
                                        Button::new("download-path-pick")
                                            .ghost()
                                            .icon(IconName::FolderOpen)
                                            .xsmall()
                                            .on_click(move |ev, window, app| {
                                                listener(ev, window, app);
                                            })
                                    });
                                // horizontal layout → 固定 256px（与 `SettingField::input`
                                // 默认行为一致）；其它 → 占满整行。
                                if options.layout.is_horizontal() {
                                    el = el.w_64();
                                } else {
                                    el = el.w_full();
                                }
                                el
                            }
                        }),
                    )
                    .description(ts("Settings.desc.download_path")),
                    // -- 默认格式 --
                    SettingItem::new(
                        ts("Settings.item.default_format"),
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
                    .description(ts("Settings.desc.default_format")),
                    // -- TXT 编码 --
                    SettingItem::new(
                        ts("Settings.item.txt_encoding"),
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
                    .description(ts("Settings.desc.txt_encoding")),
                    // -- 保留章节缓存 --
                    SettingItem::new(
                        ts("Settings.item.preserve_chapter_cache"),
                        bool_field(
                            &m,
                            move |model| model.config.download.preserve_chapter_cache,
                            move |model, val| model.config.download.preserve_chapter_cache = val,
                        ),
                    )
                    .description(ts("Settings.desc.preserve_chapter_cache")),
                ]),
        ])
}

/// 按主题模式构建条件渲染的主题 item。
///
/// - `ThemeKind::Static` → 仅「静态主题」Select（全量主题）。
/// - `ThemeKind::Dynamic` → 「浅色/深色切换」dropdown + 「浅色主题」Select + 「深色主题」Select
///   （浅/深 Select 已按 mode 过滤，不会把深色变体选进浅色槽）。
///
/// 整 item 显隐（不是返回空 div 占位）：切模式后 `apply_theme_pref` →
/// `cx.refresh_windows()` → 下一帧 `build_pages` 读到新 `kind`，本函数返回不同 item 集。
fn theme_mode_items(ctx: &PageCtx<'_>, kind: ThemeKind, m: &Entity<AppModel>) -> Vec<SettingItem> {
    match kind {
        ThemeKind::Static => vec![
            SettingItem::new(
                ts("Settings.item.theme_static"),
                SettingField::render({
                    let state = ctx.theme_state_static.clone();
                    move |options, _window, _cx| {
                        let mut el = Select::new(&state).with_size(options.size).min_w_48();
                        el = if options.layout.is_horizontal() {
                            el.w_64()
                        } else {
                            el.w_full()
                        };
                        el
                    }
                }),
            )
            .description(ts("Settings.desc.theme_static")),
        ],
        ThemeKind::Dynamic => {
            let dyn_mode_item = SettingItem::new(
                ts("Settings.item.theme_dyn_mode"),
                dropdown_field(
                    vec![
                        (
                            ThemeDynMode::System.as_str().into(),
                            ts("Settings.option.theme_dyn_mode.system"),
                        ),
                        (
                            ThemeDynMode::Light.as_str().into(),
                            ts("Settings.option.theme_dyn_mode.light"),
                        ),
                        (
                            ThemeDynMode::Dark.as_str().into(),
                            ts("Settings.option.theme_dyn_mode.dark"),
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
            .description(ts("Settings.desc.theme_dyn_mode"));

            let make_select_item = |title: SharedString, desc: SharedString, state: &Entity<_>| {
                let state = state.clone();
                SettingItem::new(
                    title,
                    SettingField::render(move |options, _window, _cx| {
                        let mut el = Select::new(&state).with_size(options.size).min_w_48();
                        el = if options.layout.is_horizontal() {
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
                    ts("Settings.item.theme_light"),
                    ts("Settings.desc.theme_light"),
                    ctx.theme_state_dyn_light,
                ),
                make_select_item(
                    ts("Settings.item.theme_dark"),
                    ts("Settings.desc.theme_dark"),
                    ctx.theme_state_dyn_dark,
                ),
            ]
        }
    }
}

/// `theme_kind` / `theme_dyn_mode` setter 写完字段后的副作用：
/// 1. `apply_theme_pref` 读最新 pref + 应用到全局 Theme；
/// 2. `apply_font_size` 重新设字号（`apply_config` 会重置字号）。
///
/// 写不到 caller 的闭包环境 —— 用模块内 `fn` 强制成 fn pointer，让
/// `dropdown_field(... after_set: Option<fn(...)>)` 能 clone 出 `'static`。
fn after_theme_kind(m: &Entity<AppModel>, cx: &mut App) {
    let pref = m.read(cx).config.global.theme_pref.clone();
    themes::apply_theme_pref(&pref, None, cx);
    themes::apply_font_size(m.read(cx).config.global.font_size, cx);
}

/// language setter 写完字段后的副作用：弹「重启确认」Dialog。
///
/// setter 只有 `&mut App` 没有 `&mut Window`。直接调
/// `cx.windows().next().update(|.., window, cx| open_dialog)`
/// 中转会 `Err(window not found)`（2026-06-19 日志）——
/// 根因是 `AnyWindowHandle::update` 内部 `cx.windows.get_mut(id).take()`
/// 把窗口从 `SlotMap` 临时挪到调用栈，而我们 setter 是从 dropdown
/// Confirm 同步触发的，此时 root view 的 `update_window` 回调栈
/// 还没退出，再次 `take()` 同一窗口 → `SlotMap` 里为 None →
/// 报 "window not found"。
///
/// 解法：`cx.defer(closure)` —— 把闭包作为 Effect 推到
/// flush 队列（gpui 0.2.2 app.rs:1434），下一次 `flush_effects`
/// 时跑（届时窗口已放回 SlotMap），不再受 `update_window` 嵌套
/// take 影响。代价 1 帧延迟 ≈ 16ms，跟 `GPApp` 内部调度同步，
/// 用户无感。
///
/// 不污染 AppModel、不需要给 `SettingsPage` 加 flag、也不动 `RootView`。
fn after_language(_m: &Entity<AppModel>, cx: &mut App) {
    cx.defer(|cx| {
        tracing::info!("language setter: defer 触发, 调 open_dialog");
        if let Some(handle) = cx.windows().into_iter().next() {
            let result = handle.update(cx, |_view, window, cx| {
                window.open_dialog(cx, |dialog: Dialog, _w, _cx| {
                    dialog
                        .title(ts("Settings.language_restart_dialog.title"))
                        .child(div().child(ts("Settings.language_restart_dialog.message")))
                        .button_props(
                            DialogButtonProps::default()
                                .ok_text(ts("Settings.language_restart_dialog.restart_button"))
                                .cancel_text(ts("Settings.language_restart_dialog.later_button")),
                        )
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
