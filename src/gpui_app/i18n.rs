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

/// 翻译查找 + 简单变量替换 —— `ts()` 的扩展，handle `{var}` 占位符。
///
/// 用法：`ts_fmt("Library.delete_dialog.message", &[("file_name", "foo.epub")])`
/// 对应 YAML：
/// ```yaml
/// Library:
///   delete_dialog:
///     message: "Are you sure you want to delete \"{file_name}\"? ..."
/// ```
///
/// 实现：`_rust_i18n_try_translate` 对 v2 YAML 返回的是**带 `{var}` 占位符的原文**
/// （不替占位符——rust-i18n 的 `t!` 宏自己接管 format!，我们是裸函数访问层），
/// 所以这里手动 `replace("{var}", value)`。
///
/// 安全前提：替换的 value 不能包含 `{` 或 `}` 字面字符 —— 否则会误替换或注入
/// 新的占位符。所有 caller 的 value 都是内部数据（PathBuf 转 String、enum 名等），
/// 不会带花括号。如果未来 value 可能包含用户输入，需要 escape。
pub fn ts_fmt(key: &'static str, vars: &[(&str, &str)]) -> SharedString {
    let locale = rust_i18n::locale();
    let mut result = crate::_rust_i18n_try_translate(&locale, key)
        .map(|cow| cow.into_owned())
        .unwrap_or_else(|| key.to_string());
    for (name, value) in vars {
        // 占位符形式 `{name}` —— `format!("{{{}}}", name)` 转义出字面 `{name}`。
        result = result.replace(&format!("{{{name}}}"), value);
    }
    SharedString::from(result)
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

        // ---- Library 页面（library.rs）----
        assert_eq!(ts("Library.page_title"), "Library");
        assert_eq!(ts("Library.download_path_label"), "Download directory");
        assert_eq!(ts("Library.filter_placeholder"), "Filter by file name…");
        assert_eq!(ts("Library.filter_option_all"), "All");
        assert_eq!(ts("Library.action_open"), "Open");
        assert_eq!(ts("Library.action_reveal"), "Reveal");
        assert_eq!(ts("Library.action_delete"), "Delete");
        assert_eq!(ts("Library.empty_title"), "Library is empty");
        assert_eq!(
            ts("Library.empty_subtitle"),
            "Download a book and it will appear here automatically."
        );
        assert_eq!(ts("Library.scan_failed"), "Scan failed");
        assert_eq!(ts("Library.delete_dialog.title"), "Confirm delete");
        // ts_fmt 替换 {file_name} 占位符
        assert_eq!(
            ts_fmt(
                "Library.delete_dialog.message",
                &[("file_name", "foo.epub")]
            ),
            "Are you sure you want to delete \"foo.epub\"? This action cannot be undone."
        );
        assert_eq!(ts("Library.delete_dialog.confirm_button"), "Delete");
        assert_eq!(ts("Library.delete_dialog.cancel_button"), "Cancel");
        assert_eq!(ts("Library.fallback_unknown_filename"), "(unknown file name)");
        assert_eq!(ts("Library.time.unknown"), "(unknown)");
        assert_eq!(ts("Library.time.invalid"), "(invalid time)");
        assert_eq!(ts("Library.time.format_failed"), "(format failed)");

        // ---- Sources 页面（sources.rs）----
        assert_eq!(ts("Sources.page_title"), "Source management");
        assert_eq!(
            ts("Sources.page_subtitle"),
            "Enable / disable, connectivity check, JSON import"
        );
        assert_eq!(ts("Sources.filter.placeholder"), "Filter by name or URL…");
        assert_eq!(ts("Sources.status.all"), "All");
        assert_eq!(ts("Sources.status.enabled"), "Enabled");
        assert_eq!(ts("Sources.status.disabled"), "Disabled");
        assert_eq!(ts("Sources.stat.total"), "Total");
        assert_eq!(ts("Sources.stat.available"), "Available");
        assert_eq!(ts("Sources.health.progress"), "checked");
        assert_eq!(ts("Sources.health.not_tested"), "Not tested");
        assert_eq!(ts("Sources.health.error"), "Error");
        assert_eq!(
            ts_fmt("Sources.health.http_status", &[("status", "404")]),
            "HTTP 404"
        );
        assert_eq!(ts("Sources.health.network_error"), "Network error");
        assert_eq!(ts("Sources.error.load_failed"), "Rule load failed");
        assert_eq!(ts("Sources.empty.title"), "No sources imported");
        assert_eq!(ts("Sources.action.add"), "Add");
        assert_eq!(ts("Sources.action.health_check"), "Health check");
        assert_eq!(ts("Sources.action.delete"), "Delete");
        assert_eq!(ts("Sources.tag.proxy"), "Proxy");
        assert_eq!(ts("Sources.delete_dialog.title"), "Confirm delete");
        assert_eq!(
            ts_fmt("Sources.delete_dialog.message", &[("source_id", "42")]),
            "Are you sure you want to delete source #42? This action cannot be undone."
        );
        assert_eq!(ts("Sources.delete_dialog.cancel"), "Cancel");
        // 导入反馈：{inserted} / {skipped} 占位符
        assert_eq!(
            ts_fmt(
                "Sources.import.result",
                &[("inserted", "5"), ("skipped", "2")]
            ),
            "Imported 5, skipped 2 duplicate(s)"
        );
        assert_eq!(ts("Sources.add_source.dialog_title"), "Select source JSON file");
        assert_eq!(ts("Sources.add_source.filter_json"), "JSON rule files");
        assert_eq!(ts("Sources.add_source.filter_all"), "All files");

        // ---- Search 页面（search.rs）----
        assert_eq!(ts("Search.page_title"), "Search & Download");
        assert_eq!(
            ts("Search.page_subtitle"),
            "Search by book name or author; aggregate or single-source"
        );
        assert_eq!(ts("Search.filter.placeholder"), "Book name / author");
        assert_eq!(ts("Search.source.label"), "Source");
        assert_eq!(ts("Search.source.aggregate"), "All sources");
        assert_eq!(ts("Search.action.search"), "Search");
        assert_eq!(ts("Search.action.select_chapters"), "Select Chapters");
        assert_eq!(ts("Search.action.select_chapters_unavailable"), "Coming soon");
        assert_eq!(ts("Search.action.download_whole"), "Whole Book");
        assert_eq!(ts("Search.action.download_started"), "Download queued");
        assert_eq!(ts("Search.empty.title"), "Enter a keyword to start searching");
        assert_eq!(
            ts("Search.empty.subtitle"),
            "Search by book name or author; single-source reduces network requests."
        );
        assert_eq!(ts("Search.source_status.pending"), "Pending");
        assert_eq!(ts("Search.source_status.format"), "results");
        assert_eq!(ts("Search.result.author"), "Author");
        assert_eq!(ts("Search.result.latest_chapter"), "Latest");
        assert_eq!(ts("Search.result.unknown_author"), "(Unknown)");
        assert_eq!(ts("Search.result.no_latest"), "(No latest chapter)");
        // ts_fmt 替换 {id} 占位符
        assert_eq!(ts_fmt("Search.result.source", &[("id", "3")]), "Source #3");

        // 未知 key 走 fallback：返回 key 字符串本身（开发期可见漏翻译）
        assert_eq!(ts("foo.bar"), "foo.bar");
        // 未知 key 在 ts_fmt 里同样走 fallback（不会 panic）
        assert_eq!(
            ts_fmt("foo.bar", &[("x", "y")]),
            "foo.bar"
        );

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

        // ---- Library zh-CN ----
        assert_eq!(ts("Library.page_title"), "本地书库");
        assert_eq!(ts("Library.download_path_label"), "下载目录");
        assert_eq!(ts("Library.filter_placeholder"), "按文件名过滤…");
        assert_eq!(ts("Library.filter_option_all"), "全部");
        assert_eq!(ts("Library.action_open"), "打开");
        assert_eq!(ts("Library.action_reveal"), "位置");
        assert_eq!(ts("Library.action_delete"), "删除");
        assert_eq!(ts("Library.empty_title"), "本地书库为空");
        assert_eq!(
            ts("Library.empty_subtitle"),
            "下载一本书后，它会自动出现在这里。"
        );
        assert_eq!(ts("Library.scan_failed"), "扫描失败");
        assert_eq!(ts("Library.delete_dialog.title"), "确认删除");
        assert_eq!(
            ts_fmt("Library.delete_dialog.message", &[("file_name", "foo.epub")]),
            "确定要删除 \"foo.epub\" 吗？此操作无法撤销。"
        );
        assert_eq!(ts("Library.delete_dialog.cancel_button"), "取消");
        assert_eq!(ts("Library.fallback_unknown_filename"), "(未知文件名)");
        assert_eq!(ts("Library.time.unknown"), "(未知)");
        assert_eq!(ts("Library.time.invalid"), "(无效时间)");
        assert_eq!(ts("Library.time.format_failed"), "(格式化失败)");

        // ---- Sources zh-CN ----
        assert_eq!(ts("Sources.page_title"), "书源管理");
        assert_eq!(
            ts("Sources.page_subtitle"),
            "启用/禁用、连通性检测、JSON 导入"
        );
        assert_eq!(ts("Sources.filter.placeholder"), "按名称或 URL 过滤…");
        assert_eq!(ts("Sources.status.all"), "全部");
        assert_eq!(ts("Sources.status.enabled"), "启用");
        assert_eq!(ts("Sources.status.disabled"), "禁用");
        assert_eq!(ts("Sources.stat.total"), "总数");
        assert_eq!(ts("Sources.stat.available"), "可用");
        assert_eq!(ts("Sources.health.progress"), "已检测");
        assert_eq!(ts("Sources.health.not_tested"), "未测");
        assert_eq!(ts("Sources.health.error"), "错误");
        assert_eq!(
            ts_fmt("Sources.health.http_status", &[("status", "404")]),
            "HTTP 404"
        );
        assert_eq!(ts("Sources.health.network_error"), "网络错误");
        assert_eq!(ts("Sources.error.load_failed"), "规则加载失败");
        assert_eq!(ts("Sources.empty.title"), "暂未导入任何书源");
        assert_eq!(ts("Sources.action.add"), "添加");
        assert_eq!(ts("Sources.action.health_check"), "测速");
        assert_eq!(ts("Sources.action.delete"), "删除");
        assert_eq!(ts("Sources.tag.proxy"), "代理");
        assert_eq!(ts("Sources.delete_dialog.title"), "确认删除书源");
        assert_eq!(
            ts_fmt("Sources.delete_dialog.message", &[("source_id", "42")]),
            "确定要删除书源 #42 吗？此操作无法撤销。"
        );
        assert_eq!(ts("Sources.delete_dialog.cancel"), "取消");
        assert_eq!(
            ts_fmt(
                "Sources.import.result",
                &[("inserted", "5"), ("skipped", "2")]
            ),
            "已导入 5 个，跳过 2 个重复"
        );
        assert_eq!(ts("Sources.add_source.dialog_title"), "选择书源 JSON 文件");
        assert_eq!(ts("Sources.add_source.filter_json"), "JSON 规则文件");
        assert_eq!(ts("Sources.add_source.filter_all"), "所有文件");

        // ---- Search zh-CN ----
        assert_eq!(ts("Search.page_title"), "搜索下载");
        assert_eq!(
            ts("Search.page_subtitle"),
            "按书名 / 作者搜索；支持单源或全源聚合搜索"
        );
        assert_eq!(ts("Search.filter.placeholder"), "书名 / 作者");
        assert_eq!(ts("Search.source.label"), "书源");
        assert_eq!(ts("Search.source.aggregate"), "聚合搜索");
        assert_eq!(ts("Search.action.search"), "搜索");
        assert_eq!(ts("Search.action.select_chapters"), "选章");
        assert_eq!(ts("Search.action.select_chapters_unavailable"), "功能即将推出");
        assert_eq!(ts("Search.action.download_whole"), "全本");
        assert_eq!(ts("Search.action.download_started"), "已派下载任务");
        assert_eq!(ts("Search.empty.title"), "输入关键词开始搜索");
        assert_eq!(
            ts("Search.empty.subtitle"),
            "按书名或作者搜索；选单源可减少网络请求。"
        );
        assert_eq!(ts("Search.source_status.pending"), "等待");
        assert_eq!(ts("Search.source_status.format"), "条");
        assert_eq!(ts("Search.result.author"), "作者");
        assert_eq!(ts("Search.result.latest_chapter"), "最新");
        assert_eq!(ts("Search.result.unknown_author"), "(未知作者)");
        assert_eq!(ts("Search.result.no_latest"), "(无最新章节)");
        assert_eq!(ts_fmt("Search.result.source", &[("id", "3")]), "源 #3");

        // ---------- zh-HK ----------
        rust_i18n::set_locale("zh-HK");
        assert_eq!(ts("Nav.search"), "搜尋下載");
        assert_eq!(ts("App.title"), "So Novel");
        assert_eq!(
            ts("Settings.choose_download_dir_dialog_title"),
            "選擇下載目錄"
        );

        // ---- Library zh-HK ----
        assert_eq!(ts("Library.page_title"), "本地書庫");
        assert_eq!(ts("Library.download_path_label"), "下載目錄");
        assert_eq!(ts("Library.filter_placeholder"), "按檔案名稱過濾…");
        assert_eq!(ts("Library.filter_option_all"), "全部");
        assert_eq!(ts("Library.action_open"), "打開");
        assert_eq!(ts("Library.action_reveal"), "位置");
        assert_eq!(ts("Library.action_delete"), "刪除");
        assert_eq!(ts("Library.empty_title"), "本地書庫為空");
        assert_eq!(
            ts("Library.empty_subtitle"),
            "下載一本書後，它會自動出現在這裡。"
        );
        assert_eq!(ts("Library.scan_failed"), "掃描失敗");
        assert_eq!(ts("Library.delete_dialog.title"), "確認刪除");
        assert_eq!(
            ts_fmt("Library.delete_dialog.message", &[("file_name", "foo.epub")]),
            "確定要刪除 \"foo.epub\" 嗎？此操作無法撤銷。"
        );
        assert_eq!(ts("Library.delete_dialog.cancel_button"), "取消");
        assert_eq!(ts("Library.fallback_unknown_filename"), "(未知檔案名稱)");
        assert_eq!(ts("Library.time.unknown"), "(未知)");
        assert_eq!(ts("Library.time.invalid"), "(無效時間)");
        assert_eq!(ts("Library.time.format_failed"), "(格式化失敗)");

        // ---- Sources zh-HK ----
        assert_eq!(ts("Sources.page_title"), "書源管理");
        assert_eq!(
            ts("Sources.page_subtitle"),
            "啟用/停用、連通性偵測、JSON 匯入"
        );
        assert_eq!(ts("Sources.filter.placeholder"), "按名稱或 URL 過濾…");
        assert_eq!(ts("Sources.status.all"), "全部");
        assert_eq!(ts("Sources.status.enabled"), "啟用");
        assert_eq!(ts("Sources.status.disabled"), "停用");
        assert_eq!(ts("Sources.stat.total"), "總數");
        assert_eq!(ts("Sources.stat.available"), "可用");
        assert_eq!(ts("Sources.health.progress"), "已偵測");
        assert_eq!(ts("Sources.health.not_tested"), "未測");
        assert_eq!(ts("Sources.health.error"), "錯誤");
        assert_eq!(
            ts_fmt("Sources.health.http_status", &[("status", "404")]),
            "HTTP 404"
        );
        assert_eq!(ts("Sources.health.network_error"), "網路錯誤");
        assert_eq!(ts("Sources.error.load_failed"), "規則載入失敗");
        assert_eq!(ts("Sources.empty.title"), "暫未匯入任何書源");
        assert_eq!(ts("Sources.action.add"), "添加");
        assert_eq!(ts("Sources.action.health_check"), "測速");
        assert_eq!(ts("Sources.action.delete"), "刪除");
        assert_eq!(ts("Sources.tag.proxy"), "代理");
        assert_eq!(ts("Sources.delete_dialog.title"), "確認刪除書源");
        assert_eq!(
            ts_fmt("Sources.delete_dialog.message", &[("source_id", "42")]),
            "確定要刪除書源 #42 嗎？此操作無法撤銷。"
        );
        assert_eq!(ts("Sources.delete_dialog.cancel"), "取消");
        assert_eq!(
            ts_fmt(
                "Sources.import.result",
                &[("inserted", "5"), ("skipped", "2")]
            ),
            "已匯入 5 個，跳過 2 個重複"
        );
        assert_eq!(ts("Sources.add_source.dialog_title"), "選擇書源 JSON 檔案");
        assert_eq!(ts("Sources.add_source.filter_json"), "JSON 規則檔案");
        assert_eq!(ts("Sources.add_source.filter_all"), "所有檔案");

        // ---- Search zh-HK ----
        assert_eq!(ts("Search.page_title"), "搜尋下載");
        assert_eq!(
            ts("Search.page_subtitle"),
            "按書名 / 作者搜尋；支援單源或全源聚合搜尋"
        );
        assert_eq!(ts("Search.filter.placeholder"), "書名 / 作者");
        assert_eq!(ts("Search.source.label"), "書源");
        assert_eq!(ts("Search.source.aggregate"), "聚合搜尋");
        assert_eq!(ts("Search.action.search"), "搜尋");
        assert_eq!(ts("Search.action.select_chapters"), "選章");
        assert_eq!(ts("Search.action.select_chapters_unavailable"), "功能即將推出");
        assert_eq!(ts("Search.action.download_whole"), "全本");
        assert_eq!(ts("Search.action.download_started"), "已派下載任務");
        assert_eq!(ts("Search.empty.title"), "輸入關鍵詞開始搜尋");
        assert_eq!(
            ts("Search.empty.subtitle"),
            "按書名或作者搜尋；選單源可減少網絡請求。"
        );
        assert_eq!(ts("Search.source_status.pending"), "等待");
        assert_eq!(ts("Search.source_status.format"), "條");
        assert_eq!(ts("Search.result.author"), "作者");
        assert_eq!(ts("Search.result.latest_chapter"), "最新");
        assert_eq!(ts("Search.result.unknown_author"), "(未知作者)");
        assert_eq!(ts("Search.result.no_latest"), "(無最新章節)");
        assert_eq!(ts_fmt("Search.result.source", &[("id", "3")]), "源 #3");

        // 恢复 en —— 避免污染其他 lib 测试（虽然 i18n locale 是 crate 全局，
        // 但其他测试模块不依赖 locale）
        rust_i18n::set_locale("en");
    }
}
