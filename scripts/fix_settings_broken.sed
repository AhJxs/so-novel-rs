# 修复 settings.rs 4 类 broken 模式
# 1. i18n::tr(i18n::_K,           → ts("X"),
# 2. i18n::tr(i18n::_K, lang)     → ts("X")
# 3. .title(ts("X").items(        → .title(ts("X")).items(
# 4. .description(ts("X"),        → .description(ts("X"),)

# ---------- 1) bare call（缺 lang，缺右括号） ----------
s|i18n::tr(i18n::_ITEM_THEME,|ts("Settings.item.theme"),|g
s|i18n::tr(i18n::_ITEM_APP_LANG,|ts("Settings.item.app_lang"),|g
s|i18n::tr(i18n::_ITEM_GH_PROXY,|ts("Settings.item.gh_proxy"),|g
s|i18n::tr(i18n::_ITEM_CF_BYPASS,|ts("Settings.item.cf_bypass"),|g
s|i18n::tr(i18n::_ITEM_DOWNLOAD_PATH,|ts("Settings.item.download_path"),|g
s|i18n::tr(i18n::_ITEM_DEFAULT_FORMAT,|ts("Settings.item.default_format"),|g
s|i18n::tr(i18n::_ITEM_TXT_ENCODING,|ts("Settings.item.txt_encoding"),|g
s|i18n::tr(i18n::_ITEM_PRESERVE_CHAPTER_CACHE,|ts("Settings.item.preserve_chapter_cache"),|g
s|i18n::tr(i18n::_ITEM_ENABLE_PROGRESSBAR,|ts("Settings.item.enable_progressbar"),|g
s|i18n::tr(i18n::_ITEM_BOOK_LANG,|ts("Settings.item.book_lang"),|g
s|i18n::tr(i18n::_ITEM_SEARCH_LIMIT,|ts("Settings.item.search_limit"),|g
s|i18n::tr(i18n::_ITEM_SEARCH_FILTER,|ts("Settings.item.search_filter"),|g
s|i18n::tr(i18n::_ITEM_CONCURRENCY,|ts("Settings.item.concurrency"),|g
s|i18n::tr(i18n::_ITEM_MIN_INTERVAL,|ts("Settings.item.min_interval"),|g
s|i18n::tr(i18n::_ITEM_MAX_INTERVAL,|ts("Settings.item.max_interval"),|g
s|i18n::tr(i18n::_ITEM_ENABLE_RETRY,|ts("Settings.item.enable_retry"),|g
s|i18n::tr(i18n::_ITEM_MAX_RETRIES,|ts("Settings.item.max_retries"),|g
s|i18n::tr(i18n::_ITEM_RETRY_MIN_INTERVAL,|ts("Settings.item.retry_min_interval"),|g
s|i18n::tr(i18n::_ITEM_RETRY_MAX_INTERVAL,|ts("Settings.item.retry_max_interval"),|g
s|i18n::tr(i18n::_ITEM_PROXY_ENABLED,|ts("Settings.item.proxy_enabled"),|g
s|i18n::tr(i18n::_ITEM_PROXY_HOST,|ts("Settings.item.proxy_host"),|g
s|i18n::tr(i18n::_ITEM_PROXY_PORT,|ts("Settings.item.proxy_port"),|g
s|i18n::tr(i18n::_ITEM_QIDIAN_COOKIE,|ts("Settings.item.qidian_cookie"),|g
s|i18n::tr(i18n::_ITEM_VERSION,|ts("Settings.item.version"),|g
s|i18n::tr(i18n::_ITEM_CHECK_UPDATE_TITLE,|ts("Settings.item.check_update"),|g
s|i18n::tr(i18n::_ITEM_OPEN_GITHUB_TITLE,|ts("Settings.item.open_github"),|g

# ---------- 2) full call with lang ----------
s|i18n::tr(i18n::_SETTINGS_CHECK_UPDATE, lang)|ts("Settings.check_update_button")|g
s|i18n::tr(i18n::_SETTINGS_OPEN_GITHUB, lang)|ts("Settings.open_github_button")|g

# ---------- 3) .title() 缺右括号 ----------
s|\.title(ts("Settings.group.appearance")\.items(|\.title(ts("Settings.group.appearance")).items(|g
s|\.title(ts("Settings.group.network")\.items(|\.title(ts("Settings.group.network")).items(|g
s|\.title(ts("Settings.group.download")\.items(|\.title(ts("Settings.group.download")).items(|g
s|\.title(ts("Settings.group.source")\.items(|\.title(ts("Settings.group.source")).items(|g
s|\.title(ts("Settings.group.concurrency")\.items(|\.title(ts("Settings.group.concurrency")).items(|g
s|\.title(ts("Settings.group.retry")\.items(|\.title(ts("Settings.group.retry")).items(|g
s|\.title(ts("Settings.group.http_proxy")\.items(|\.title(ts("Settings.group.http_proxy")).items(|g
s|\.title(ts("Settings.group.cookie")\.items(|\.title(ts("Settings.group.cookie")).items(|g
s|\.title(ts("Settings.group.info")\.items(|\.title(ts("Settings.group.info")).items(|g

# ---------- 4) .description() 缺右括号（所有都是 ts("...") + trailing comma + newline） ----------
# 用循环直到不再变化
:loop
s|\.description(ts(\("[^"]*"\)),)$|\.description(ts(\1),)|
t loop
