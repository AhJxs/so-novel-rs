# so-novel-rs GPUI Migration Plan

> Goal: Replace the existing egui/eframe GUI with a new GPUI + gpui-component GUI. This is not a coexistence migration. The new GUI should use gpui-component for theme, layout, controls, dialogs, notifications, sidebar navigation, icons, settings UI, and common component behavior.

## 0. Migration Principles

- Do not keep egui and GPUI GUI paths side by side.
- Keep the CLI path working.
- Keep core business modules stable wherever possible: `config`, `crawler`, `db`, `export`, `http`, `js`, `models`, `parser`, `rules`, `util`.
- Treat `src/ui`, `src/design_system`, `src/material_icons`, egui-specific cover rendering, egui window chrome, and `eframe::App` as replaceable GUI implementation.
- Prefer gpui-component components over custom widgets:
  - `Sidebar` for navigation.
  - `Button`, `DropdownButton`, `ButtonGroup` for actions.
  - `Input`, `NumberInput`, `Select`, `Combobox`, `Switch`, `Checkbox` for forms.
  - `Dialog`, `AlertDialog`, `Sheet`, `Notification` for overlays.
  - `List`, `VirtualList`, `DataTable`, `Table` for large collections.
  - `Settings` or gpui-component form helpers for settings pages.
  - `Icon` and `IconName` instead of local Material Symbols.
- Avoid recreating a full in-house design system. Keep only small project-specific helpers for formatting, empty states, status mapping, and domain display.
- Use GPUI `Entity<T>` for app/page state, and use gpui-component state objects such as `InputState`, `SelectState`, `ListState`, `TableState` where required.
- All state updates that affect UI should call `cx.notify()` from the correct GPUI context.

## 1. Current GUI Analysis

### Current Entry And App State

- `src/main.rs` starts the GUI through `eframe::run_native`.
- `src/app/mod.rs` defines `SoNovelApp`, which currently mixes:
  - app configuration and database handles,
  - page state,
  - business operation methods,
  - background channel draining,
  - toast lifecycle,
  - egui rendering loop.

### Current UI Modules

- `src/ui/nav.rs`: top horizontal navigation and toast pill.
- `src/ui/title_bar.rs`: custom egui title bar, drag, maximize, minimize, close, resize hit areas.
- `src/ui/pages/search.rs`: search/download page. This is the most complex page.
- `src/ui/pages/tasks.rs`: download task cards, progress, open/reveal/retry/cancel.
- `src/ui/pages/library.rs`: local library list, filter, open/reveal/delete.
- `src/ui/pages/sources.rs`: source management, health check, enable/disable, import, delete.
- `src/ui/pages/settings.rs`: app settings forms and persistence.
- `src/design_system/*`: egui-specific custom buttons, inputs, chips, popup, frames, theme picker, toggles.
- `src/material_icons/*`: egui font registration and Material Symbols codepoints.

### Business Logic That Should Survive

- `src/app/ops/search.rs`
- `src/app/ops/download.rs`
- `src/app/ops/library.rs`
- `src/app/ops/sources.rs`
- `src/app/ops/settings.rs`
- `src/app/ops/update.rs`
- state structs such as `SearchState`, `DownloadTask`, `LibraryState`, `SourcesState`, `UpdateState`, after removing egui-specific fields.

### Main Risk Areas

- The current egui app drains `tokio::mpsc` receivers every frame and calls `request_repaint`.
- GPUI should update entities from foreground tasks and notify the UI instead of relying on egui frame polling.
- `CoverEntry` currently stores `egui::Image<'static>`, so it must be changed to UI-neutral image data.
- `ThemePref` currently maps to `egui::ThemePreference`; it must map to gpui-component theme mode/config instead.
- The current window chrome is eframe/egui-specific and should be deferred until after the GPUI app works.

## 2. Target Architecture

### Proposed Module Layout

```text
src/
  app/
    model.rs              # UI-neutral AppModel
    controller.rs         # business-facing methods called by GPUI views
    events.rs             # AppEvent, event bridge, drain/tick logic
    i18n.rs               # language state and translation lookup
    theme.rs              # gpui-component theme integration
    ...
  gpui_app/
    mod.rs
    root.rs               # Root app view, sidebar shell, page switch
    actions.rs            # GPUI actions and keybindings
    components/
      empty_state.rs
      status_badge.rs
      book_card.rs
      task_card.rs
    pages/
      search.rs
      tasks.rs
      library.rs
      sources.rs
      settings.rs
  main.rs
```

The exact names can be adjusted during implementation, but the boundary should stay clear:

- `app` owns domain state and business actions.
- `gpui_app` owns rendering, component state, and GPUI/gpui-component integration.

### App Model

Create a UI-neutral `AppModel` that replaces the egui-specific role of `SoNovelApp`:

```rust
pub struct AppModel {
    pub paths: ConfigPaths,
    pub config: AppConfig,
    pub rules: Vec<Rule>,
    pub rule_load_error: Option<String>,
    pub config_load_error: Option<String>,
    pub source_overrides: crate::rules::SourceOverrides,
    pub current_page: NavPage,
    pub settings_dirty: bool,
    pub db: Db,
    pub toast: Option<Toast>,
    pub runtime: &'static Runtime,
    pub search: SearchState,
    pub tasks: Vec<DownloadTask>,
    pub library: LibraryState,
    pub sources_state: SourcesState,
    pub update_state: UpdateState,
}
```

The old `SoNovelApp` name can be retired or renamed to `AppModel`. Do not keep an egui app wrapper.

### GPUI Root View

The root GPUI view should hold an `Entity<AppModel>` and page/component state:

```rust
pub struct RootView {
    model: Entity<AppModel>,
    search_page: Entity<SearchPage>,
    tasks_page: Entity<TasksPage>,
    library_page: Entity<LibraryPage>,
    sources_page: Entity<SourcesPage>,
    settings_page: Entity<SettingsPage>,
    _event_task: Task<()>,
}
```

Every GPUI window must be wrapped with gpui-component `Root`:

```rust
gpui_component::init(cx);

cx.open_window(WindowOptions::default(), |window, cx| {
    let view = cx.new(|cx| RootView::new(window, cx));
    cx.new(|cx| gpui_component::Root::new(view, window, cx))
})?;
```

## 3. Theme Plan

Theme is a first-class migration requirement, not a later polish step.

### Requirements

- Use gpui-component theme APIs as the source of UI colors.
- Do not port `design_system::color` as a parallel theme system.
- Keep the existing config-level theme preference concept, but map it to gpui-component theme mode.
- All new views should style via `cx.theme().background`, `surface`, `foreground`, `muted`, `border`, `primary`, `destructive`, etc.
- Replace custom selected/hover colors with theme-aware component states wherever possible.

### Implementation Tasks

1. Replace `ThemePref::to_theme_preference` with a GPUI/gpui-component theme mapping.
2. Add `app::theme` with:
   - config-to-theme conversion,
   - theme apply function,
   - theme toggle/update function,
   - optional app-specific semantic color helpers.
3. Initialize gpui-component before creating the first window.
4. In settings, use gpui-component controls for theme selection:
   - light,
   - dark,
   - system, if supported cleanly by the GPUI/theme stack.
5. Persist theme changes through existing config save logic.

### Validation

- Start app in light mode.
- Switch to dark mode and verify sidebar, pages, dialogs, lists, and notifications update.
- Restart app and verify persisted theme.
- Ensure no page uses hard-coded egui color values.

## 4. I18n / Multi-Language Plan

The new GUI should introduce a real UI translation layer instead of hard-coded Chinese strings scattered through view code.

### Requirements

- Use the existing `LangType` config field as the selected language.
- Move user-facing GUI strings behind a translation lookup.
- Keep the first supported languages aligned with current config:
  - `简体中文`
  - `繁体中文`
  - `English`
- Use translation keys in GPUI views, not raw string literals, except for domain data such as book names, file names, URLs, and error messages returned from business logic.

### Proposed Structure

```rust
pub struct I18n {
    lang: LangType,
}

impl I18n {
    pub fn t(&self, key: I18nKey) -> &'static str;
}

pub enum I18nKey {
    AppTitle,
    NavSearch,
    NavTasks,
    NavLibrary,
    NavSources,
    NavSettings,
    SearchPlaceholder,
    SearchButton,
    DownloadFull,
    DownloadPartial,
    Open,
    Reveal,
    Delete,
    ConfirmDelete,
    Cancel,
    SettingsTheme,
    SettingsLanguage,
    // extend by page
}
```

Alternative: use static tables keyed by `&'static str`, but typed keys are preferred for refactor safety.

### Implementation Tasks

1. Create `src/app/i18n.rs`.
2. Add `i18n` state or helper access on `AppModel`.
3. Replace all new GPUI page strings with translation lookups.
4. On settings language change:
   - update `config.language`,
   - persist settings,
   - notify root/page views.
5. Keep business errors as-is initially; translate high-level UI wrappers later.

### Validation

- Switching language updates sidebar labels, page headers, buttons, empty states, dialogs, and settings labels.
- Restart preserves language.
- No new GPUI page hard-codes primary UI labels.

## 5. Stage Plan

### Stage 1: Remove egui GUI Entry And Add GPUI Foundation

Objective: Replace the GUI entry point with GPUI. Do not keep an egui GUI fallback.

Tasks:

- Update `Cargo.toml`:
  - remove `eframe`, `egui`, `egui_extras` once replacement compiles,
  - add `gpui`, `gpui_platform`, `gpui-component`, `gpui-component-assets`.
- Keep CLI dependencies and CLI behavior intact.
- Rewrite GUI branch in `src/main.rs` to start GPUI.
- Initialize tracing before GUI startup as before.
- Initialize gpui-component with `gpui_component::init(cx)`.
- Use gpui-component assets.
- Open a window with a placeholder root view.

Acceptance:

- `so-novel-rs` with no args opens a GPUI window.
- `so-novel-rs <subcommand>` still runs CLI.
- No egui/eframe app path remains.

### Stage 2: Extract UI-Neutral AppModel

Objective: Preserve business state and operations without egui types.

Tasks:

- Move the non-rendering parts of `SoNovelApp` into `AppModel`.
- Remove `impl eframe::App`.
- Move old business methods into `AppModel` or `AppController`:
  - `spawn_search`
  - `select_search_result`
  - `spawn_download`
  - `spawn_download_range`
  - `spawn_resolve_toc`
  - `toggle_source_disabled`
  - `add_sources_from_file`
  - `delete_source`
  - `persist_settings`
  - `spawn_health_check`
  - `spawn_update_check`
  - `refresh_library`
  - `delete_library_entry`
  - `clear_finished_tasks`
- Replace egui toast representation with a UI-neutral `Toast`.
- Replace egui-specific theme fields with GPUI theme config.

Acceptance:

- `AppModel::new()` can initialize config, db, rules, runtime, tasks, and search state without egui.
- Business unit tests still pass.

### Stage 3: Replace Frame Polling With GPUI Event Bridge

Objective: Make async progress update GPUI state correctly.

Tasks:

- Create `app::events`.
- Define `AppEvent` for search, details, covers, toc, downloads, source health, update check, toast expiry, and periodic ticks.
- Keep existing tokio background operations initially, but route receiver draining through GPUI foreground tasks.
- In `RootView::new`, spawn a task that periodically updates `Entity<AppModel>`.
- When events arrive, update model and call `cx.notify()`.
- Persist completed download tasks when they finish.
- Replace egui `request_repaint_after` with GPUI task/timer-driven updates.

Acceptance:

- Running search updates the model and UI.
- Running downloads update task progress.
- Health checks update source cards.
- Toasts appear and expire.

### Stage 4: Build The Modern Sidebar Shell

Objective: Establish the new UI frame using gpui-component.

Tasks:

- Implement `gpui_app::root::RootView`.
- Use a left sidebar layout:
  - app title/logo area,
  - navigation items,
  - optional footer/settings/version area.
- Use gpui-component `Sidebar` and `SidebarMenu` where practical.
- Use `IconName` instead of `material_icons`.
- Add GPUI actions/keybindings:
  - switch to search,
  - switch to tasks,
  - switch to library,
  - switch to sources,
  - switch to settings,
  - close dialog / escape.
- Render gpui-component dialog, sheet, and notification layers through `Root`.

Acceptance:

- Sidebar switches pages.
- Active page is visually clear.
- Theme changes affect shell colors.
- No custom egui navigation remains.

### Stage 5: Rebuild Shared GPUI Components

Objective: Replace the old design system with small GPUI/gpui-component wrappers only where useful.

Allowed helpers:

- `EmptyState`
- `StatusBadge`
- `StatBadge`
- `PageHeader`
- `Toolbar`
- `BookResultCard`
- `DownloadTaskCard`
- `SourceCard`
- formatting helpers such as `truncate`, `format_size`, `format_duration`.

Avoid:

- custom button system,
- custom input system,
- custom popup system,
- custom icon font system,
- duplicated theme palette.

Acceptance:

- Shared helpers use gpui-component components and `cx.theme()`.
- No new large custom design system is created.

### Stage 6: Rebuild Library Page

Objective: First functional page migration with low risk.

Components:

- `Input` for filename filter.
- `Select` for extension filter.
- `Button` for refresh/open/reveal/delete.
- `AlertDialog` for delete confirmation.
- `List` or simple card list for entries.

Tasks:

- Auto-scan library when first entering page or when download path changes.
- Preserve open/reveal/delete behavior.
- Translate all labels with `I18n`.
- Use theme-aware empty state.

Acceptance:

- Refresh works.
- Filtering works.
- Open/reveal works.
- Delete requires confirmation.

### Stage 7: Rebuild Sources Page

Objective: Source management with gpui-component cards/table.

Components:

- `Badge` for total/enabled/disabled/available counts.
- `Button` for add and health check.
- `Switch` or `Button` for enable/disable.
- `AlertDialog` for delete confirmation.
- `List`, `VirtualList`, or `DataTable` for sources.

Tasks:

- Keep source import through `rfd`.
- Keep DB-backed enable/disable overrides.
- Keep health check progress.
- Replace old card styling with theme-aware component layout.

Acceptance:

- Import JSON/JSON5 source works.
- Enable/disable persists.
- Health check updates each source.
- Delete works with confirmation.

### Stage 8: Rebuild Tasks Page

Objective: Download task UI with GPUI components.

Components:

- `ProgressBar` for progress.
- `Badge` for status counts.
- `Button` for open/reveal/retry/cancel.
- `Accordion` or `Collapsible` for failed chapters.
- `List` or `VirtualList` for tasks.

Tasks:

- Preserve task ordering: newest first.
- Preserve cancel behavior through `CancelToken`.
- Preserve DB save on finish.
- Preserve clear-finished behavior.
- Translate UI labels.

Acceptance:

- Running task updates progress.
- Completed task opens file/reveals folder.
- Failed/cancelled task can retry.
- Clear finished removes only finished tasks.

### Stage 9: Rebuild Settings Page

Objective: Replace iOS-style egui settings cards with gpui-component settings/forms.

Components:

- `Settings` or form helpers.
- `Switch` for booleans.
- `Select` for enums.
- `NumberInput` for numeric settings.
- `Input` for text settings.
- `Button` for directory picker, update check, project link.

Tasks:

- Integrate theme controls with gpui-component theme.
- Integrate language controls with new `I18n`.
- Persist changes through existing config write path.
- Keep update check behavior.
- Keep directory picker with `rfd`.

Acceptance:

- Theme switching works immediately and persists.
- Language switching works immediately and persists.
- All settings write back to `config.toml`.
- Update check still works.

### Stage 10: Rebuild Search Page

Objective: Migrate the most complex page after the infrastructure is stable.

Subcomponents:

- `SearchToolbar`
- `SourceStatusBar`
- `SearchResultList`
- `SearchResultCard`
- `BookDetailDialog`
- `DownloadRangeDialog`
- `RecentTaskBanner`

Components:

- `Input` for keyword.
- `Select` or `Combobox` for source selection.
- `Button` for search/detail/full download/partial download.
- `Badge` or `Tag` for per-source status.
- `List` or `VirtualList` for results.
- `Dialog` for detail and chapter range.
- `NumberInput` or `Stepper` for chapter range.
- `Image` for cover display.
- `Notification` or banner component for task-added feedback.

Required Data Refactors:

- Change `CoverEntry` so it no longer stores `egui::Image`.
- Store cover bytes, decoded image data, or a GPUI-compatible image cache key.
- Keep `DetailState`, `TocState`, and source status semantics.

Acceptance:

- Search starts on enter and button click.
- Source status updates as each source returns.
- Result list updates incrementally.
- Detail dialog loads metadata and cover.
- Full download starts a task.
- Partial download loads TOC and starts selected chapter range.
- New task appears on tasks page.

### Stage 11: Remove egui-specific Code

Objective: Complete the replacement and reduce maintenance burden.

Tasks:

- Delete or rewrite:
  - `src/ui`
  - `src/design_system`
  - `src/material_icons`
  - egui-specific `src/window.rs`
  - egui-specific comments and docs.
- Remove egui imports from:
  - `src/app`
  - `src/main.rs`
  - `src/lib.rs`
  - `src/cli.rs` docs/comments where needed.
- Remove egui/eframe dependencies from `Cargo.toml`.
- Update README and CLAUDE.md to describe GPUI.

Acceptance:

- `rg "\begui\b|\beframe\b|egui_" src Cargo.toml` returns no real GUI dependency references.
- `cargo check` passes.
- `cargo test` passes.

### Stage 12: Window Chrome And Polish

Objective: Reintroduce native polish only after the GPUI app is functional.

Tasks:

- Decide whether to use native GPUI title bar or gpui-component `TitleBar`.
- Revisit Windows 11 rounded corners and dark caption behavior only if GPUI exposes the required native handle cleanly.
- Add keyboard navigation and focus behavior.
- Verify text overflow, list performance, dialogs, resizing, and theme contrast.

Acceptance:

- Window can be moved, resized, minimized, maximized, and closed.
- Layout works at minimum size.
- No text overlaps.
- Large search results and long source lists remain responsive.

## 6. Claude Code Task Breakdown

Use small task prompts. Do not ask Claude Code to "migrate the GUI" in one pass.

Recommended prompts:

1. "Replace the no-args GUI entry with a minimal GPUI + gpui-component window. Do not keep an egui GUI fallback. Keep CLI mode working."
2. "Extract the non-rendering parts of `SoNovelApp` into a UI-neutral `AppModel`. Remove `impl eframe::App`."
3. "Create GPUI `RootView` with gpui-component initialization, Root wrapper, sidebar shell, theme setup, and placeholder pages."
4. "Add app-level i18n using existing `LangType`; replace sidebar and placeholder page labels with translation keys."
5. "Implement the GPUI event bridge that drains existing tokio receivers into `Entity<AppModel>` updates."
6. "Rebuild Library page with gpui-component Input, Select, Button, AlertDialog, and theme-aware list cards."
7. "Rebuild Sources page with gpui-component list/cards, badges, switch/buttons, import, health check, and delete confirmation."
8. "Rebuild Tasks page with gpui-component progress bars, status badges, action buttons, and failed-chapter collapsibles."
9. "Rebuild Settings page with gpui-component settings/forms, theme switching, language switching, persistence, and update check."
10. "Refactor cover cache to remove egui image types and rebuild Search page with GPUI dialogs, source status, result list, detail view, and chapter range download."
11. "Remove old egui design system, material icons, egui dependencies, and stale comments/docs."
12. "Polish window chrome, keyboard actions, focus handling, resizing, and final visual QA."

## 7. Verification Checklist

Run after each stage:

- `cargo check`
- `cargo test`

Manual GUI checks:

- App starts with no args.
- CLI still works with args.
- Theme switch works and persists.
- Language switch works and persists.
- Search works.
- Detail dialog works.
- Full download works.
- Partial download works.
- Task progress updates.
- Task cancel/retry works.
- Library refresh/filter/open/reveal/delete works.
- Source import/enable/disable/delete/health check works.
- Settings persist to config.
- Update check works.
- No old egui GUI path remains.

## 8. Final Completion Criteria

The migration is complete only when:

- The GUI uses GPUI and gpui-component as the only desktop GUI stack.
- No egui/eframe dependency remains.
- The app has a modern sidebar UI.
- Theme is integrated through gpui-component.
- Multi-language UI strings use the new i18n layer.
- Material icons are replaced with gpui-component `IconName`.
- Old `design_system` components are removed or reduced to small GPUI-compatible helpers.
- Core search, download, source management, local library, settings, and update workflows work.
