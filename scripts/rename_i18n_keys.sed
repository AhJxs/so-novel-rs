# sed 脚本：把 root.rs / settings.rs 里所有 i18n::tr(i18n::_KEY) 改成 ts("Key.path")
# YAML 顶层大写（Nav / App / Settings）

# ---------- Nav ----------
s|i18n::tr(i18n::_NAV_SEARCH)|ts("Nav.search")|g
s|i18n::tr(i18n::_NAV_TASKS)|ts("Nav.tasks")|g
s|i18n::tr(i18n::_NAV_LIBRARY)|ts("Nav.library")|g
s|i18n::tr(i18n::_NAV_SOURCES)|ts("Nav.sources")|g
s|i18n::tr(i18n::_NAV_SETTINGS)|ts("Nav.settings")|g

# ---------- App ----------
s|i18n::tr(i18n::_APP_TITLE)|ts("App.title")|g

# ---------- Settings page ----------
s|i18n::tr(i18n::_PAGE_GENERAL)|ts("Settings.page.general")|g
s|i18n::tr(i18n::_PAGE_CRAWL)|ts("Settings.page.crawl")|g
s|i18n::tr(i18n::_PAGE_PROXY)|ts("Settings.page.proxy")|g
s|i18n::tr(i18n::_PAGE_ABOUT)|ts("Settings.page.about")|g

# ---------- Settings group ----------
s|i18n::tr(i18n::_GROUP_APPEARANCE)|ts("Settings.group.appearance")|g
s|i18n::tr(i18n::_GROUP_NETWORK)|ts("Settings.group.network")|g
s|i18n::tr(i18n::_GROUP_DOWNLOAD)|ts("Settings.group.download")|g
s|i18n::tr(i18n::_GROUP_SOURCE)|ts("Settings.group.source")|g
s|i18n::tr(i18n::_GROUP_CONCURRENCY)|ts("Settings.group.concurrency")|g
s|i18n::tr(i18n::_GROUP_RETRY)|ts("Settings.group.retry")|g
s|i18n::tr(i18n::_GROUP_HTTP_PROXY)|ts("Settings.group.http_proxy")|g
s|i18n::tr(i18n::_GROUP_COOKIE)|ts("Settings.group.cookie")|g
s|i18n::tr(i18n::_GROUP_INFO)|ts("Settings.group.info")|g

# ---------- Settings item ----------
s|i18n::tr(i18n::_ITEM_THEME)|ts("Settings.item.theme")|g
s|i18n::tr(i18n::_ITEM_APP_LANG)|ts("Settings.item.app_lang")|g
s|i18n::tr(i18n::_ITEM_GH_PROXY)|ts("Settings.item.gh_proxy")|g
s|i18n::tr(i18n::_ITEM_CF_BYPASS)|ts("Settings.item.cf_bypass")|g
s|i18n::tr(i18n::_ITEM_DOWNLOAD_PATH)|ts("Settings.item.download_path")|g
s|i18n::tr(i18n::_ITEM_DEFAULT_FORMAT)|ts("Settings.item.default_format")|g
s|i18n::tr(i18n::_ITEM_TXT_ENCODING)|ts("Settings.item.txt_encoding")|g
s|i18n::tr(i18n::_ITEM_PRESERVE_CHAPTER_CACHE)|ts("Settings.item.preserve_chapter_cache")|g
s|i18n::tr(i18n::_ITEM_ENABLE_PROGRESSBAR)|ts("Settings.item.enable_progressbar")|g
s|i18n::tr(i18n::_ITEM_BOOK_LANG)|ts("Settings.item.book_lang")|g
s|i18n::tr(i18n::_ITEM_SEARCH_LIMIT)|ts("Settings.item.search_limit")|g
s|i18n::tr(i18n::_ITEM_SEARCH_FILTER)|ts("Settings.item.search_filter")|g
s|i18n::tr(i18n::_ITEM_CONCURRENCY)|ts("Settings.item.concurrency")|g
s|i18n::tr(i18n::_ITEM_MIN_INTERVAL)|ts("Settings.item.min_interval")|g
s|i18n::tr(i18n::_ITEM_MAX_INTERVAL)|ts("Settings.item.max_interval")|g
s|i18n::tr(i18n::_ITEM_ENABLE_RETRY)|ts("Settings.item.enable_retry")|g
s|i18n::tr(i18n::_ITEM_MAX_RETRIES)|ts("Settings.item.max_retries")|g
s|i18n::tr(i18n::_ITEM_RETRY_MIN_INTERVAL)|ts("Settings.item.retry_min_interval")|g
s|i18n::tr(i18n::_ITEM_RETRY_MAX_INTERVAL)|ts("Settings.item.retry_max_interval")|g
s|i18n::tr(i18n::_ITEM_PROXY_ENABLED)|ts("Settings.item.proxy_enabled")|g
s|i18n::tr(i18n::_ITEM_PROXY_HOST)|ts("Settings.item.proxy_host")|g
s|i18n::tr(i18n::_ITEM_PROXY_PORT)|ts("Settings.item.proxy_port")|g
s|i18n::tr(i18n::_ITEM_QIDIAN_COOKIE)|ts("Settings.item.qidian_cookie")|g
s|i18n::tr(i18n::_ITEM_VERSION)|ts("Settings.item.version")|g
s|i18n::tr(i18n::_ITEM_CHECK_UPDATE_TITLE)|ts("Settings.item.check_update")|g
s|i18n::tr(i18n::_ITEM_OPEN_GITHUB_TITLE)|ts("Settings.item.open_github")|g

# ---------- Settings desc ----------
s|i18n::tr(i18n::_DESC_THEME)|ts("Settings.desc.theme")|g
s|i18n::tr(i18n::_DESC_APP_LANG)|ts("Settings.desc.app_lang")|g
s|i18n::tr(i18n::_DESC_GH_PROXY)|ts("Settings.desc.gh_proxy")|g
s|i18n::tr(i18n::_DESC_CF_BYPASS)|ts("Settings.desc.cf_bypass")|g
s|i18n::tr(i18n::_DESC_DOWNLOAD_PATH)|ts("Settings.desc.download_path")|g
s|i18n::tr(i18n::_DESC_DEFAULT_FORMAT)|ts("Settings.desc.default_format")|g
s|i18n::tr(i18n::_DESC_TXT_ENCODING)|ts("Settings.desc.txt_encoding")|g
s|i18n::tr(i18n::_DESC_PRESERVE_CHAPTER_CACHE)|ts("Settings.desc.preserve_chapter_cache")|g
s|i18n::tr(i18n::_DESC_ENABLE_PROGRESSBAR)|ts("Settings.desc.enable_progressbar")|g
s|i18n::tr(i18n::_DESC_BOOK_LANG)|ts("Settings.desc.book_lang")|g
s|i18n::tr(i18n::_DESC_SEARCH_LIMIT)|ts("Settings.desc.search_limit")|g
s|i18n::tr(i18n::_DESC_SEARCH_FILTER)|ts("Settings.desc.search_filter")|g
s|i18n::tr(i18n::_DESC_CONCURRENCY)|ts("Settings.desc.concurrency")|g
s|i18n::tr(i18n::_DESC_MIN_INTERVAL)|ts("Settings.desc.min_interval")|g
s|i18n::tr(i18n::_DESC_MAX_INTERVAL)|ts("Settings.desc.max_interval")|g
s|i18n::tr(i18n::_DESC_ENABLE_RETRY)|ts("Settings.desc.enable_retry")|g
s|i18n::tr(i18n::_DESC_MAX_RETRIES)|ts("Settings.desc.max_retries")|g
s|i18n::tr(i18n::_DESC_RETRY_MIN_INTERVAL)|ts("Settings.desc.retry_min_interval")|g
s|i18n::tr(i18n::_DESC_RETRY_MAX_INTERVAL)|ts("Settings.desc.retry_max_interval")|g
s|i18n::tr(i18n::_DESC_PROXY_ENABLED)|ts("Settings.desc.proxy_enabled")|g
s|i18n::tr(i18n::_DESC_PROXY_HOST)|ts("Settings.desc.proxy_host")|g
s|i18n::tr(i18n::_DESC_PROXY_PORT)|ts("Settings.desc.proxy_port")|g
s|i18n::tr(i18n::_DESC_QIDIAN_COOKIE)|ts("Settings.desc.qidian_cookie")|g
s|i18n::tr(i18n::_DESC_VERSION)|ts("Settings.desc.version")|g
s|i18n::tr(i18n::_DESC_CHECK_UPDATE)|ts("Settings.desc.check_update")|g
s|i18n::tr(i18n::_DESC_OPEN_GITHUB)|ts("Settings.desc.open_github")|g

# ---------- Settings button ----------
s|i18n::tr(i18n::_SETTINGS_CHECK_UPDATE)|ts("Settings.check_update_button")|g
s|i18n::tr(i18n::_SETTINGS_OPEN_GITHUB)|ts("Settings.open_github_button")|g

# ---------- Settings option ----------
s|i18n::tr(i18n::_OPT_BOOKLANG_ZH_CN)|ts("Settings.option.booklang.zh_cn")|g
s|i18n::tr(i18n::_OPT_BOOKLANG_ZH_TW)|ts("Settings.option.booklang.zh_tw")|g
s|i18n::tr(i18n::_OPT_BOOKLANG_ZH_HANT)|ts("Settings.option.booklang.zh_hant")|g
s|i18n::tr(i18n::_OPT_APPLANG_ZH_CN)|ts("Settings.option.applang.zh_cn")|g
s|i18n::tr(i18n::_OPT_APPLANG_ZH_TW)|ts("Settings.option.applang.zh_tw")|g
s|i18n::tr(i18n::_OPT_APPLANG_EN)|ts("Settings.option.applang.en")|g
