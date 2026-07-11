# Search 页「URL 下载」入口 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "下载链接" (Download by URL) entry button to the Search page's PageHeader that lets users paste a novel detail page URL from the browser, matches it against the configured book sources, and reuses the existing range-dialog flow to queue a download task.

**Architecture:** Single desktop-only entry on `SearchPage` (not `TasksPage` — see spec for rationale). New `open_url_dialog` method on `SearchPage` opens a URL-input Dialog with clipboard auto-paste, calls `core::sources::match_source_by_url` for origin matching, then delegates to the existing `open_range_dialog` flow with a constructed `SearchResult`. Zero structural changes to `TasksPage`, `open_range_dialog`, or the range dialog body.

**Tech Stack:** GPUI / gpui-component 0.5.1 (Button, Dialog, InputState, PageHeader), `core::sources::match_source_by_url`, `i18n::ts` / `ts_fmt`, `AppModel::push_warning` / `push_success`, `rust_i18n` translation tables in `locales/app.yml`.

## Global Constraints

- **Single locale file**: `locales/app.yml` (not `bundle/locales/*.yml`). All `Search.*` translations live here, in the format `<key>:\n    en: ...\n    zh-CN: ...\n    zh-TW: ...` per existing convention (see `Search.range.*` at lines 1004-1058).
- **No exhaustive key registry for `Search.*`**: Only `WebErrors.*` keys are exhaustively listed in `src/i18n.rs::tests::WEB_ERROR_KEYS` (line 338). New `Search.url_download.*` keys do NOT need a registry update — adding them to `app.yml` for all 3 locales is sufficient (they'll fall through to `ts` returning the key itself if missing).
- **Zero TasksPage changes**: The spec deliberately restricts the entry to `SearchPage` Header. Do not touch `src/desktop/pages/tasks/*`.
- **Reuse `open_range_dialog`**: The URL → range flow must delegate to the existing `open_range_dialog(SearchResult, window, cx)` method. Do not re-implement the TOC-loading + chapter-select pipeline.
- **`match_source_by_url` semantics**: origin-based (scheme + host + port), ignores path/query/hash. See `src/core/sources.rs:132`.
- **UI dependency tree**: gpui `Button` with `.small().outline().icon().label().on_click()` (see `tasks/row.rs:237-249` for canonical pattern). PageHeader `.action()` slot wraps in `div().child(action)` (`src/desktop/components/page_header.rs:35`).
- **TDD applies only to Task 1** (pure unit test on existing function). Tasks 2-4 are UI/integration changes verified with `cargo build` / `cargo clippy` + manual GUI smoke (per spec §6.2).

---

## File Structure

**Files modified:**
- `src/core/sources.rs` — add 3 unit tests to existing `tests` module (no production code change)
- `locales/app.yml` — append `Search.url_download.*` keys (8 keys × 3 locales) under `Search:` section near line 955
- `src/desktop/pages/search/mod.rs` — add `url_input` field, init in `new()`, add `open_url_dialog` method (~80 lines), add `.action(...)` to PageHeader

**Files NOT touched** (per spec scope):
- `src/desktop/pages/tasks/*` — no changes
- `src/desktop/pages/search/range_dialog.rs` — body unchanged, only reused via `open_range_dialog`
- `src/desktop/model/download.rs` — only consumed via `spawn_resolve_toc` (already public)
- `src/i18n.rs` — no new key registration list needed

**Estimated diff:** ~150 lines added across 3 files, no deletions.

---

## Task 1: Lock down `match_source_by_url` semantics with regression tests

**Files:**
- Modify: `src/core/sources.rs:142` (append to existing `tests` module)

**Why first:** This is the only piece of pure logic in the spec. Lock it down with tests BEFORE adding UI on top, so any later regression points here.

**Interfaces consumed:**
- `core::sources::match_source_by_url(sources: &[Source], url: &str) -> Option<&Source>` (existing, line 132)
- `models::Source { rule: Rule, ... }`, `models::Rule`

- [ ] **Step 1: Read existing test helpers in `src/core/sources.rs`**

Read `src/core/sources.rs` lines 142-200 (existing `tests` module). Confirm helpers:
- `rule(id: i32, url: &str, disabled: bool) -> Rule` — signature exists per codegraph
- Existing tests `match_source_by_url_ignores_path` and similar — confirm naming pattern so new tests don't collide

- [ ] **Step 2: Write 3 new failing tests**

Append at the end of the `tests` module (before the closing `}` of `mod tests`), in this order:

```rust
    /// 模拟 `bundle/rules/no-search.json` 的三条规则 + 三种浏览器粘贴的 URL。
    /// 校验 origin 匹配能命中正确的 source_id。
    #[test]
    fn match_source_by_url_realistic_no_search_json() {
        use crate::models::{Rule, RuleSearch};

        // 镜像 bundle/rules/no-search.json 的三条 rule（简化字段）。
        let rules = vec![
            Rule {
                id: 1,
                url: "https://www.xiaoshuohu.com/".to_string(),
                name: "小说虎".to_string(),
                search: Some(RuleSearch {
                    disabled: true,
                    ..RuleSearch::default()
                }),
                ..Rule::default()
            },
            Rule {
                id: 2,
                url: "https://cn.ttkan.co/".to_string(),
                name: "天天看小说".to_string(),
                search: Some(RuleSearch {
                    disabled: true,
                    ..RuleSearch::default()
                }),
                ..Rule::default()
            },
            Rule {
                id: 3,
                url: "https://xszj.org/".to_string(),
                name: "小说之家".to_string(),
                search: Some(RuleSearch {
                    disabled: true,
                    ..RuleSearch::default()
                }),
                ..Rule::default()
            },
        ];
        let cfg = AppConfig::default();
        let sources: Vec<Source> = rules.iter().map(|r| Source::from(r.clone(), &cfg)).collect();

        // 浏览器粘贴的 3 种 URL：含 path / query / hash，应都命中。
        let cases = [
            ("https://www.xiaoshuohu.com/0/12345/", 1),
            ("https://cn.ttkan.co/novel/page/abc?utm_source=x#chapter-1", 2),
            ("https://xszj.org/b/42/cs/1", 3),
        ];
        for (url, expected_id) in cases {
            let matched = match_source_by_url(&sources, url);
            assert_eq!(
                matched.map(|s| s.rule.id),
                Some(expected_id),
                "URL {url:?} 应命中 source_id={expected_id}"
            );
        }
    }

    /// Origin 含 port — rule `https://a.com:8080` 不会被 `https://a.com` 命中，
    /// 反之亦然（防止用户粘贴端口错配的 URL 误命中）。
    #[test]
    fn match_source_by_url_handles_port_difference() {
        let cfg = AppConfig::default();
        let rules = vec![rule(1, "https://a.com:8080", false)];
        let sources: Vec<Source> = rules.iter().map(|r| Source::from(r.clone(), &cfg)).collect();

        assert!(
            match_source_by_url(&sources, "https://a.com/foo").is_none(),
            "rule 带 :8080，URL 不带端口 → 不应命中"
        );
        assert!(
            match_source_by_url(&sources, "https://a.com:8080/foo").is_some(),
            "URL 带 :8080 → 应命中"
        );
        assert!(
            match_source_by_url(&sources, "https://a.com:9090/foo").is_none(),
            "URL 带不同端口 :9090 → 不应命中"
        );
    }

    /// 完全不认识的 origin → None（不 panic / 不返回首条）。
    #[test]
    fn match_source_by_url_returns_none_for_unknown_origin() {
        let cfg = AppConfig::default();
        let rules = vec![rule(1, "https://known.com/", false)];
        let sources: Vec<Source> = rules.iter().map(|r| Source::from(r.clone(), &cfg)).collect();

        assert!(match_source_by_url(&sources, "https://unknown.org/foo").is_none());
        // 非法 URL（不是 http/https） → 也返回 None。
        assert!(match_source_by_url(&sources, "not a url").is_none());
        assert!(match_source_by_url(&sources, "ftp://known.com/foo").is_none());
    }
```

**Imports already present in module scope:** `super::*` re-exports `match_source_by_url`, `Source`. `crate::models::{Rule, RuleSearch}` already imported per codegraph. `AppConfig` is in `crate::core::AppConfig` — verify import is present (if not, add `use crate::core::AppConfig;` to module top).

- [ ] **Step 3: Run tests**

Run:
```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib core::sources::tests::match_source_by_url -v
```

Expected: 3 tests PASS (since `match_source_by_url` already works correctly — these tests are regression locks, not TDD red→green).

If `AppConfig` import missing, add `use crate::core::AppConfig;` at top of `tests` module and re-run.

- [ ] **Step 4: Run full test suite for sources module to verify no regression**

Run:
```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib core::sources
```

Expected: ALL sources tests pass (old + 3 new).

- [ ] **Step 5: Commit**

```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && git add src/core/sources.rs && git commit -m "test(sources): lock down match_source_by_url for no-search.json scenarios"
```

---

## Task 2: Add `Search.url_download.*` i18n keys to `locales/app.yml`

**Files:**
- Modify: `locales/app.yml:955` — append a new `url_download:` sub-section under the `Search:` block (which currently ends with `source_status:` at line 1068+)

**Why before UI:** All new UI text must already exist in the locale table before the GUI uses `ts(...)` — otherwise the toast / button label shows the raw key string `"Search.url_download.button"`.

**Interfaces consumed:**
- All `Search.url_download.*` keys consumed in Task 3 / Task 4:
  - `button` — PageHeader button label
  - `dialog_title` — URL input Dialog title
  - `placeholder` — InputState placeholder
  - `auto_pasted` — hint line below input when clipboard paste happened
  - `paste_button` — fallback「粘贴」button label
  - `confirm` — Dialog OK button
  - `cancel` — Dialog Cancel button
  - `no_match` — warning toast when no rule matches
  - `matched_source` — success toast after match (`ts_fmt` with `{name}`)

- [ ] **Step 1: Locate the end of the `Search:` section in `locales/app.yml`**

Read `locales/app.yml` starting at line 955 to find the last key inside `Search:`. The `source_status:` block is the current tail (around lines 1068-1080+). Add the new section AFTER `source_status:` ends, still indented at 2 spaces (same level as `page_title:`, `range:`, `source_status:`).

- [ ] **Step 2: Append the `url_download` block**

Insert the following YAML after the last key of the `Search:` section (preserve correct indent — top-level keys are 0 spaces, sub-keys 2 spaces, locale keys 4 spaces):

```yaml
  # URL 下载入口（PageHeader「下载链接」按钮 → 输入 Dialog → 复用 range_dialog）
  url_download:
    # PageHeader 按钮 label
    button:
      en: "Download by URL"
      zh-CN: 下载链接
      zh-TW: 下載連結
    # URL 输入 Dialog 标题
    dialog_title:
      en: "Download from URL"
      zh-CN: 从链接下载
      zh-TW: 從連結下載
    # URL 输入框 placeholder
    placeholder:
      en: "Paste novel detail page URL (http/https)..."
      zh-CN: 请粘贴小说详情页链接（http/https）...
      zh-TW: 請粘貼小說詳情頁連結（http/https）...
    # 剪贴板已自动粘贴时的提示行
    auto_pasted:
      en: "✓ Auto-pasted from clipboard"
      zh-CN: "✓ 已自动粘贴剪贴板"
      zh-TW: "✓ 已自動粘貼剪貼板"
    # Dialog 内的兜底「粘贴」按钮
    paste_button:
      en: Paste
      zh-CN: 粘贴
      zh-TW: 粘貼
    # Dialog OK 按钮（=开始解析 URL）
    confirm:
      en: Resolve
      zh-CN: 解析
      zh-TW: 解析
    # Dialog Cancel 按钮
    cancel:
      en: Cancel
      zh-CN: 取消
      zh-TW: 取消
    # 未匹配到任何书源的 warning toast
    no_match:
      en: "No book source matches this URL. Check the link or enable a source on the Sources page."
      zh-CN: "未匹配到任何书源，请检查链接是否正确或在「书源」页启用对应源"
      zh-TW: "未匹配到任何書源，請檢查連結是否正確或在「書源」頁啟用對應源"
    # 匹配成功的 success toast（{name} = 书源名）
    matched_source:
      en: "✓ Matched: {name}"
      zh-CN: "✓ 已匹配：{name}"
      zh-TW: "✓ 已匹配：{name}"
```

- [ ] **Step 3: Verify YAML syntax via cargo build**

Run:
```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo build -p so-novel 2>&1 | head -30
```

Expected: build succeeds (rust_i18n macro validates YAML at compile time). Any indentation / quote error will surface here.

If error: fix YAML and re-run.

- [ ] **Step 4: Verify each new key resolves to non-empty translation in all 3 locales**

Run a one-off Rust test to confirm all 9 keys are translated (sanity check; the runtime `ts()` only warns on miss, not errors). Add this as a temporary test in `src/i18n.rs::tests` (or run via `cargo test --test` if a separate test file exists).

Easiest path: add a one-shot test next to `web_errors_translated_in_all_three_locales` at line 394:

```rust
    /// Sanity check: spec 2026-07-11-tasks-url-download 新增的 9 个 Search.url_download.* key
    /// 在三 locale 下都非空。**未**加进 WEB_ERROR_KEYS 注册表（Search.* 段无注册表约束），
    /// 但仍需运行时兜底校验。
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
```

Run:
```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib i18n::tests::url_download_translated -v
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && git add locales/app.yml src/i18n.rs && git commit -m "feat(i18n): add Search.url_download.* keys for download-by-URL feature"
```

---

## Task 3: Add `url_input` field + `open_url_dialog` method to `SearchPage`

**Files:**
- Modify: `src/desktop/pages/search/mod.rs:59` (struct fields)
- Modify: `src/desktop/pages/search/mod.rs:108` (`new()` initialization)
- Modify: `src/desktop/pages/search/mod.rs` (add `open_url_dialog` method, place it near `open_range_dialog` at line 371)

**Interfaces consumed:**
- `core::sources::match_source_by_url(sources, url) -> Option<&Source>` (Task 1)
- `core::sources::Source::from(rule, cfg)` (existing)
- `AppModel::push_warning`, `AppModel::push_success` (existing, see `src/desktop/model/mod.rs:192,197`)
- `AppModel::spawn_resolve_toc(target)` (existing, called by `open_range_dialog` already)
- `SearchPage::open_range_dialog(target, window, cx)` (existing, line 371)
- `SearchResult { source_id, source_name, url, book_name, author, ... }` — construct directly with `book_name: ""` and other fields `None` / empty
- `gpui::Window::open_dialog(cx, |Dialog, ...| ...)` (same pattern as `open_range_dialog`)
- `gpui::AppContext::read_from_clipboard() -> Option<String>` (gpui std API for clipboard)

**Interfaces produced:**
- `SearchPage::open_url_dialog(window, cx)` — public method called by PageHeader action in Task 4

- [ ] **Step 1: Add `url_input` field to `SearchPage` struct**

In `src/desktop/pages/search/mod.rs` line 104 (just below `range_initialized: bool`), add:

```rust
    /// URL 输入 Dialog 的输入框 —— PageHeader「下载链接」按钮弹 Dialog 时承载 URL 输入。
    /// 同 `keyword` / range_start_input 同款 owner 持有（Entity 在 owner 里缓存，
    /// render 闭包只复用），避免 InputState 失活。placeholder 在 `new()` 一次性设好。
    url_input: Entity<InputState>,
```

- [ ] **Step 2: Initialize `url_input` in `new()`**

Find the end of `SearchPage::new()` (currently at the closing `}` after `current_page: 0` assignment + range_start_input / range_end_input init around lines 167-250). Add initialization BEFORE the `Self { ... }` literal:

```rust
        let url_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(ts("Search.url_download.placeholder").to_string())
        });
```

Then add the field inside the `Self { ... }` literal:

```rust
            url_input,
```

Place it right after `range_initialized: false,` to group with the other Dialog-input fields.

- [ ] **Step 3: Verify compile**

Run:
```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo build -p so-novel 2>&1 | tail -40
```

Expected: build succeeds. If error about unused field, ignore (it's used in Step 4).

- [ ] **Step 4: Add `open_url_dialog` method**

Add this method right above `open_range_dialog` (line 371). The method:
1. Reads clipboard → if http(s) URL, sets `url_input` value
2. Opens a confirm Dialog with TextInput body
3. On OK: reads value, calls `match_source_by_url`, either push_warning (no match) or constructs `SearchResult` + delegates to `open_range_dialog`

```rust
    /// PageHeader「下载链接」按钮回调：弹 URL 输入 Dialog（自动粘贴剪贴板）→
    /// 匹配书源 → 复用 `open_range_dialog` 走选章下载流程。
    ///
    /// 与 `open_range_dialog` 的关系：URL 输入 + 匹配后，构造一个最小 `SearchResult`
    /// （book_name 空 / 元信息全 None —— range_dialog 不依赖这些，TOC 解析时会从
    /// 详情页拿完整数据）→ 走 `open_range_dialog` 同样的 `spawn_resolve_toc` +
    /// `range_dialog::content` 反应式渲染路径。零结构改动。
    fn open_url_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 自动粘贴：Dialog 打开时把剪贴板里的 URL 填进去（http(s) 才填）。
        // `read_from_clipboard` 跨平台都返回 Option<String>，失败 / 非字符串返回 None。
        if let Some(s) = cx.read_from_clipboard() {
            let trimmed = s.trim();
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                self.url_input.update(cx, |state, cx| {
                    state.set_value(trimmed.to_string(), window, cx);
                });
            }
        }

        let page = cx.entity();
        window.open_dialog(cx, move |dialog: Dialog, window, cx| {
            // builder 是 Fn（每帧重调）→ 每帧 clone page 进当帧闭包。
            let page = page.clone();
            // 渲染 body：TextInput + 「粘贴」兜底按钮 + 自动粘贴提示行。
            let url_input = page.read(cx).url_input.clone();
            let body = v_flex()
                .gap_2()
                .child(Input::new(&url_input).cleanable(true))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            Button::new("url-paste")
                                .small()
                                .ghost()
                                .label(ts("Search.url_download.paste_button"))
                                .on_click(move |_, window, cx| {
                                    // 兜底：Dialog 打开后剪贴板被新内容覆盖时，重新读一次。
                                    if let Some(s) = cx.read_from_clipboard() {
                                        let trimmed = s.trim();
                                        if trimmed.starts_with("http://")
                                            || trimmed.starts_with("https://")
                                        {
                                            url_input.update(cx, |state, cx| {
                                                state.set_value(
                                                    trimmed.to_string(),
                                                    window,
                                                    cx,
                                                );
                                            });
                                        }
                                    }
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(ts("Search.url_download.auto_pasted")),
                        ),
                );
            dialog
                .title(ts("Search.url_download.dialog_title"))
                .w(px(520.))
                .child(body)
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(ts("Search.url_download.confirm"))
                        .cancel_text(ts("Search.url_download.cancel")),
                )
                .confirm()
                .on_ok(move |_ev, _window, cx| {
                    // OK 不关 Dialog（保留以便用户改 URL 重试）—— 除非匹配成功且 range Dialog
                    // 接管，否则返回 false 让 Dialog 保持。但 OK 后会立刻构造 SearchResult +
                    // 调 open_range_dialog 弹第二个 Dialog，第二个 Dialog 弹出时第一个还在，
                    // 视觉重叠；range_dialog 流程结束时通知会显示，关第二个时第一个还在。
                    // 这里选择：OK 后关 URL Dialog（true），改 URL 时重新点按钮。
                    let url = page.read(cx).url_input.read(cx).value().to_string();
                    let url = url.trim().to_string();
                    if url.is_empty() {
                        page.update(cx, |p, cx| {
                            p.model.update(cx, |m, _cx| {
                                m.push_warning(ts("Search.url_download.no_match"));
                            });
                            cx.notify();
                        });
                        return true; // 关 Dialog（避免空提交噪音）
                    }
                    // 匹配书源。model.read 借用 rules + config → 构造 Vec<Source> 一次性。
                    let Some(source) = page.read(cx).model.read(cx).rules.iter()
                        .find_map(|r| {
                            let sources = [Source::from(r.clone(), &page.read(cx).model.read(cx).config)];
                            crate::core::sources::match_source_by_url(&sources, &url).map(|s| s.rule.clone())
                        })
                    else {
                        page.update(cx, |p, cx| {
                            p.model.update(cx, |m, _cx| {
                                m.push_warning(ts("Search.url_download.no_match"));
                            });
                            cx.notify();
                        });
                        return true;
                    };
                    // 匹配成功：构造最小 SearchResult + 复用现有 open_range_dialog。
                    let target = SearchResult {
                        source_id: source.id,
                        source_name: source.name.clone(),
                        url: url.clone(),
                        book_name: String::new(),
                        author: None,
                        intro: None,
                        category: None,
                        latest_chapter: None,
                        last_update_time: None,
                        status: None,
                        word_count: None,
                    };
                    page.update(cx, |p, cx| {
                        p.model.update(cx, |m, _cx| {
                            m.push_success(ts_fmt(
                                "Search.url_download.matched_source",
                                &[("name", &source.name)],
                            ));
                        });
                        p.open_range_dialog(target, _window, cx);
                        cx.notify();
                    });
                    true // 关 URL Dialog，range Dialog 接管
                })
        });
    }
```

**Required imports to add at top of `src/desktop/pages/search/mod.rs`** (some may already exist; check before adding):

```rust
use gpui_component::{
    // ...existing...
    input::Input,  // 新增：TextInput 用 Input::new(&state)
    button::Button,  // 新增：「粘贴」兜底按钮 + PageHeader 按钮
    h_flex,  // 新增：粘贴按钮 + 提示行的水平布局
};
```

Verify each import is needed by searching the file; remove duplicates. `ts_fmt` may also need to be imported from `crate::i18n`:

```rust
use crate::i18n::{ts, ts_fmt};  // 在 ts 后加 ts_fmt
```

- [ ] **Step 5: Verify compile + clippy**

Run:
```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo build -p so-novel 2>&1 | tail -40
cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo clippy -p so-novel --no-deps -- -D warnings 2>&1 | tail -40
```

Expected: build succeeds, clippy clean (or with only existing warnings).

Common fixes:
- If `match_source_by_url` signature in the on_ok closure trips borrow checker (calling `page.read(cx).model.read(cx)` twice in the `find_map`), refactor to bind `let m = page.read(cx).model.read(cx);` once before the closure.
- If `Input` import collides with other imports, rename.

- [ ] **Step 6: Commit**

```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && git add src/desktop/pages/search/mod.rs && git commit -m "feat(search): add open_url_dialog with clipboard auto-paste and origin matching"
```

---

## Task 4: Wire PageHeader action button + final verification

**Files:**
- Modify: `src/desktop/pages/search/mod.rs` (PageHeader call site, around line 570)

**Interfaces consumed:**
- `SearchPage::open_url_dialog(window, cx)` (Task 3)
- `PageHeader::action(impl IntoElement)` (existing, `src/desktop/components/page_header.rs:35`)
- `Button::new(id).small().outline().icon(IconName::X).label(...).on_click(closure)` (existing pattern in `tasks/row.rs:237-249`)

**Why last:** Final wire-up. Once this lands, the feature is user-visible end-to-end.

- [ ] **Step 1: Locate the PageHeader call in `SearchPage::render`**

Find the existing line (~570):

```rust
            .child(PageHeader::new(ts("Search.page_title")).subtitle(ts("Search.page_subtitle")))
```

Replace it with:

```rust
            .child(
                PageHeader::new(ts("Search.page_title"))
                    .subtitle(ts("Search.page_subtitle"))
                    .action(
                        Button::new("search-url-download")
                            .small()
                            .outline()
                            .icon(Icon::new(IconName::ArrowDownToLine))
                            .label(ts("Search.url_download.button"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_url_dialog(window, cx);
                            })),
                    ),
            )
```

- [ ] **Step 2: Verify all new imports are present**

Confirm `Icon` is imported. The current import at line 30-32 of `search/mod.rs` is:

```rust
use gpui_component::{
    ActiveTheme as _, IconName, WindowExt,
    dialog::{Dialog, DialogButtonProps},
    ...
};
```

`IconName` is imported but `Icon` is not. Add:

```rust
use gpui_component::{
    ActiveTheme as _, Icon, IconName, WindowExt,  // 加 Icon
    ...
};
```

`Button` was added in Task 3 if not already present (verify with grep on existing imports).

- [ ] **Step 3: Run full build + clippy**

Run:
```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo build -p so-novel 2>&1 | tail -40
cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo clippy -p so-novel --no-deps -- -D warnings 2>&1 | tail -40
```

Expected: both clean. Common clippy warnings to fix:
- `clippy::too_many_arguments` on `open_url_dialog` closure — refactor to extract helper fn if hit.
- `clippy::needless_borrow` — strip `&` on `&url` when calling `match_source_by_url` if clippy complains.

- [ ] **Step 4: Run all Search-related tests**

Run:
```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib desktop::pages::search 2>&1 | tail -30
```

Expected: existing tests pass (no regressions in search page tests).

- [ ] **Step 5: Manual GUI smoke test (per spec §6.2)**

Launch desktop binary and verify:
1. Search page header shows the new「下载链接」button (right side of title)
2. Copy `https://cn.ttkan.co/novel/chapters/some-book-id` from browser → click button → Dialog opens with URL pre-filled → click「解析」→ range Dialog opens with TOC loading
3. Wait for TOC → click「下载」→ Tasks page shows new Running task
4. Copy an unknown URL (e.g., `https://example.com/foo`) → click button → paste URL → click「解析」→ warning toast「未匹配到任何书源…」appears, URL Dialog closes
5. With clipboard empty → click button → Dialog opens empty (placeholder visible) → manually paste a valid no-search.json URL → click「解析」→ range Dialog opens

If any step fails: trace via `tracing::warn!` output (the `find_matched_rule` / `match_source_by_url` path emits warnings on no-match).

- [ ] **Step 6: Commit**

```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && git add src/desktop/pages/search/mod.rs && git commit -m "feat(search): wire URL download button on SearchPage header"
```

---

## Self-Review

**1. Spec coverage** — every requirement in `docs/superpowers/specs/2026-07-11-tasks-url-download-design.md`:

- §2.1 复用 SearchPage 现有结构 → Task 3 step 4 delegates to `open_range_dialog` (✓)
- §2.2 匹配规则（origin 前缀）→ Task 1 tests + Task 3 uses `match_source_by_url` (✓)
- §2.3 自动粘贴剪贴板 → Task 3 step 4 `cx.read_from_clipboard()` in `open_url_dialog` (✓)
- §3.1 新增 `url_input: Entity<InputState>` 字段 → Task 3 step 1 (✓)
- §3.2 流程图（按钮 → Dialog → 匹配 → 复用 range_dialog）→ Task 3 step 4 (✓)
- §3.3 PageHeader `.action(Button)` → Task 4 step 1 (✓)
- §3.4 内联 vs 抽模块 → Decision in spec, no code split needed (✓)
- §4 i18n keys（zh-CN/zh-TW/en）→ Task 2 step 2 (✓)
- §5 错误处理表（空 / 不匹配 / disabled / TOC 失败 / range 无效）→ Task 3 step 4 handles empty + no_match; range_dialog handles rest (✓)
- §6.1 单元测试 → Task 1 (✓)
- §6.2 集成 + cargo build/clippy → Task 3 step 5, Task 4 step 3 (✓)
- §7 决策点（disabled 不过滤 / OK 禁用 / SearchPage 位置 / 不批量）→ All honored (✓)
- §8 不在范围（CLI / TasksPage / Web / 批量）→ No changes there (✓)

**2. Placeholder scan:** No "TBD" / "TODO" / "implement later" / "fill in details" present. Every code block is complete.

**3. Type consistency:**
- `Source::from(rule, cfg)` — matches Task 3 usage and `core::sources::Source` constructor pattern
- `SearchResult` fields — all 10 fields named per `models::SearchResult` definition
- `Entity<InputState>` — same type as `keyword`, `range_start_input`
- `match_source_by_url(sources: &[Source], url: &str) -> Option<&Source>` — matches Task 1 test usage
- `ts_fmt(key, &[("name", value)])` — matches existing convention in `tasks/row.rs:281-283`
- `Button::new("id").small().outline().icon(Icon::new(IconName::X)).label(...).on_click(closure)` — matches `tasks/row.rs:237-249` pattern

**4. Risk note:** Task 3 step 4 `on_ok` closure has nested borrows (`page.read(cx).model.read(cx).rules.iter().find_map(...)`). If borrow checker fails, refactor to:

```rust
let m = page.read(cx).model.read(cx);
let rules = m.rules.clone();
let cfg = m.config.clone();
let source = rules.iter().find_map(|r| {
    let sources = [Source::from(r.clone(), &cfg)];
    crate::core::sources::match_source_by_url(&sources, &url).map(|s| s.rule.clone())
});
```

(`Rule` is Clone per `models::rule.rs`.)

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-11-tasks-url-download.md`.

**Two execution options:**

1. **Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration. Best for catching regressions early.

2. **Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints for review. Faster but I won't have fresh context per task.

Which approach?