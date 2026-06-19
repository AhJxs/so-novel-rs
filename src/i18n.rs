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
