//! `rust_i18n` 的 `t!` 宏是**唯一**翻译入口（`gpui_kit::component` 同款机制），调用方写字符串
//! 字面量 key：`t!("Settings.item.theme")`。翻译表在 `locales/app.yml`（编译期嵌入，YAML
//! 顶层大写），带变量用官方 args：`t!("K", name = v)`（yaml 侧写 `%{name}`）。
//!
//! 与组件库**共享全局 locale**：两套 i18n 实例各管各的 key 表，但 `set_locale` 写的是同一个
//! `CURRENT_LOCALE`，所以一次 `set_locale` 同时影响双方。改语言**重启生效**：组件把文案
//! 一次性缓存在各 entity 里，切语言当帧 `refresh_windows` 不会重求这些缓存值。
//!
//! 本模块只保留 `t!` 不覆盖的 `Language → locale 字符串` 映射。

use crate::config::Language;

/// 把 `Language` 映射到本项目 `app.yml` 用的 locale 标签（`gpui_kit::component` 接受同一套标签）。
///
/// **`Language → locale 字符串` 的唯一权威映射**。
/// `TraditionalChinese` → `"zh-TW"`，**不是** `gpui_kit::component` 旧版用的 `"zh-HK"`。
pub const fn locale_for(lang: Language) -> &'static str {
    match lang {
        Language::SimplifiedChinese => "zh-CN",
        Language::TraditionalChinese => "zh-TW",
        Language::English => "en",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    /// 全局 locale 是共享可变状态：并行测试会互相踩。碰全局 locale 的测试都先抢这把锁，
    /// 串行执行。
    static LOCALE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock_locale() -> std::sync::MutexGuard<'static, ()> {
        LOCALE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 验证 `t!` 在三 locale 下的翻译返回，以及缺 key 时回落成 key 本身。
    #[test]
    fn t_macro_translates_across_locales() {
        let _g = lock_locale();
        rust_i18n::set_locale("en");
        assert_eq!(rust_i18n::t!("Nav.tasks"), "Tasks");

        rust_i18n::set_locale("zh-CN");
        assert_eq!(rust_i18n::t!("Nav.tasks"), "下载任务");

        rust_i18n::set_locale("zh-TW");
        assert_eq!(rust_i18n::t!("Nav.tasks"), "下載任務");

        rust_i18n::set_locale("en");
        assert_eq!(
            rust_i18n::t!("definitely.not.a.real.key"),
            "definitely.not.a.real.key"
        );
    }

    /// 验证 `t!` 的 args 形式按 `%{name}` 占位符替换（显式 locale，不碰全局）。
    #[test]
    fn t_macro_substitutes_args() {
        assert_eq!(
            rust_i18n::t!("Search.result.source", locale = "en", id = 3),
            "Source #3"
        );
        assert_eq!(
            rust_i18n::t!("Search.result.source", locale = "zh-CN", id = 3),
            "源 #3"
        );
    }

    #[test]
    fn locale_for_matches_app_yml_locale_tags() {
        assert_eq!(locale_for(Language::SimplifiedChinese), "zh-CN");
        assert_eq!(locale_for(Language::TraditionalChinese), "zh-TW");
        assert_eq!(locale_for(Language::English), "en");
    }

    const URL_DOWNLOAD_KEYS: &[&str] = &[
        "Search.url_download.button",
        "Search.url_download.dialog_title",
        "Search.url_download.placeholder",
        "Search.url_download.auto_pasted",
        "Search.url_download.paste_button",
        "Search.url_download.confirm",
        "Search.url_download.cancel",
        "Search.url_download.no_match",
        "Search.url_download.matched_source",
    ];

    #[test]
    fn url_download_translated_in_all_three_locales() {
        for &key in URL_DOWNLOAD_KEYS {
            for locale in ["en", "zh-CN", "zh-TW"] {
                let v = rust_i18n::t!(key, locale = locale);
                assert!(!v.is_empty(), "{key} 在 locale={locale} 翻译为空字符串");
                assert_ne!(
                    v.as_ref(),
                    key,
                    "{key} 在 locale={locale} 缺失（返回 key 本身）"
                );
            }
        }
    }
}
