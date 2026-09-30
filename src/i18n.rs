//! 走 `rust_i18n` 官方 `t!` 宏（`gpui_kit::component` 同款机制），调用方写字符串字面量 key：
//! `ts("Settings.item.theme")`，翻译表在 `locales/app.yml`（编译期嵌入，YAML 顶层大写）。
//!
//! `ts*` 家族只在官方宏外补三件事：`Cow<str> → TStr`、热路径缓存 [`ts_cached`]、
//! **固定用 `{var}` 占位符**的手动替换（[`ts_fmt`]；官方 `t!` args 只认 `%{var}`）。
//!
//! 与组件库**共享全局 locale**：两套 i18n 实例各管各的 key 表，但 `set_locale` 写的是同一个
//! `CURRENT_LOCALE`，所以一次 `set_locale` 同时影响双方。改语言**重启生效**：组件把文案
//! 一次性缓存在各 entity 里，切语言当帧 `refresh_windows` 不会重求这些缓存值。

use std::sync::OnceLock;

use crate::config::Language;

/// 把 `Language` 映射到本项目 `app.yml` 用的 locale 标签。
///
/// **`Language → locale 字符串` 的唯一权威映射**，也是 web 前端
/// `web-ui/src/i18n/locales/{en,zh-CN,zh-TW}.json` 文件名的来源（前后端 locale tag 统一）。
/// `TraditionalChinese` → `"zh-TW"`，**不是** `gpui_kit::component` 用的 `"zh-HK"`。
///
/// CLI / web-only 构建不依赖 `desktop`，但也要按 `config.toml` 的 language 切帮助语言，
/// 所以本函数必须留在 cfg gate 之外的 crate root。
pub const fn locale_for(lang: Language) -> &'static str {
    match lang {
        Language::SimplifiedChinese => "zh-CN",
        Language::TraditionalChinese => "zh-TW",
        Language::English => "en",
    }
}

/// 把 `Language` 映射到 **`gpui_kit::component` 接受**的 locale 标签。
///
/// 组件库用 `rust_i18n` + 自带的 `locales/ui.yml`，其 `zh-TW` key 已全覆盖，标签与本项目
/// `app.yml` 统一，因此映射与 [`locale_for`] 完全一致。两者分开是为了不互相踩：
/// web 路径走 [`locale_for`]，桌面路径走本函数（调用点只有 `src/desktop/mod.rs::run` 一行）。
pub const fn locale_for_gpui(lang: Language) -> &'static str {
    match lang {
        Language::SimplifiedChinese => "zh-CN",
        Language::TraditionalChinese => "zh-TW",
        Language::English => "en",
    }
}

/// 翻译返回类型别名：gui feature 下为 `gpui_kit::SharedString`（`Arc<str>` 语义，clone 零 alloc），
/// 非 gui 构建（如 web-only Docker）为 `String`。两种构建下调用方都可直接 `.into()`。
#[cfg(feature = "gui")]
pub type TStr = gpui_kit::SharedString;
#[cfg(not(feature = "gui"))]
pub type TStr = String;

/// 全局 `TStr` 缓存：key → 已翻译的 `TStr`。
///
/// **仅缓存无变量 key（`ts`）的结果**；`ts_fmt` 的 value 不可预测，不进缓存。
/// locale 变化时须由调用方调 [`invalidate_cache`] 清空（`rust_i18n::set_locale` 没有 hook），
/// 但本项目切语言走重启流程，实际整进程有效、命中率很高。
static TS_CACHE: OnceLock<std::sync::Mutex<Option<std::collections::HashMap<&'static str, TStr>>>> =
    OnceLock::new();

fn ts_cache() -> &'static std::sync::Mutex<Option<std::collections::HashMap<&'static str, TStr>>> {
    TS_CACHE.get_or_init(|| std::sync::Mutex::new(None))
}

/// 清空 `ts` 缓存。切 locale 时调用 —— 本项目切语言走重启、暂不主动调，保留给未来运行时切换。
pub fn invalidate_cache() {
    if let Ok(mut g) = ts_cache().lock() {
        *g = None;
    }
}

/// 翻译查找 —— 走官方 `t!` 宏，再 `Cow<'_, str> → TStr`。
///
/// 用宏而非手写 `_rust_i18n_try_translate`：把「locale 临时值怎么借用」交回上游维护；
/// 查不到时回落成 key 字符串本身（开发期可见漏翻译）。`t!` 也接受非字面量 key，
/// 故 `&'static str` 形参合法（只有开 `_minify_key` 才要求字面量，本项目没开）。
///
/// 前提：`rust_i18n::i18n!("locales")` 必须在 crate root 调一次（见 `src/lib.rs`），
/// `t!` 展开出的 `crate::_rust_i18n_t...` 才是真正的查找后端。
pub fn ts(key: &'static str) -> TStr {
    TStr::from(rust_i18n::t!(key))
}

/// 翻译查找的缓存版本，热路径（行 / 按钮 builder 每次 render 调）走这个。
///
/// 首次访问查 `rust_i18n` 并写入 `TS_CACHE`，之后直接 clone 共享 `SharedString`
/// （只增引用计数，无 alloc）；语义与 [`ts`] 完全一致，实测约 2.1× 提速。
///
/// `Mutex` 而非 `RwLock`：全局只有「首次访问」一种写者、临界区极短，`RwLock` 开销不划算。
/// 读侧用 `try_lock`，万一锁被持有则退到非缓存路径 [`ts`]，**绝不阻塞渲染线程**。
pub fn ts_cached(key: &'static str) -> TStr {
    if let Ok(g) = ts_cache().try_lock()
        && let Some(map) = g.as_ref()
        && let Some(cached) = map.get(key)
    {
        return cached.clone();
    }
    // miss（或读锁没抢到）：查一次 + 写回缓存。
    let v = ts(key);
    if let Ok(mut g) = ts_cache().lock() {
        let map = g.get_or_insert_with(std::collections::HashMap::new);
        // 同 key 的并发写以最后一个写者为准（覆盖语义，无害）。
        map.insert(key, v.clone());
    }
    v
}

/// 翻译查找的 per-request 变体 —— 显式传 locale，**不**读 / 不写全局 `rust_i18n::locale()`。
///
/// Web handler 入口拿到 `Locale` extractor 后，闭包里所有翻译都走这里，保证并发请求
/// 各自用自己的 locale、互不干扰。等价于 [`ts`]，但每次调用都做 yaml lookup
/// （不进 `TS_CACHE`：缓存按全局 locale 组织，per-request 命中率低且易出错）。
pub fn ts_for_locale(locale: &str, key: &'static str) -> String {
    rust_i18n::t!(key, locale = locale).into_owned()
}

/// 翻译查找 + 变量替换 —— [`ts`] 的扩展，替换 `{var}` 占位符。
///
/// 用法：`ts_fmt("Library.delete_dialog.message", &[("file_name", "foo.epub")])`
/// 对应 YAML：
/// ```yaml
/// Library:
///   delete_dialog:
///     message: "Are you sure you want to delete \"{file_name}\"? ..."
/// ```
///
/// 查表仍走官方 `t!`（不带 args），占位符替换手动做：`t!` 的 args 只认 `%{name}`，
/// 而 `locales/app.yml` 全用 `{name}`；改语法要动整份翻译表 + 全部调用点，不划算。
///
/// **安全前提：替换的 value 不能含 `{` 或 `}`**，否则会误替换或注入新占位符。所有 caller
/// 传的都是内部数据（PathBuf、enum 名等），若未来 value 可能含用户输入，必须先 escape。
pub fn ts_fmt(key: &'static str, vars: &[(&str, &str)]) -> TStr {
    let mut result = rust_i18n::t!(key).into_owned();
    for (name, value) in vars {
        // 占位符形式 `{name}` —— `format!("{{{}}}", name)` 转义出字面 `{name}`。
        result = result.replace(&format!("{{{name}}}"), value);
    }
    TStr::from(result)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    /// 只验证 `ts` / `ts_fmt` 在三 locale 下的行为（翻译返回、占位符替换、缺 key fallback），
    /// 不逐 key 断言——逐 key 既脆又拖慢改动。
    /// 全局 locale 是共享状态，并行测试会互相踩；放一个测试里顺序跑就稳，退出时恢复 en。
    #[test]
    fn ts_and_ts_fmt_work() {
        rust_i18n::set_locale("en");
        assert_eq!(ts("Nav.tasks"), "Tasks");

        assert_eq!(ts_fmt("Search.result.source", &[("id", "3")]), "Source #3");

        rust_i18n::set_locale("zh-CN");
        assert_eq!(ts("Nav.tasks"), "下载任务");
        assert_eq!(ts_fmt("Search.result.source", &[("id", "3")]), "源 #3");

        rust_i18n::set_locale("zh-TW");
        assert_eq!(ts("Nav.tasks"), "下載任務");
        assert_eq!(ts_fmt("Search.result.source", &[("id", "3")]), "源 #3");

        rust_i18n::set_locale("en");
        assert_eq!(ts("definitely.not.a.real.key"), "definitely.not.a.real.key");

        // 恢复 en，避免污染其他测试。
        rust_i18n::set_locale("en");
    }

    #[test]
    fn ts_for_locale_does_not_mutate_global() {
        rust_i18n::set_locale("en");
        let before: String = (*rust_i18n::locale()).to_string();
        let _ = ts_for_locale("zh-CN", "Nav.tasks");
        let after: String = (*rust_i18n::locale()).to_string();
        assert_eq!(before, after, "ts_for_locale 不应改全局 locale");
        rust_i18n::set_locale("en");
    }

    #[test]
    fn ts_for_locale_returns_correct_translation_per_locale() {
        assert_eq!(ts_for_locale("en", "Nav.tasks"), "Tasks");
        assert_eq!(ts_for_locale("zh-CN", "Nav.tasks"), "下载任务");
        assert_eq!(ts_for_locale("zh-TW", "Nav.tasks"), "下載任務");
    }

    #[test]
    fn ts_for_locale_falls_back_to_key_on_missing() {
        assert_eq!(
            ts_for_locale("en", "definitely.not.a.real.key"),
            "definitely.not.a.real.key"
        );
    }

    #[test]
    fn ts_for_locale_independent_of_global_locale() {
        // 全局是 en, 但传 zh-CN 应该返回中文 —— 证明不走全局
        rust_i18n::set_locale("en");
        assert_eq!(ts_for_locale("zh-CN", "Nav.tasks"), "下载任务");
        rust_i18n::set_locale("en");
    }

    #[test]
    fn locale_for_matches_app_yml_locale_tags() {
        assert_eq!(locale_for(Language::SimplifiedChinese), "zh-CN");
        assert_eq!(locale_for(Language::TraditionalChinese), "zh-TW");
        assert_eq!(locale_for(Language::English), "en");
    }

    /// `WebErrors` 全部 key：与 `src/web/error_code.rs::ErrorCode` 1:1 + handler 散落字符串。
    const WEB_ERROR_KEYS: &[&str] = &[
        // 1xxx 业务规则
        "WebErrors.book_rule_missing",
        "WebErrors.missing_title_or_author",
        "WebErrors.toc_rule_missing",
        "WebErrors.chapter_rule_missing",
        "WebErrors.empty_content",
        "WebErrors.search_disabled",
        "WebErrors.source_disabled",
        "WebErrors.empty_toc",
        "WebErrors.invalid_range",
        "WebErrors.cancelled",
        // 2xxx 解析/网络
        "WebErrors.book_http",
        "WebErrors.book_cloudflare",
        "WebErrors.book_parse",
        "WebErrors.toc_http",
        "WebErrors.toc_cloudflare",
        "WebErrors.toc_parse",
        "WebErrors.chapter_http",
        "WebErrors.chapter_cloudflare",
        "WebErrors.chapter_parse",
        "WebErrors.search_http",
        "WebErrors.search_cloudflare",
        "WebErrors.search_parse",
        "WebErrors.crawler_client",
        "WebErrors.crawler_io",
        "WebErrors.crawler_export",
        "WebErrors.crawler_book_aggregate",
        "WebErrors.crawler_toc_aggregate",
        // 3xxx 资源
        "WebErrors.not_found",
        "WebErrors.conflict",
        "WebErrors.bad_request",
        "WebErrors.download_path_empty",
        "WebErrors.download_path_not_dir",
        "WebErrors.task_already_finished",
        // 4xxx 内部
        "WebErrors.internal",
        "WebErrors.io_error",
        // 5xxx 导出
        "WebErrors.export_empty_chapters_dir",
        "WebErrors.export_io",
        "WebErrors.export_epub",
        "WebErrors.export_zip",
        "WebErrors.export_encoding",
        "WebErrors.export_pdf",
        // 内联字符串
        "WebErrors.source_not_found",
        "WebErrors.task_not_found",
        "WebErrors.task_cancelled",
        "WebErrors.task_deleted",
        "WebErrors.library_deleted",
        "WebErrors.source_test_http_status",
    ];

    #[test]
    fn web_errors_translated_in_all_three_locales() {
        for &key in WEB_ERROR_KEYS {
            for locale in ["en", "zh-CN", "zh-TW"] {
                let v = ts_for_locale(locale, key);
                assert!(!v.is_empty(), "{key} 在 locale={locale} 翻译为空字符串");
                assert_ne!(v, key, "{key} 在 locale={locale} 缺失（返回 key 本身）");
            }
        }
    }

    #[test]
    fn web_errors_en_zh_cn_zh_tw_differ() {
        // 抽查代表性 key：三 locale 互不相同（防止 fallback 串味）。跳过
        // `source_test_http_status` —— 它是技术字符串 literal token，不参与本地化。
        for key in [
            "WebErrors.book_rule_missing",
            "WebErrors.source_not_found",
            "WebErrors.download_path_not_dir",
            "WebErrors.task_already_finished",
        ] {
            let en = ts_for_locale("en", key);
            let zh_cn = ts_for_locale("zh-CN", key);
            let zh_tw = ts_for_locale("zh-TW", key);
            assert_ne!(en, zh_cn, "{key}: en == zh-CN");
            assert_ne!(en, zh_tw, "{key}: en == zh-TW");
            assert_ne!(zh_cn, zh_tw, "{key}: zh-CN == zh-TW");
        }
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
                let v = ts_for_locale(locale, key);
                assert!(!v.is_empty(), "{key} 在 locale={locale} 翻译为空字符串");
                assert_ne!(v, key, "{key} 在 locale={locale} 缺失（返回 key 本身）");
            }
        }
    }
}
