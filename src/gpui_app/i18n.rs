//! 直接走 `rust_i18n`（gpui-component 同款机制）。
//!
//! ## 用法
//!
//! 在调用方直接写 `ts!("Settings.item.theme")` 字符串字面量 —— 无 key 常量、无单独枚举。
//! 翻译表在 `locales/app.yml`（编译期嵌入二进制），YAML 顶层大写（参考 gpui-component/ui.yml）。
//!
//! ## 与 gpui-component 共享全局 locale
//!
//! 我们加载 `so-novel-rs/locales/app.yml`，gpui-component 加载 `gpui-component/locales/ui.yml`，
//! 两个 i18n 实例**各自独立**（不会互相看到对方 YAML 的 key），但**全局 locale 是同一个**
//! （`rust_i18n::set_locale` 写到全局 `CURRENT_LOCALE`）。所以一次 `gpui_component::set_locale("en")`
//! 同时影响双方：`t!("Nav.search")` → "Search"，`t!("Settings.search_placeholder")` → "Search..."
//!
//! ## 改语言时的流程
//!
//! 1. 用户在设置页选 en
//! 2. `AppConfig.app_lang = AppLang::En; persist_settings()`
//! 3. `gpui_component::set_locale("en")` 写到全局 locale
//! 4. `cx.refresh_windows()` 触发整 app 重 render
//! 5. 所有 `ts!()` 调用的 `t!` 读新全局 locale 拿新翻译

use gpui::SharedString;

/// 翻译查找 — `Cow<'_, str>` → `SharedString` 转换。
///
/// 直接调 `_rust_i18n_try_translate` 而不是 `t!` 宏 —— `t!` 宏内部生成
/// `&rust_i18n::locale()` 拿到一个临时 `impl Deref<Target = str>` 的引用，
/// 触发 E0716 "temporary value dropped while borrowed"。手写等价版本，
/// 把 locale 绑到 local 变量，引用就指向 `Lazy<AtomicStr>` 内部的 static storage。
///
/// 找不到的 key 返回 key 字符串本身（开发期可见漏翻译）。
///
/// 注：`rust_i18n::i18n!("locales")` 必须在 crate root 调一次（见 `src/lib.rs`），
/// 生成的 `_rust_i18n_try_translate` 才是真正的翻译查找后端。
pub fn ts(key: &'static str) -> SharedString {
    let locale = rust_i18n::locale();
    crate::_rust_i18n_try_translate(&locale, key)
        .map(|cow| SharedString::from(cow.into_owned()))
        .unwrap_or_else(|| SharedString::from(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **单 `#[test]` 集中测所有 key × locale** —— 全局 locale 是共享状态，
    /// 并行测试会互相踩；放一个测试里顺序跑就稳。
    /// 退出时把 locale 恢复成 en。
    #[test]
    fn all_translations() {
        // ---------- en ----------
        rust_i18n::set_locale("en");
        assert_eq!(ts("Nav.search"), "Search");
        assert_eq!(ts("Nav.tasks"), "Tasks");
        assert_eq!(ts("Nav.library"), "Library");
        assert_eq!(ts("Nav.sources"), "Sources");
        assert_eq!(ts("Nav.settings"), "Settings");
        assert_eq!(ts("App.title"), "So Novel");
        assert_eq!(ts("Settings.page.general"), "General");
        assert_eq!(ts("Settings.page.crawl"), "Crawl");
        assert_eq!(ts("Settings.page.proxy"), "Proxy");
        assert_eq!(ts("Settings.page.about"), "About");
        assert_eq!(ts("Settings.group.appearance"), "Appearance");
        assert_eq!(ts("Settings.group.network"), "Network");
        assert_eq!(ts("Settings.group.download"), "Download");
        assert_eq!(ts("Settings.group.source"), "Sources");
        assert_eq!(ts("Settings.group.concurrency"), "Concurrency & interval");
        assert_eq!(ts("Settings.group.retry"), "Retry");
        assert_eq!(ts("Settings.group.http_proxy"), "HTTP proxy");
        assert_eq!(ts("Settings.group.cookie"), "Cookie");
        assert_eq!(ts("Settings.group.info"), "Info");
        assert_eq!(ts("Settings.item.theme"), "Theme");
        assert_eq!(ts("Settings.item.app_lang"), "Interface language");
        assert_eq!(ts("Settings.item.gh_proxy"), "GitHub proxy");
        assert_eq!(ts("Settings.item.download_path"), "Download directory");
        assert_eq!(ts("Settings.item.default_format"), "Default format");
        assert_eq!(ts("Settings.item.txt_encoding"), "TXT encoding");
        assert_eq!(ts("Settings.item.preserve_chapter_cache"), "Preserve chapter cache");
        assert_eq!(ts("Settings.item.enable_progressbar"), "Enable download progress bar");
        assert_eq!(ts("Settings.item.book_lang"), "Source language");
        assert_eq!(ts("Settings.item.search_limit"), "Search result limit");
        assert_eq!(ts("Settings.item.search_filter"), "Filter & sort by similarity");
        assert_eq!(ts("Settings.item.concurrency"), "Concurrency limit");
        assert_eq!(ts("Settings.item.min_interval"), "Request interval min (ms)");
        assert_eq!(ts("Settings.item.max_interval"), "Request interval max (ms)");
        assert_eq!(ts("Settings.item.enable_retry"), "Enable retry on failure");
        assert_eq!(ts("Settings.item.max_retries"), "Max retry count");
        assert_eq!(ts("Settings.item.retry_min_interval"), "Retry interval min (ms)");
        assert_eq!(ts("Settings.item.retry_max_interval"), "Retry interval max (ms)");
        assert_eq!(ts("Settings.item.proxy_enabled"), "Enable HTTP proxy");
        assert_eq!(ts("Settings.item.proxy_host"), "Proxy host");
        assert_eq!(ts("Settings.item.proxy_port"), "Proxy port");
        assert_eq!(ts("Settings.item.qidian_cookie"), "Qidian cookie");
        assert_eq!(ts("Settings.item.version"), "Version");
        assert_eq!(ts("Settings.item.check_update"), "Check for updates");
        assert_eq!(ts("Settings.item.open_github"), "Project homepage");
        assert_eq!(
            ts("Settings.desc.theme"),
            "Choose the UI theme; changes take effect immediately."
        );
        assert_eq!(
            ts("Settings.desc.app_lang"),
            "App UI language; affects Sidebar search / Select / Dialog placeholders, etc. Changes take effect immediately."
        );
        assert_eq!(
            ts("Settings.desc.gh_proxy"),
            "Used to speed up release / raw asset downloads; leave empty to use default."
        );
        assert_eq!(
            ts("Settings.desc.cf_bypass"),
            "Base URL of a local or remote sarperavci/CloudflareBypass service."
        );
        assert_eq!(
            ts("Settings.desc.download_path"),
            "Directory to save downloaded book files (absolute path)."
        );
        assert_eq!(
            ts("Settings.desc.default_format"),
            "Output format for downloaded files."
        );
        assert_eq!(
            ts("Settings.desc.txt_encoding"),
            "Character encoding for TXT export; pick GBK for older devices."
        );
        assert_eq!(
            ts("Settings.desc.preserve_chapter_cache"),
            "When disabled, re-downloading will fetch all chapters from scratch."
        );
        assert_eq!(
            ts("Settings.desc.enable_progressbar"),
            "Takes effect in terminal / CLI mode."
        );
        assert_eq!(
            ts("Settings.desc.book_lang"),
            "Locale hint used when matching sources (zh_CN / zh_TW / zh_Hant); affects source filtering only, not the app UI."
        );
        assert_eq!(
            ts("Settings.desc.search_limit"),
            "Max results per source; -1 means unlimited."
        );
        assert_eq!(
            ts("Settings.desc.search_filter"),
            "Filter out low-similarity results and sort by title/author similarity."
        );
        assert_eq!(
            ts("Settings.desc.concurrency"),
            "-1 = auto: min(50, chapter count)."
        );
        assert_eq!(
            ts("Settings.desc.min_interval"),
            "min ≤ max; wait a random ms in [min..max] before each chapter fetch."
        );
        assert_eq!(ts("Settings.desc.max_interval"), "See the previous item.");
        assert_eq!(
            ts("Settings.desc.enable_retry"),
            "On chapter fetch failure, retry with the intervals below; give up after max retries."
        );
        assert_eq!(ts("Settings.desc.max_retries"), "0 = no retry.");
        assert_eq!(
            ts("Settings.desc.retry_min_interval"),
            "min ≤ max; wait a random ms in [min..max] between retries."
        );
        assert_eq!(ts("Settings.desc.retry_max_interval"), "See the previous item.");
        assert_eq!(
            ts("Settings.desc.proxy_enabled"),
            "Route all outbound requests through this proxy (source fetch + health check)."
        );
        assert_eq!(ts("Settings.desc.proxy_host"), "Proxy server address (IP or domain).");
        assert_eq!(ts("Settings.desc.proxy_port"), "Proxy server port (1-65535).");
        assert_eq!(
            ts("Settings.desc.qidian_cookie"),
            "Required for qidian.com / qidian overseas sites; can be ignored for other sources."
        );
        assert_eq!(
            ts("Settings.desc.version"),
            "So Novel — Rust + GPUI desktop client."
        );
        assert_eq!(
            ts("Settings.desc.check_update"),
            "Query GitHub releases for a newer version."
        );
        assert_eq!(
            ts("Settings.desc.open_github"),
            "Source code / issue tracker / latest release are all on GitHub."
        );
        assert_eq!(ts("Settings.check_update_button"), "Check GitHub for updates");
        assert_eq!(ts("Settings.open_github_button"), "Open GitHub");
        assert_eq!(
            ts("Settings.choose_download_dir_dialog_title"),
            "Select download directory"
        );
        assert_eq!(ts("Settings.option.booklang.zh_cn"), "Simplified Chinese");
        assert_eq!(ts("Settings.option.booklang.zh_tw"), "Traditional Chinese");
        assert_eq!(
            ts("Settings.option.booklang.zh_hant"),
            "Traditional Chinese (generic)"
        );
        assert_eq!(ts("Settings.option.applang.zh_cn"), "Simplified Chinese");
        assert_eq!(ts("Settings.option.applang.zh_tw"), "Traditional Chinese");
        assert_eq!(ts("Settings.option.applang.en"), "English");
        // 未知 key 走 fallback：返回 key 字符串本身（开发期可见漏翻译）
        assert_eq!(ts("foo.bar"), "foo.bar");

        // ---------- zh-CN ----------
        rust_i18n::set_locale("zh-CN");
        assert_eq!(ts("Nav.search"), "搜索下载");
        assert_eq!(ts("App.title"), "So Novel");
        assert_eq!(ts("Settings.page.general"), "常规");
        assert_eq!(ts("Settings.page.crawl"), "抓取");
        assert_eq!(ts("Settings.page.proxy"), "代理");
        assert_eq!(ts("Settings.page.about"), "关于");
        // app_lang 标签切到中文 UI 时用各语言自身名字
        assert_eq!(ts("Settings.option.applang.zh_cn"), "简体中文");
        assert_eq!(ts("Settings.option.applang.zh_tw"), "繁體中文");
        // 英文选项保持 "English" 不译
        assert_eq!(ts("Settings.option.applang.en"), "English");
        assert_eq!(
            ts("Settings.choose_download_dir_dialog_title"),
            "选择下载目录"
        );

        // ---------- zh-HK ----------
        rust_i18n::set_locale("zh-HK");
        assert_eq!(ts("Nav.search"), "搜尋下載");
        assert_eq!(ts("App.title"), "So Novel");
        assert_eq!(
            ts("Settings.choose_download_dir_dialog_title"),
            "選擇下載目錄"
        );

        // 恢复 en —— 避免污染其他 lib 测试（虽然 i18n locale 是 crate 全局，
        // 但其他测试模块不依赖 locale）
        rust_i18n::set_locale("en");
    }
}
