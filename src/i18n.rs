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
#[must_use]
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

    const TOOLS_KEYS: &[&str] = &[
        "Tools.page_title",
        "Tools.kind.convert",
        "Tools.kind.encoding",
        "Tools.kind.zhconv",
        "Tools.desc.convert",
        "Tools.desc.encoding",
        "Tools.desc.zhconv",
        "Tools.pick.title",
        "Tools.pick.filter",
        "Tools.pick.choose_file",
        "Tools.pick.choose_folder",
        "Tools.pick.detected",
        "Tools.pick.none",
        "Tools.pick.out_dir_default",
        "Tools.convert.output_format",
        "Tools.convert.chapter_regex",
        "Tools.convert.chapter_regex_placeholder",
        "Tools.convert.txt_encoding",
        "Tools.format.epub",
        "Tools.format.txt",
        "Tools.format.html",
        "Tools.format.pdf",
        "Tools.format.markdown",
        "Tools.encoding.source_encoding",
        "Tools.encoding.target_encoding",
        "Tools.zhconv.target",
        "Tools.zhconv.zh_cn",
        "Tools.zhconv.zh_tw",
        "Tools.zhconv.zh_hant",
        "Tools.run.start",
        "Tools.run.running",
        "Tools.run.reveal",
        "Tools.stage.loading",
        "Tools.stage.rendering",
        "Tools.stage.writing",
        "Tools.toast.success",
        "Tools.error.unsupported_format",
        "Tools.error.io",
        "Tools.error.decode",
        "Tools.error.invalid_regex",
        "Tools.error.zip_structure",
        "Tools.error.epub_structure",
        "Tools.error.no_content",
        "Tools.error.unencodable",
        "Tools.error.unknown_encoding",
        "Tools.error.export",
    ];

    #[test]
    fn tools_translated_in_all_three_locales() {
        for &key in TOOLS_KEYS {
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

    /// 本地书库页面的全部 key。清单来源（多行安全，排除本文件的常量块）：
    /// `grep -rhoE '"(Library|Reader)\.[A-Za-z0-9_.]+"' src --exclude=i18n.rs | tr -d '"' | sort -u`
    /// —— 必须覆盖该命令输出的每一个 `Library.*`；`Library.time.*` 由
    /// `utils::formatting` 的测试引用，同样算在内。
    /// `--exclude=i18n.rs` 不能省：常量本身就是带引号的 `"Library.*"` 字面量，不排除的话
    /// 命令会把自己的清单也统计进去，自检从此自指，再也发现不了「常量写了但代码里没有」的 key。
    /// 统计口径 = `src/` 下除 `i18n.rs` 外的代码引用。
    /// 书库改造前的旧词条（`Library.action_delete` 等）无代码引用，不进清单。
    const LIBRARY_KEYS: &[&str] = &[
        "Library.action_clear_filters",
        "Library.action_import",
        "Library.action_load_more",
        "Library.action_rescan",
        "Library.actions.continue",
        "Library.actions.delete_file",
        "Library.actions.edit",
        "Library.actions.hide",
        "Library.actions.info",
        "Library.actions.open_external",
        "Library.actions.relink",
        "Library.actions.restart",
        "Library.actions.reveal",
        "Library.actions.title",
        "Library.card.missing",
        "Library.delete.failed",
        "Library.delete_file_dialog.confirm_button",
        "Library.delete_file_dialog.message",
        "Library.delete_file_dialog.title",
        "Library.dialog.cancel_button",
        "Library.edit.author_placeholder",
        "Library.edit.cancel",
        "Library.edit.cover_filter",
        "Library.edit.no_cover",
        "Library.edit.pick_cover",
        "Library.edit.remove_cover",
        "Library.edit.save",
        "Library.edit.title",
        "Library.edit.title_placeholder",
        "Library.empty_filtered_subtitle",
        "Library.empty_filtered_title",
        "Library.empty_subtitle",
        "Library.empty_title",
        "Library.error.already_on_shelf",
        "Library.error.backup_failed",
        "Library.error.book_not_found",
        "Library.error.corrupt_backup",
        "Library.error.cover_failed",
        "Library.error.create_dir_failed",
        "Library.error.format_mismatch",
        "Library.error.future_version",
        "Library.error.load_failed",
        "Library.error.not_a_file",
        "Library.error.save_refused",
        "Library.error.save_refused_after_load_failure",
        "Library.error.unsupported_format",
        "Library.error.write_failed",
        "Library.filter.all",
        "Library.filter.favorites",
        "Library.filter.finished",
        "Library.filter.reading",
        "Library.filter.unread",
        "Library.hide_dialog.confirm_button",
        "Library.hide_dialog.message",
        "Library.hide_dialog.title",
        "Library.import.dialog_title",
        "Library.import.done",
        "Library.import.failed_item",
        "Library.import.filter_all",
        "Library.import.filter_books",
        "Library.info.added",
        "Library.info.bookmarks",
        "Library.info.chapters",
        "Library.info.format",
        "Library.info.path",
        "Library.info.progress",
        "Library.info.size",
        "Library.info.title",
        "Library.info.unknown",
        "Library.page_title",
        "Library.relink.dialog_title",
        "Library.scan_failed",
        "Library.search_placeholder",
        "Library.sort.added_at",
        "Library.sort.last_opened",
        "Library.sort.size",
        "Library.sort.title",
        "Library.status.finished",
        "Library.status.reading",
        "Library.status.unread",
        "Library.status.unsupported",
        "Library.subtitle",
        "Library.time.format_failed",
        "Library.time.invalid",
        "Library.time.unknown",
    ];

    /// 应用内阅读器的全部 key（同样来自上面的 grep 命令）。`Reader.error.action.*`
    /// 是错误面板的三个恢复按钮，Task 24 才开始调用，先纳入覆盖。
    const READER_KEYS: &[&str] = &[
        "Reader.action.back",
        "Reader.action.bookmarks",
        "Reader.action.close",
        "Reader.action.exit_immersive",
        "Reader.action.immersive",
        "Reader.action.mode_to_page",
        "Reader.action.mode_to_scroll",
        "Reader.action.more",
        "Reader.action.next_chapter",
        "Reader.action.next_page",
        "Reader.action.prev_chapter",
        "Reader.action.prev_page",
        "Reader.action.search",
        "Reader.action.settings",
        "Reader.action.toc",
        "Reader.bookmarks.add",
        "Reader.bookmarks.chapter",
        "Reader.bookmarks.delete",
        "Reader.bookmarks.duplicate",
        "Reader.bookmarks.empty",
        "Reader.bookmarks.invalid_time",
        "Reader.bookmarks.time_failed",
        "Reader.bookmarks.title",
        "Reader.bookmarks.unknown_time",
        "Reader.chapter_indicator",
        "Reader.error.action.back",
        "Reader.error.action.force_open",
        "Reader.error.action.open_external",
        "Reader.error.action.relink",
        "Reader.error.decode",
        "Reader.error.decode_detail",
        "Reader.error.encoding",
        "Reader.error.epub",
        "Reader.error.io",
        "Reader.error.layout_fallback",
        "Reader.error.missing",
        "Reader.error.no_content",
        "Reader.error.unsupported",
        "Reader.info.added",
        "Reader.info.author",
        "Reader.info.bookmarks",
        "Reader.info.chapters",
        "Reader.info.close",
        "Reader.info.format",
        "Reader.info.intro",
        "Reader.info.last_opened",
        "Reader.info.open",
        "Reader.info.path",
        "Reader.info.progress",
        "Reader.info.reveal",
        "Reader.info.size",
        "Reader.info.title",
        "Reader.info.unknown",
        "Reader.loading",
        "Reader.mode.page",
        "Reader.mode.scroll",
        "Reader.page_indicator",
        "Reader.search.hint",
        "Reader.search.placeholder",
        "Reader.search.results",
        "Reader.search.scanning",
        "Reader.search.title",
        "Reader.settings.title",
        "Reader.toc.chapter_count",
        "Reader.toc.empty",
        "Reader.toc.filter_placeholder",
        "Reader.toc.no_match",
        "Reader.toc.title",
        "Reader.typography.encoding",
        "Reader.typography.encoding_auto",
        "Reader.typography.font_size",
        "Reader.typography.indent",
        "Reader.typography.line_height",
        "Reader.typography.page_width",
        "Reader.typography.paragraph_spacing",
        "Reader.typography.preview",
        "Reader.typography.theme.app",
        "Reader.typography.theme.green",
        "Reader.typography.theme.night",
        "Reader.typography.theme.paper",
        "Reader.typography.theme.sepia",
        "Reader.typography.title",
    ];

    #[test]
    fn library_translated_in_all_three_locales() {
        for &key in LIBRARY_KEYS {
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

    #[test]
    fn reader_translated_in_all_three_locales() {
        for &key in READER_KEYS {
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
