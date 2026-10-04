//! 抓取页（`Settings` 左侧 sidebar 第 2 项）。
//!
//! 3 个 group：书源（搜索条数上限 / 过滤低相似度）、并发与间隔（并发上限 / 请求间隔 min·max）、
//! 重试（启用重试 / 最大次数 / 重试间隔 min·max）。
//!
//! 全是纯 field + setter，无 dropdown 副作用，统一走 `bool_field` /
//! `number_field_option_i32` / `number_field_u32_clamped`。

use gpui_kit::App;
use gpui_kit::component::setting::{NumberFieldOptions, SettingGroup, SettingItem, SettingPage};
use rust_i18n::t;

use super::ctx::PageCtx;
use super::fields::{bool_field, number_field_option_i32, number_field_u32_clamped};

pub(super) fn build(ctx: &PageCtx<'_>, _cx: &App) -> SettingPage {
    let m = ctx.model.clone();

    SettingPage::new(t!("Settings.page.crawl"))
        .resettable(false)
        .default_open(true)
        .groups(vec![
            // ============ 书源 ============
            SettingGroup::new()
                .title(t!("Settings.group.source"))
                .items(vec![
                    // 搜索条数上限（Option<i32>, -1 = 不限）
                    SettingItem::new(
                        t!("Settings.item.search_limit"),
                        number_field_option_i32(
                            &m,
                            NumberFieldOptions {
                                min: -1.0,
                                max: 10_000.0,
                                ..Default::default()
                            },
                            move |model| model.config.source.search_limit,
                            move |model, v| model.config.source.search_limit = v,
                        ),
                    )
                    .description(t!("Settings.desc.search_limit").to_string()),
                    // 过滤低相似度
                    SettingItem::new(
                        t!("Settings.item.search_filter"),
                        bool_field(
                            &m,
                            move |model| model.config.source.search_filter,
                            move |model, val| model.config.source.search_filter = val,
                        ),
                    )
                    .description(t!("Settings.desc.search_filter").to_string()),
                ]),
            // ============ 并发与间隔 ============
            SettingGroup::new()
                .title(t!("Settings.group.concurrency"))
                .items(vec![
                    // 并发上限（Option<i32>, -1 = 自动）
                    SettingItem::new(
                        t!("Settings.item.concurrency"),
                        number_field_option_i32(
                            &m,
                            NumberFieldOptions {
                                min: -1.0,
                                max: 100.0,
                                ..Default::default()
                            },
                            move |model| model.config.crawl.concurrency,
                            move |model, v| model.config.crawl.concurrency = v,
                        ),
                    )
                    .description(t!("Settings.desc.concurrency").to_string()),
                    // 请求间隔 min
                    SettingItem::new(
                        t!("Settings.item.min_interval"),
                        number_field_u32_clamped(
                            &m,
                            NumberFieldOptions {
                                min: 0.0,
                                max: 60_000.0,
                                ..Default::default()
                            },
                            move |model| model.config.crawl.min_interval,
                            move |model, v| model.config.crawl.min_interval = v,
                        ),
                    )
                    .description(t!("Settings.desc.min_interval").to_string()),
                    // 请求间隔 max
                    SettingItem::new(
                        t!("Settings.item.max_interval"),
                        number_field_u32_clamped(
                            &m,
                            NumberFieldOptions {
                                min: 0.0,
                                max: 60_000.0,
                                ..Default::default()
                            },
                            move |model| model.config.crawl.max_interval,
                            move |model, v| model.config.crawl.max_interval = v,
                        ),
                    )
                    .description(t!("Settings.desc.max_interval").to_string()),
                ]),
            // ============ 重试 ============
            SettingGroup::new()
                .title(t!("Settings.group.retry"))
                .items(vec![
                    // 启用失败重试
                    SettingItem::new(
                        t!("Settings.item.enable_retry"),
                        bool_field(
                            &m,
                            move |model| model.config.crawl.enable_retry,
                            move |model, val| model.config.crawl.enable_retry = val,
                        ),
                    )
                    .description(t!("Settings.desc.enable_retry").to_string()),
                    // 最大重试次数
                    SettingItem::new(
                        t!("Settings.item.max_retries"),
                        number_field_u32_clamped(
                            &m,
                            NumberFieldOptions {
                                min: 0.0,
                                max: 20.0,
                                ..Default::default()
                            },
                            move |model| model.config.crawl.max_retries,
                            move |model, v| model.config.crawl.max_retries = v,
                        ),
                    )
                    .description(t!("Settings.desc.max_retries").to_string()),
                    // 重试间隔 min
                    SettingItem::new(
                        t!("Settings.item.retry_min_interval"),
                        number_field_u32_clamped(
                            &m,
                            NumberFieldOptions {
                                min: 0.0,
                                max: 60_000.0,
                                ..Default::default()
                            },
                            move |model| model.config.crawl.retry_min_interval,
                            move |model, v| model.config.crawl.retry_min_interval = v,
                        ),
                    )
                    .description(t!("Settings.desc.retry_min_interval").to_string()),
                    // 重试间隔 max
                    SettingItem::new(
                        t!("Settings.item.retry_max_interval"),
                        number_field_u32_clamped(
                            &m,
                            NumberFieldOptions {
                                min: 0.0,
                                max: 60_000.0,
                                ..Default::default()
                            },
                            move |model| model.config.crawl.retry_max_interval,
                            move |model, v| model.config.crawl.retry_max_interval = v,
                        ),
                    )
                    .description(t!("Settings.desc.retry_max_interval").to_string()),
                ]),
        ])
}
