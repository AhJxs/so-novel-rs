"""
i18n 迁移脚本：把 `i18n::tr(i18n::_KEY[, lang])` → `ts!("Key.path")` 宏。

修复 settings.rs 里的两类问题：
  1. `i18n::tr(i18n::_KEY,` （缺 lang 参数、缺右括号，引用已不存在的旧 API）
  2. `.title(ts("Settings.group.x").items(vec![...])` （`.title()` 缺右括号）

idempotent：重复运行结果相同（基于 i18n::_KEY 字面量）。
"""
import re
import sys
from pathlib import Path

# i18n 旧常量 → 新 ts! 字符串字面量 映射表
KEY_MAP = {
    # Nav
    "_NAV_SEARCH":    "Nav.search",
    "_NAV_TASKS":     "Nav.tasks",
    "_NAV_LIBRARY":   "Nav.library",
    "_NAV_SOURCES":   "Nav.sources",
    "_NAV_SETTINGS":  "Nav.settings",
    # App
    "_APP_TITLE":     "App.title",
    # Settings page
    "_PAGE_GENERAL":  "Settings.page.general",
    "_PAGE_CRAWL":    "Settings.page.crawl",
    "_PAGE_PROXY":    "Settings.page.proxy",
    "_PAGE_ABOUT":    "Settings.page.about",
    # Settings group
    "_GROUP_APPEARANCE": "Settings.group.appearance",
    "_GROUP_NETWORK":    "Settings.group.network",
    "_GROUP_DOWNLOAD":   "Settings.group.download",
    "_GROUP_SOURCE":     "Settings.group.source",
    "_GROUP_CONCURRENCY":"Settings.group.concurrency",
    "_GROUP_RETRY":      "Settings.group.retry",
    "_GROUP_HTTP_PROXY": "Settings.group.http_proxy",
    "_GROUP_COOKIE":     "Settings.group.cookie",
    "_GROUP_INFO":       "Settings.group.info",
    # Settings item
    "_ITEM_THEME":                  "Settings.item.theme",
    "_ITEM_APP_LANG":               "Settings.item.app_lang",
    "_ITEM_GH_PROXY":               "Settings.item.gh_proxy",
    "_ITEM_CF_BYPASS":              "Settings.item.cf_bypass",
    "_ITEM_DOWNLOAD_PATH":          "Settings.item.download_path",
    "_ITEM_DEFAULT_FORMAT":         "Settings.item.default_format",
    "_ITEM_TXT_ENCODING":           "Settings.item.txt_encoding",
    "_ITEM_PRESERVE_CHAPTER_CACHE": "Settings.item.preserve_chapter_cache",
    "_ITEM_ENABLE_PROGRESSBAR":     "Settings.item.enable_progressbar",
    "_ITEM_BOOK_LANG":              "Settings.item.book_lang",
    "_ITEM_SEARCH_LIMIT":           "Settings.item.search_limit",
    "_ITEM_SEARCH_FILTER":          "Settings.item.search_filter",
    "_ITEM_CONCURRENCY":            "Settings.item.concurrency",
    "_ITEM_MIN_INTERVAL":           "Settings.item.min_interval",
    "_ITEM_MAX_INTERVAL":           "Settings.item.max_interval",
    "_ITEM_ENABLE_RETRY":           "Settings.item.enable_retry",
    "_ITEM_MAX_RETRIES":            "Settings.item.max_retries",
    "_ITEM_RETRY_MIN_INTERVAL":     "Settings.item.retry_min_interval",
    "_ITEM_RETRY_MAX_INTERVAL":     "Settings.item.retry_max_interval",
    "_ITEM_PROXY_ENABLED":          "Settings.item.proxy_enabled",
    "_ITEM_PROXY_HOST":             "Settings.item.proxy_host",
    "_ITEM_PROXY_PORT":             "Settings.item.proxy_port",
    "_ITEM_QIDIAN_COOKIE":          "Settings.item.qidian_cookie",
    "_ITEM_VERSION":                "Settings.item.version",
    "_ITEM_CHECK_UPDATE_TITLE":     "Settings.item.check_update",
    "_ITEM_OPEN_GITHUB_TITLE":      "Settings.item.open_github",
    # Settings desc
    "_DESC_THEME":                  "Settings.desc.theme",
    "_DESC_APP_LANG":               "Settings.desc.app_lang",
    "_DESC_GH_PROXY":               "Settings.desc.gh_proxy",
    "_DESC_CF_BYPASS":              "Settings.desc.cf_bypass",
    "_DESC_DOWNLOAD_PATH":          "Settings.desc.download_path",
    "_DESC_DEFAULT_FORMAT":         "Settings.desc.default_format",
    "_DESC_TXT_ENCODING":           "Settings.desc.txt_encoding",
    "_DESC_PRESERVE_CHAPTER_CACHE": "Settings.desc.preserve_chapter_cache",
    "_DESC_ENABLE_PROGRESSBAR":     "Settings.desc.enable_progressbar",
    "_DESC_BOOK_LANG":              "Settings.desc.book_lang",
    "_DESC_SEARCH_LIMIT":           "Settings.desc.search_limit",
    "_DESC_SEARCH_FILTER":          "Settings.desc.search_filter",
    "_DESC_CONCURRENCY":            "Settings.desc.concurrency",
    "_DESC_MIN_INTERVAL":           "Settings.desc.min_interval",
    "_DESC_MAX_INTERVAL":           "Settings.desc.max_interval",
    "_DESC_ENABLE_RETRY":           "Settings.desc.enable_retry",
    "_DESC_MAX_RETRIES":            "Settings.desc.max_retries",
    "_DESC_RETRY_MIN_INTERVAL":     "Settings.desc.retry_min_interval",
    "_DESC_RETRY_MAX_INTERVAL":     "Settings.desc.retry_max_interval",
    "_DESC_PROXY_ENABLED":          "Settings.desc.proxy_enabled",
    "_DESC_PROXY_HOST":             "Settings.desc.proxy_host",
    "_DESC_PROXY_PORT":             "Settings.desc.proxy_port",
    "_DESC_QIDIAN_COOKIE":          "Settings.desc.qidian_cookie",
    "_DESC_VERSION":                "Settings.desc.version",
    "_DESC_CHECK_UPDATE":           "Settings.desc.check_update",
    "_DESC_OPEN_GITHUB":            "Settings.desc.open_github",
    # Settings button
    "_SETTINGS_CHECK_UPDATE":       "Settings.check_update_button",
    "_SETTINGS_OPEN_GITHUB":        "Settings.open_github_button",
    # Settings option
    "_OPT_BOOKLANG_ZH_CN":   "Settings.option.booklang.zh_cn",
    "_OPT_BOOKLANG_ZH_TW":   "Settings.option.booklang.zh_tw",
    "_OPT_BOOKLANG_ZH_HANT": "Settings.option.booklang.zh_hant",
    "_OPT_APPLANG_ZH_CN":    "Settings.option.applang.zh_cn",
    "_OPT_APPLANG_ZH_TW":    "Settings.option.applang.zh_tw",
    "_OPT_APPLANG_EN":       "Settings.option.applang.en",
}


def build_call_re() -> re.Pattern:
    """匹配 `i18n::tr(i18n::_KEY)` 或 `i18n::tr(i18n::_KEY, lang)`。"""
    keys = "|".join(re.escape(k) for k in KEY_MAP)
    # group 1: 整个 call  →  替换成 ts!("X")
    return re.compile(
        r'i18n::tr\(i18n::(' + keys + r')(?:,\s*[^)]*)?\)'
    )


def build_bare_re() -> re.Pattern:
    """匹配 `i18n::tr(i18n::_KEY,` （缺右括号、缺 lang）—— settings.rs 里的破缺调用。"""
    keys = "|".join(re.escape(k) for k in KEY_MAP)
    return re.compile(
        r'i18n::tr\(i18n::(' + keys + r'),'
    )


def build_keyonly_re() -> re.Pattern:
    """匹配 `i18n::_KEY` 单独引用（root.rs 的 `label_key` 用）。"""
    keys = "|".join(re.escape(k) for k in KEY_MAP)
    return re.compile(r'i18n::(' + keys + r')')


def fix_title_parens(text: str) -> tuple[str, int]:
    """
    修 `.title(ts("...").items(vec![...])` → `.title(ts("...")).items(vec![...])`
    一次性 regex：`.title(` 后跟一段非括号内容到 `ts("...").items(`
    """
    # 模式：.title(  +  空白  +  ts("...")  +  .items(
    # 现状：.title(ts("X").items(   缺一个 )
    # 目标：.title(ts("X")).items(
    pat = re.compile(r'\.title\(ts\("([^"]+)"\)\.items\(')
    new_text, n = pat.subn(r'.title(ts("\1")).items(', text)
    return new_text, n


def migrate(path: Path) -> None:
    text = path.read_text(encoding="utf-8")
    orig = text

    # 1. 完整调用 i18n::tr(i18n::_K[, ...]) → ts!("...")
    call_re = build_call_re()
    text, n_call = call_re.subn(lambda m: f'ts("{KEY_MAP[m.group(1)]}")', text)

    # 2. 破缺调用 i18n::tr(i18n::_K, → ts!("...") ,  （补回 `)` 和 `,`）
    bare_re = build_bare_re()
    text, n_bare = bare_re.subn(lambda m: f'ts("{KEY_MAP[m.group(1)]}"),', text)

    # 3. 修 `.title(ts("...").items(...))` 缺 `)`
    text, n_title = fix_title_parens(text)

    # 4. 单独 i18n::_KEY 引用 → "Settings.foo.bar" 字面量
    #    (root.rs 的 label_key 改成返回 &'static str)
    keyonly_re = build_keyonly_re()
    text, n_key = keyonly_re.subn(lambda m: f'"{KEY_MAP[m.group(1)]}"', text)

    # 5. 改 label() 函数体：原来是 i18n::tr(self.label_key())  →  ts!(self.label_key())
    #    已经在 step 1 命中（call_re 也匹配 i18n::tr(KEY)）
    #    但 step 4 把 KEY 改成了字面量，所以上面这行会变成 ts!("Nav.search")
    #    完美。无需额外处理。

    # 6. 移除 `use crate::gpui_app::{i18n, locale_for, themes};` 里的 `i18n`
    #    （只要文件中再无 i18n:: 引用）
    if "i18n::" not in text:
        text = re.sub(
            r'use crate::gpui_app::\{([^}]*?)\bi18n\b,?\s*([^}]*?)\};',
            lambda m: (
                f'use crate::gpui_app::{{{m.group(1)}{m.group(2)}}};'
                if m.group(1).strip() or m.group(2).strip()
                else 'use crate::gpui_app::themes;'
            ),
            text,
        )
        # 修复可能的 "use crate::gpui_app::{themes};" →  "use crate::gpui_app::themes;"
        text = re.sub(
            r'use crate::gpui_app::\{\s*([A-Za-z0-9_]+)\s*\};',
            r'use crate::gpui_app::\1;',
            text,
        )

    if text != orig:
        path.write_text(text, encoding="utf-8")
        print(
            f"  {path.relative_to(Path.cwd())}: "
            f"calls={n_call} bare={n_bare} titles={n_title} keyonly={n_key}"
        )
    else:
        print(f"  {path.relative_to(Path.cwd())}: unchanged")


def main() -> int:
    root = Path(__file__).parent.parent
    targets = [
        root / "src/gpui_app/pages/settings.rs",
        root / "src/gpui_app/root.rs",
        # 其他 page 文件里可能也有 i18n::tr 调用：
        root / "src/gpui_app/pages/library.rs",
        root / "src/gpui_app/pages/search.rs",
        root / "src/gpui_app/pages/sources.rs",
        root / "src/gpui_app/pages/tasks.rs",
    ]
    for p in targets:
        if p.exists():
            migrate(p)
    return 0


if __name__ == "__main__":
    sys.exit(main())
