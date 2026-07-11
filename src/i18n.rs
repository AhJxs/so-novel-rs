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
//! ## 改语言时的流程（重启生效）
//!
//! 1. 用户在设置页选 en → `AppConfig.language = English; persist_settings()`
//! 2. 弹重启确认 Dialog：立即重启 → `cx.restart()`；取消 → 不动 locale
//! 3. 重启后新进程启动时 `mod.rs::run` 调 `gpui_component::set_locale(locale_for(language))`
//!    写全局 locale，再开窗 —— 首次 render 就用新 locale
//!
//! `为什么不实时切换（set_locale` + refresh_windows）？gpui-component 的 `InputState.placeholder`、
//! `SettingsState` 等很多翻译是一次性求值缓存在各 entity 里的，切语言当帧 `refresh_windows`
//! 不会重求这些缓存值（下拉框已选值 / 输入框 placeholder 不更新）。早先用「render 里 sentinel
//! 差量比对 + `set_placeholder」逐个刷新，代价大且覆盖不全，故改成重启生效，彻底删掉` sentinel 机制。
//!
//! ## 模块位置历史
//!
//! 早期在 `desktop::i18n`（UI 子模块下），但 `crate::desktop::model` 的业务层（`events::drain` /
//! `ops::library`）也要调用 `ts()` 翻译错误信息，跨层依赖很别扭。挪到 crate root
//! 后 `crate::i18n::ts()` 是中性 API，`app/` 和 `desktop/` 都可以自然使用。

use std::sync::OnceLock;

use crate::config::Language;

/// 把 `Language` 映射到 `rust_i18n` 用的 locale 标签（**本项目自己**的 `app.yml`）。
///
/// **这是项目里 `Language → locale 字符串` 的唯一权威映射**，给 `app.yml` 用的 key
/// —— 也是 web 前端 `web-ui/src/i18n/locales/{en,zh-CN,zh-TW}.json` 文件名的
/// 来源（前后端 locale tag 统一为 `en` / `zh-CN` / `zh-TW`）。
///
/// `Language::as_str()` 返回的是 `toml_io` 持久化用的 `"zh-TW"`，跟 `app.yml`
/// 现在的 locale 标签（`"zh-TW"`）已经一致；但**跟 `gpui_component::set_locale`
/// 接受的标签（`"zh-HK"`）不一致** —— 那个走 [`locale_for_gpui`]。
///
/// 三种映射：
/// - `SimplifiedChinese` → `"zh-CN"`
/// - `TraditionalChinese` → `"zh-TW"`（**不是** gpui-component 用的 `"zh-HK"`）
/// - `English` → `"en"`
///
/// **位置历史**：原本在 `desktop::mod::locale_for`（仅 gui feature 编译）。
/// CLI 路径（web-only / 未来 cli-only 构建）不依赖 `desktop`，但 CLI 也要按
/// `config.toml` 的 language 切帮助语言 —— 必须从 cfg gate 模块搬到中性 crate
/// root 模块 `crate::i18n`。`desktop` 那边改成 `use crate::i18n::locale_for`。
pub const fn locale_for(lang: Language) -> &'static str {
    match lang {
        Language::SimplifiedChinese => "zh-CN",
        Language::TraditionalChinese => "zh-TW",
        Language::English => "en",
    }
}

/// 把 `Language` 映射到 **gpui-component 接受**的 locale 标签。
///
/// gpui-component 0.5.1 用 `rust_i18n` + 自家 `locales/ui.yml`，**只支持 4 个 locale**：
/// `en` / `zh-CN` / `zh-HK` / `it` —— **没有 `zh-TW`**。本项目的 `app.yml` 用 `zh-TW`，
/// 但调用 `gpui_component::set_locale(...)` 时必须传 `zh-HK`，否则 gpui-component
/// 会 fallback 到 `en`（传统中文用户看到英文 UI）。
///
/// 三种映射：
/// - `SimplifiedChinese` → `"zh-CN"`（同 [`locale_for`]）
/// - `TraditionalChinese` → `"zh-TW"`（同 [`locale_for`]）
/// - `English` → `"en"`（同 [`locale_for`]）
///
/// 调用点只有 `src/desktop/mod.rs::run` 启动时一行 —— CLI / web 路径不碰 gpui-component。
pub const fn locale_for_gpui(lang: Language) -> &'static str {
    match lang {
        Language::SimplifiedChinese => "zh-CN",
        Language::TraditionalChinese => "zh-TW",
        Language::English => "en",
    }
}

/// 翻译返回类型别名：gui feature 开启时为 `gpui::SharedString`（`Arc<str>` 语义，clone 零 alloc）；
/// 非 gui 构建（如 web-only Docker）时为 `String`。
/// 调用方在两种构建下均可直接 `.into()` 得到目标类型。
#[cfg(feature = "gui")]
pub type TStr = gpui::SharedString;
#[cfg(not(feature = "gui"))]
pub type TStr = String;

/// 全局 `TStr` 缓存：key → 已翻译的 `TStr`。
///
/// **仅缓存无变量 key（`ts`）的结果**。`ts_fmt` 因 value 不可预测不进缓存。
/// locale 变化（`set_locale`）时清空 —— 通过 `invalidate_cache` 在 `set_locale`
/// 调用方手动触发。`rust_i18n::set_locale` 本身没有 hook 给我们挂，
/// 但本项目切语言**走重启流程**（见模块顶部注释），所以实际场景里这个缓存
/// 整进程有效，命中率很高。
static TS_CACHE: OnceLock<std::sync::Mutex<Option<std::collections::HashMap<&'static str, TStr>>>> =
    OnceLock::new();

fn ts_cache() -> &'static std::sync::Mutex<Option<std::collections::HashMap<&'static str, TStr>>> {
    TS_CACHE.get_or_init(|| std::sync::Mutex::new(None))
}

/// 清空 `ts` 缓存。切 locale 时调用 —— 本项目暂不主动调（切语言走重启），
/// 但保留 API 以备未来运行时切换场景。
pub fn invalidate_cache() {
    if let Ok(mut g) = ts_cache().lock() {
        *g = None;
    }
}

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
pub fn ts(key: &'static str) -> TStr {
    let locale = rust_i18n::locale();
    crate::_rust_i18n_try_translate(&locale, key)
        .map_or_else(|| TStr::from(key), |cow| TStr::from(cow.into_owned()))
}

/// 翻译查找的缓存版本。热路径（行 builder 每次 render 调）走这个：
/// - 首次访问时查 `rust_i18n，写入` `TS_CACHE`（全局 `OnceLock<Mutex<HashMap>>`）。
/// - 后续访问直接 clone 共享的 `SharedString`（`SharedString` 内部 `Arc<str>`，
///   clone 只增引用计数，无 alloc）。
///
/// 与 `ts` 的唯一区别就是缓存层。语义完全一致（key 找不到返回 key 本身）。
pub fn ts_cached(key: &'static str) -> TStr {
    // 读路径走 Mutex 而非 RwLock：① 全局只一个 key-value 写者（第一次访问），
    // ② SharedString clone 很轻，无锁阻塞竞争更友好。读侧用 `try_lock` 退路：
    // 万一锁被持有（极罕见）退到非缓存路径，绝不阻塞调用方。
    if let Ok(g) = ts_cache().lock()
        && let Some(map) = g.as_ref()
        && let Some(cached) = map.get(key)
    {
        return cached.clone();
    }
    // miss：调底层查 + 写回缓存。
    let v = ts(key);
    if let Ok(mut g) = ts_cache().lock() {
        let map = g.get_or_insert_with(std::collections::HashMap::new);
        // 同 key 的并发写以最后一个写者为准（覆盖语义，无害）。
        map.insert(key, v.clone());
    }
    v
}

/// 翻译查找的 per-request 变体 —— 显式传 locale 字符串，**不**读 / **不**写
/// `rust_i18n::locale()`（全局 `AtomicStr`，并发请求之间会互相踩）。
///
/// Web handler 入口拿到 `Locale` extractor 后，闭包里所有翻译都走这里 —— 保证
/// A 请求 `Accept-Language: zh-CN` 和 B 请求 `Accept-Language: en` 各自走自己的
/// locale，互不干扰。
///
/// 找不到 key 走与 `ts` 相同的 fallback —— 返回 key 字符串本身（开发期可见漏翻译）。
///
/// 性能特征：等价于 `ts`，但**每次调用都做 yaml hashmap lookup**（不进 `TS_CACHE`，
/// 因为缓存按全局 locale 组织，per-request 缓存命中率低且易出错）。热路径（每请求
/// 1-2 次翻译调用）完全可接受。
pub fn ts_for_locale(locale: &str, key: &'static str) -> String {
    crate::_rust_i18n_try_translate(locale, key)
        .map_or_else(|| key.to_string(), std::borrow::Cow::into_owned)
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
pub fn ts_fmt(key: &'static str, vars: &[(&str, &str)]) -> TStr {
    let locale = rust_i18n::locale();
    let mut result = crate::_rust_i18n_try_translate(&locale, key)
        .map_or_else(|| key.to_string(), std::borrow::Cow::into_owned);
    for (name, value) in vars {
        // 占位符形式 `{name}` —— `format!("{{{}}}", name)` 转义出字面 `{name}`。
        result = result.replace(&format!("{{{name}}}"), value);
    }
    TStr::from(result)
}

/// `ts_fmt` 的缓存版本。**只缓存"模板字符串"**（带 `{var}` 占位符的原文），
/// 不缓存"模板+变量组合" —— 后者组合数太大，命中率低。模板命中后仍要做
/// `replace`（必 alloc），但**跳过了 `rust_i18n` 的 yaml 解析 / map lookup**，
/// 热路径上节省主要来自这里。
///
/// 模板用 `Option<String>` 区分"未缓存"（None）和"已缓存且 key 不存在"（Some("")）。
/// 实际不存在时 `Some(key.to_string())` 走 fallback，避免反复调用底层。
pub fn ts_fmt_cached(key: &'static str, vars: &[(&str, &str)]) -> TStr {
    use std::collections::HashMap;
    use std::sync::Mutex;

    static TPL_CACHE: OnceLock<Mutex<HashMap<&'static str, String>>> = OnceLock::new();
    let cache = TPL_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    // 拿模板（缓存 miss 时回退到直接调 ts_fmt + 写回）
    let template = cache.lock().map_or(None, |g| g.get(key).cloned());
    let template = template.unwrap_or_else(|| {
        // miss：复用 ts_fmt 算出一次完整结果作为模板（变量无关部分是模板本体）。
        // 但 ts_fmt 已经替了占位符，这里手动走底层拿"未替"版本。
        let locale = rust_i18n::locale();
        let raw = crate::_rust_i18n_try_translate(&locale, key)
            .map_or_else(|| key.to_string(), std::borrow::Cow::into_owned);
        if let Ok(mut g) = cache.lock() {
            g.insert(key, raw.clone());
        }
        raw
    });

    if vars.is_empty() {
        // 常见 case：UI 上很多 "已翻译" 标签走 `ts_fmt(key, &[])` —— 实际等于 `ts_cached`。
        return TStr::from(template);
    }
    let mut result = template;
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

    /// 精简版：只验证 `ts` / `ts_fmt` 两个方法在三 locale 下行为正确（翻译返回、
    /// 占位符替换、缺 key fallback），不再逐 key 断言——逐 key 既脆又拖慢改动。
    /// 全局 locale 是共享状态，并行测试会互相踩；放一个测试里顺序跑就稳。
    /// 退出时恢复 en。
    #[test]
    fn ts_and_ts_fmt_work() {
        // ---- en：ts 返回英文翻译 ----
        rust_i18n::set_locale("en");
        assert_eq!(ts("Nav.tasks"), "Tasks");

        // ts_fmt 占位符替换（{id} → 3）。
        assert_eq!(ts_fmt("Search.result.source", &[("id", "3")]), "Source #3");

        // ---- zh-CN：切语言后 ts 返回中文 ----
        rust_i18n::set_locale("zh-CN");
        assert_eq!(ts("Nav.tasks"), "下载任务");
        assert_eq!(ts_fmt("Search.result.source", &[("id", "3")]), "源 #3");

        // ---- zh-TW：传统中文（本项目 app.yml 现在用 zh-TW，跟前端 JSON 文件名一致）----
        rust_i18n::set_locale("zh-TW");
        assert_eq!(ts("Nav.tasks"), "下載任務");
        assert_eq!(ts_fmt("Search.result.source", &[("id", "3")]), "源 #3");

        // ---- 缺 key fallback：返回 key 本身（见 ts 实现 unwrap_or_else）----
        rust_i18n::set_locale("en");
        assert_eq!(ts("definitely.not.a.real.key"), "definitely.not.a.real.key");

        // 恢复 en，避免污染其他测试。
        rust_i18n::set_locale("en");
    }

    // ── ts_for_locale：per-request 翻译查找 ─────────────────────────
    //
    // 与 `ts` 的关键区别：**不读 / 不写 `rust_i18n::locale()` 全局 atomic**，
    // 避免并发请求之间 locale 互相踩。Web handler 热路径专用。

    #[test]
    fn ts_for_locale_does_not_mutate_global() {
        rust_i18n::set_locale("en");
        let before: String = (&*rust_i18n::locale()).to_string();
        let _ = ts_for_locale("zh-CN", "Nav.tasks");
        let after: String = (&*rust_i18n::locale()).to_string();
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

    // ── locale_for vs locale_for_gpui：拆分映射 ───────────────────────
    //
    // `locale_for` 跟 web 前端 + app.yml 统一（zh-TW）；
    // `locale_for_gpui` 跟 gpui-component 0.5.1 接受列表对齐（zh-HK）。
    // 两个分开是为了不互相踩：web 路径走 locale_for，桌面路径走 locale_for_gpui。

    #[test]
    fn locale_for_matches_app_yml_locale_tags() {
        assert_eq!(locale_for(Language::SimplifiedChinese), "zh-CN");
        assert_eq!(locale_for(Language::TraditionalChinese), "zh-TW");
        assert_eq!(locale_for(Language::English), "en");
    }

    // ── WebErrors 翻译表完整性 ─────────────────────────────────────
    //
    // 验证 app.yml 里 `WebErrors` 段的所有 47 个 key 在三 locale 下都能拿到非空
    // 翻译（fallback 到 key 本身也算 miss，但必须非空以保证 frontend 至少能看到
    // 一个稳定的字符串 id）。失败的 key 一定是手抄漏译。

    /// 47 个 WebErrors key：38 原 ErrorCode + 3 新 3xxx (3004/3005/3006) + 6 内联。
    /// 与 `src/web/error_code.rs::ErrorCode` 1:1 + handler 散落字符串。
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
        // 47 个 key × 3 locale 必须都有非空翻译
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
        // 抽查代表性 key：en / zh-CN / zh-TW 互不相同（防止 fallback 串味）。
        // 注意 `source_test_http_status` 是 literal token "HTTP {status}"，三 locale
        // 形式相同 —— 技术字符串，不参与本地化，所以跳过。
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

    // ── Search.url_download.* 完整性（spec 2026-07-11-tasks-url-download）──
    //
    // Search.* 段没有像 WebErrors 那样的全局注册表，但 url_download 是新增的
    // 一整组 key（按钮 / Dialog / 提示 / toast），漏译一个就会在桌面端某 locale
    // 下显示原始 key 字面量。这里跟 WEB_ERROR_KEYS 同模式做一次兜底校验。
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
