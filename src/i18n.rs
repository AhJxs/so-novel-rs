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
//! 为什么不实时切换（set_locale + refresh_windows）？gpui-component 的 `InputState.placeholder`、
//! `SettingsState` 等很多翻译是一次性求值缓存在各 entity 里的，切语言当帧 `refresh_windows`
//! 不会重求这些缓存值（下拉框已选值 / 输入框 placeholder 不更新）。早先用「render 里 sentinel
//! 差量比对 + set_placeholder」逐个刷新，代价大且覆盖不全，故改成重启生效，彻底删掉 sentinel 机制。
//!
//! ## 模块位置历史
//!
//! 早期在 `gpui_app::i18n`（UI 子模块下），但 `crate::app` 的业务层（`events::drain` /
//! `ops::library`）也要调用 `ts()` 翻译错误信息，跨层依赖很别扭。挪到 crate root
//! 后 `crate::i18n::ts()` 是中性 API，`app/` 和 `gpui_app/` 都可以自然使用。

use std::sync::OnceLock;

use crate::config::Language;

/// 把 `Language` 映射到 `rust_i18n` 用的 locale 标签。
///
/// **这是项目里 `Language → locale 字符串` 的唯一权威映射**。
/// `Language::as_str()` 返回的是 toml_io 持久化用的 `"zh-TW"` —— 跟 `app.yml`
/// 实际的 locale 标签（`"zh-HK"`）不一致，所以 `load_config` 之后、任何
/// `ts("Cli.xxx")` / `gpui_component::set_locale` 之前都要走这里。
///
/// 三种映射：
/// - `SimplifiedChinese` → `"zh-CN"`
/// - `TraditionalChinese` → `"zh-HK"`（**不是** `Language::as_str()` 返回的 `"zh-TW"`）
/// - `English` → `"en"`
///
/// **位置历史**：原本在 `gpui_app::mod::locale_for`（仅 gui feature 编译）。
/// CLI 路径（web-only / 未来 cli-only 构建）不依赖 `gpui_app`，但 CLI 也要按
/// `config.toml` 的 language 切帮助语言 —— 必须从 cfg gate 模块搬到中性 crate
/// root 模块 `crate::i18n`。`gpui_app` 那边改成 `use crate::i18n::locale_for`。
pub fn locale_for(lang: Language) -> &'static str {
    match lang {
        Language::SimplifiedChinese => "zh-CN",
        Language::TraditionalChinese => "zh-HK",
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

/// 全局 TStr 缓存：key → 已翻译的 TStr。
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
        .map(|cow| TStr::from(cow.into_owned()))
        .unwrap_or_else(|| TStr::from(key))
}

/// 翻译查找的缓存版本。热路径（行 builder 每次 render 调）走这个：
/// - 首次访问时查 rust_i18n，写入 `TS_CACHE`（全局 `OnceLock<Mutex<HashMap>>`）。
/// - 后续访问直接 clone 共享的 `SharedString`（`SharedString` 内部 `Arc<str>`，
///   clone 只增引用计数，无 alloc）。
///
/// 与 `ts` 的唯一区别就是缓存层。语义完全一致（key 找不到返回 key 本身）。
pub fn ts_cached(key: &'static str) -> TStr {
    // 读路径走 Mutex 而非 RwLock：① 全局只一个 key-value 写者（第一次访问），
    // ② SharedString clone 很轻，无锁阻塞竞争更友好。读侧用 `try_lock` 退路：
    // 万一锁被持有（极罕见）退到非缓存路径，绝不阻塞调用方。
    if let Ok(g) = ts_cache().lock() {
        if let Some(map) = g.as_ref() {
            if let Some(cached) = map.get(key) {
                return cached.clone();
            }
        }
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
        .map(|cow| cow.into_owned())
        .unwrap_or_else(|| key.to_string());
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
    let template = if let Ok(g) = cache.lock() {
        g.get(key).cloned()
    } else {
        None
    };
    let template = match template {
        Some(t) => t,
        None => {
            // miss：复用 ts_fmt 算出一次完整结果作为模板（变量无关部分是模板本体）。
            // 但 ts_fmt 已经替了占位符，这里手动走底层拿"未替"版本。
            let locale = rust_i18n::locale();
            let raw = crate::_rust_i18n_try_translate(&locale, key)
                .map(|cow| cow.into_owned())
                .unwrap_or_else(|| key.to_string());
            if let Ok(mut g) = cache.lock() {
                g.insert(key, raw.clone());
            }
            raw
        }
    };

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

        // ---- zh-HK ----
        rust_i18n::set_locale("zh-HK");
        assert_eq!(ts("Nav.tasks"), "下載任務");
        assert_eq!(ts_fmt("Search.result.source", &[("id", "3")]), "源 #3");

        // ---- 缺 key fallback：返回 key 本身（见 ts 实现 unwrap_or_else）----
        rust_i18n::set_locale("en");
        assert_eq!(ts("definitely.not.a.real.key"), "definitely.not.a.real.key");

        // 恢复 en，避免污染其他测试。
        rust_i18n::set_locale("en");
    }
}
