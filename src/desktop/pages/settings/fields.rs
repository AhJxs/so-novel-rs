//! 各种 `SettingField` 的 boilerplate helper。
//!
//! 原 `settings.rs::build_pages` 里有 23 个 setter，其中 20 个共用模式：
//! `m.update(cx, |model, _| { model.config.X = ...; model.persist_settings(); })`。
//! 抽成 helper 后每个 call site 只写「字段 getter」+「字段 setter」2 行闭包。
//!
//! 3 个有副作用的 `setter（theme_kind` / `theme_dyn_mode` / language）走 `dropdown_field`
//! + `after_set: Option<fn(...)>` —— `fn` 指针（不是 `FnMut`）让 helper 内部能 clone
//!   出 `'static`，避开闭包生命周期问题。
//!
//! `number_field` 拆 3 个 helper —— `Option<i32>` 的 -1 sentinel / `u32` 的 `clamp(val, 0.0)`
//! / `u16` 的 `as` cast 三者语义不同，硬抽成 1 个会让 caller 写更多类型注解。

use gpui::{App, Entity, SharedString};
use gpui_component::setting::{NumberFieldOptions, SettingField};

use crate::desktop::model::AppModel;
use crate::config::ExportFormat;

/// String 字段（Input）—— getter 返回 `SharedString`，setter 拿到新 `String`。
///
/// 用于 `gh_proxy` / `cf_bypass` / `proxy_host` / `qidian_cookie`。
pub(super) fn string_field<G, S>(
    m: &Entity<AppModel>,
    getter: G,
    setter_field: S,
) -> SettingField<SharedString>
where
    G: Fn(&AppModel) -> SharedString + 'static,
    S: Fn(&mut AppModel, String) + 'static,
{
    SettingField::input(
        {
            let m = m.clone();
            move |cx: &App| getter(m.read(cx))
        },
        {
            let m = m.clone();
            move |val: SharedString, cx: &mut App| {
                m.update(cx, |model, _| {
                    setter_field(model, val.to_string());
                    model.persist_settings();
                });
            }
        },
    )
}

/// bool 字段（Switch）—— 5 `处（search_filter` / `preserve_chapter_cache` / `enable_retry` / `proxy_enabled`）。
pub(super) fn bool_field<G, S>(
    m: &Entity<AppModel>,
    getter: G,
    setter_field: S,
) -> SettingField<bool>
where
    G: Fn(&AppModel) -> bool + 'static,
    S: Fn(&mut AppModel, bool) + 'static,
{
    SettingField::switch(
        {
            let m = m.clone();
            move |cx: &App| getter(m.read(cx))
        },
        {
            let m = m.clone();
            move |val: bool, cx: &mut App| {
                m.update(cx, |model, _| {
                    setter_field(model, val);
                    model.persist_settings();
                });
            }
        },
    )
}

/// `Option<i32>` 字段（number_input，-1 sentinel 表示"不限制"）。
///
/// 用于 `search_limit` / concurrency。`getter` 返 `None` → UI 显示 -1；
/// `setter` 拿到 `None` 时 caller 写 `model.config.X = None`。
pub(super) fn number_field_option_i32<G, S>(
    m: &Entity<AppModel>,
    opts: NumberFieldOptions,
    getter: G,
    setter_field: S,
) -> SettingField<f64>
where
    G: Fn(&AppModel) -> Option<i32> + 'static,
    S: Fn(&mut AppModel, Option<i32>) + 'static,
{
    SettingField::number_input(
        opts,
        {
            let m = m.clone();
            move |cx: &App| {
                getter(m.read(cx)).map_or(-1.0, |v| v as f64) // sentinel: None → -1
            }
        },
        {
            let m = m.clone();
            move |val: f64, cx: &mut App| {
                m.update(cx, |model, _| {
                    let v = if val < 0.0 { None } else { Some(val as i32) };
                    setter_field(model, v);
                    model.persist_settings();
                });
            }
        },
    )
}

/// `u32` `字段（number_input，val` 钳到 ≥0）。
///
/// 用于 `min_interval` / `max_interval` / `max_retries` / `retry_min_interval` / `retry_max_interval`。
/// `val.max(0.0) as u32` —— 负数向上 saturate。
pub(super) fn number_field_u32_clamped<G, S>(
    m: &Entity<AppModel>,
    opts: NumberFieldOptions,
    getter: G,
    setter_field: S,
) -> SettingField<f64>
where
    G: Fn(&AppModel) -> u32 + 'static,
    S: Fn(&mut AppModel, u32) + 'static,
{
    SettingField::number_input(
        opts,
        {
            let m = m.clone();
            move |cx: &App| getter(m.read(cx)) as f64
        },
        {
            let m = m.clone();
            move |val: f64, cx: &mut App| {
                m.update(cx, |model, _| {
                    let v = val.max(0.0) as u32;
                    setter_field(model, v);
                    model.persist_settings();
                });
            }
        },
    )
}

/// `u16` `字段（number_input，val` as u16）。
///
/// 仅 `proxy_port` 一处。范围在 `NumberFieldOptions { min: 1.0, max: 65535.0 }` 由 caller 控制。
pub(super) fn number_field_u16<G, S>(
    m: &Entity<AppModel>,
    opts: NumberFieldOptions,
    getter: G,
    setter_field: S,
) -> SettingField<f64>
where
    G: Fn(&AppModel) -> u16 + 'static,
    S: Fn(&mut AppModel, u16) + 'static,
{
    SettingField::number_input(
        opts,
        {
            let m = m.clone();
            move |cx: &App| getter(m.read(cx)) as f64
        },
        {
            let m = m.clone();
            move |val: f64, cx: &mut App| {
                m.update(cx, |model, _| {
                    let v = val as u16;
                    setter_field(model, v);
                    model.persist_settings();
                });
            }
        },
    )
}

/// Dropdown 字段 —— 5 处共用（encoding / `ext_name` / `theme_kind` / `theme_dyn_mode` / language）。
///
/// `after_set: Option<fn(&Entity<AppModel>, &mut App)>` —— setter body 写完后
/// `触发的副作用（reapply_theme` / cx.defer 弹 dialog）。`fn` 指针 (不是闭包) 让
/// helper 内部能 clone 出 `'static`；caller 用模块内 `fn` 或 `let after: fn(...) = ...`。
pub(super) fn dropdown_field<G, S>(
    options: Vec<(SharedString, SharedString)>,
    m: &Entity<AppModel>,
    getter: G,
    setter_field: S,
    after_set: Option<fn(&Entity<AppModel>, &mut App)>,
) -> SettingField<SharedString>
where
    G: Fn(&AppModel) -> SharedString + 'static,
    S: Fn(&mut AppModel, SharedString) + 'static,
{
    SettingField::dropdown(
        options,
        {
            let m = m.clone();
            move |cx: &App| getter(m.read(cx))
        },
        {
            let m = m.clone();
            move |val: SharedString, cx: &mut App| {
                m.update(cx, |model, _| {
                    setter_field(model, val);
                    model.persist_settings();
                });
                if let Some(after) = after_set {
                    after(&m, cx);
                }
            }
        },
    )
}

/// `ExportFormat` ↔ `&'static str` 转换 —— 用于 `ext_name` dropdown。
///
/// 4 个值用 match 而不是 `as_ref()`，避免依赖 `Display` 顺序。
pub(super) const fn ext_value(e: ExportFormat) -> &'static str {
    match e {
        ExportFormat::Epub => "epub",
        ExportFormat::Txt => "txt",
        ExportFormat::Html => "html",
        ExportFormat::Pdf => "pdf",
    }
}

pub(super) fn ext_from_str(s: &str) -> Option<ExportFormat> {
    match s {
        "epub" => Some(ExportFormat::Epub),
        "txt" => Some(ExportFormat::Txt),
        "html" => Some(ExportFormat::Html),
        "pdf" => Some(ExportFormat::Pdf),
        _ => None,
    }
}

/// 7 种常见 TXT 编码。
pub(super) const TXT_ENCODINGS: &[&str] = &[
    "UTF-8",
    "GBK",
    "GB18030",
    "Big5",
    "BIG5HKSCS",
    "UTF-16LE",
    "UTF-16BE",
];
