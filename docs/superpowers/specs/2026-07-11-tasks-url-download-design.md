# Search 页「URL 下载」入口设计

> 状态：草案 v1 · 2026-07-11（v2：定位改为 SearchPage Header）
> 范围：GPUI 桌面端 `SearchPage`
> 关联：`bundle/rules/no-search.json`、search 页 `range_dialog.rs` / `open_range_dialog`

## 1. 背景与目标

`bundle/rules/no-search.json` 里收录的书源（小说虎、天天看小说、小说之家）
都把 `Rule.search.disabled` 设为 true —— 搜索功能不可用，但详情 / TOC / 章节
下载规则完整。用户从这些站点的浏览器里复制小说详情页 URL，目前没有 GUI
入口把它转成下载任务，只能走 CLI `run_download`。

**v1 → v2 改动**：入口从 `TasksPage` 移到 `SearchPage`。
- SearchPage 已具备完整的 range_dialog 状态机（`range_target` / `range_initialized` /
  `range_start_input` / `range_end_input` 四个 NumberInput entity + Change/Step 订阅 +
  `toc_cache` 查询路径），复用零成本。
- TasksPage 不需要任何改动。
- 仅多一个 PageHeader action 按钮 + URL 输入 Dialog + `match_source_by_url` 调用。

目标：在 SearchPage PageHeader 增加一个「下载链接」按钮 →
弹 URL 输入 Dialog → 匹配已有书源 → 复用现有 `open_range_dialog` 流程
加载章节信息 → 派下载任务。

行为对齐现有 `open_range_dialog`，但触发条件从"点搜索结果"变成"粘贴 URL"。

## 2. 设计要点

### 2.1 复用 SearchPage 现有结构

零结构改动。SearchPage 已有的：

- `range_target: Option<SearchResult>`（line 101）
- `range_initialized: bool`（line 104）
- `range_start_input / range_end_input: Entity<InputState>`（lines 97-98）
- 4 个 NumberInput 订阅（start/end × Change/Step，lines 167-250）
- `open_range_dialog(target, window, cx)`（line 371）
- `confirm_range_download(cx) -> RangeOutcome`（line 443）
- `current_range_chapters_len(cx) -> Option<u32>`（line 288）

全部不动。新增的 `open_url_dialog` 在 URL 匹配成功后直接调 `open_range_dialog`，
传递构造的 `SearchResult`。整条链路 0 改动。

### 2.2 匹配规则

用 **`core::sources::match_source_by_url`** —— 基于 `url::Url::parse` 的 origin
比对（scheme + host + port），忽略 path/query。这是 CLI `run_download` 已经在用的语义。

支持场景：`https://www.xiaoshuohu.com/0/12345/` 命中 `https://www.xiaoshuohu.com/`。

```rust
fn find_matched_rule(rules: &[Rule], cfg: &AppConfig, url: &str) -> Option<Rule> {
    let sources: Vec<Source> = rules.iter().map(|r| Source::from(r.clone(), cfg)).collect();
    match_source_by_url(&sources, url).map(|s| s.rule.clone())
}
```

调用点：
```rust
let m = self.model.read(cx);
let rule = find_matched_rule(&m.rules, &m.config, &url)?;
```

### 2.3 自动粘贴剪贴板

Dialog 打开时调用 `cx.read_from_clipboard()`：
- 剪贴板空 / 非字符串 / 不是 http(s) URL → 不填，提示用户手动粘贴
- 是 http(s) URL → 填入 TextInput + 显示一行小灰字 "已自动粘贴剪贴板"

同时保留 Dialog 内一个「粘贴」小按钮作为兜底（剪贴板被其他进程覆盖后用）。

## 3. 组件与数据流

### 3.1 SearchPage 新增字段

只新增 1 个：

```rust
pub struct SearchPage {
    // 现有字段全部保留...
    url_input: Entity<InputState>,    // 新增：URL 输入 Dialog 的输入框
}
```

NumberInput entity 不动（复用）。

### 3.2 流程

```
用户点 PageHeader "下载链接" 按钮
    │
    ▼
SearchPage::open_url_dialog(window, cx)
    │
    ├─ 读剪贴板 cx.read_from_clipboard()，若是 http(s) URL → url_input.set_value
    │  url_input 已有 placeholder 复用 ts("Search.url_download.placeholder")
    │
    ▼
弹 Dialog (URL 输入, 520px)
    body: TextInput (url_input) + 「粘贴」按钮 + 自动粘贴提示行
    OK 回调: read url from url_input.value()
    │
    ▼
SearchPage::try_match_url(url, cx) → Option<Rule>
    │
    ├─ None → push_warning(ts("Search.url_download.no_match"))
    │         Dialog 保持，用户可改 URL 重试
    │         (注: 非法 URL 也走这条 — match_source_by_url 内部 url::Url::parse 失败
    │          直接返 None, 不区分 "URL 不合法" vs "没匹配到源")
    │
    └─ Some(rule) →
            ├─ 构造 SearchResult { source_id: rule.id, source_name: rule.name.clone(),
            │                       url: url.clone(), book_name: "",
            │                       author: None, ... }
            ├─ push_success(ts_fmt("Search.url_download.matched_source",
            │                      &[("name", &rule.name)]))
            └─ 复用现有 open_range_dialog(target, window, cx)
                    ├─ spawn_resolve_toc(target)
                    ├─ range_target = Some(target)
                    ├─ range_initialized = false
                    └─ 弹 Dialog #2 (现有 range_dialog, 完全复用)
                            body: range_dialog::content(&page, window, cx)
                            OK 回调: 现有 confirm_range_download(cx) →
                                    RangeOutcome::Done/Invalid/Pending
                            Done → spawn_download_range + 关闭 Dialog + push_success
```

### 3.3 PageHeader 改动

`src/desktop/pages/search/mod.rs:570`：

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

### 3.4 内联 vs 抽模块

URL 输入 Dialog body 简单（~30 行：标题 + TextInput + 「粘贴」按钮 + 自动粘贴提示行
+ OK 回调），跟 `range_dialog.rs` 不一样（后者涉及 toc_cache 反应式渲染 + 起止输入框
+ 章节名预览 ~200 行）。

**决策**：内联在 `search/mod.rs`，不开新模块。如果未来 Dialog body 长到拆出去划算
（>80 行 / 含多分支），再抽 `url_dialog.rs`。

## 4. i18n

`bundle/locales/{zh-CN,zh-TW,en}.yml` 同步新增：

```yaml
Search:
  url_download:
    button: 下载链接                # en: Download by URL
    dialog_title: 从链接下载         # en: Download from URL
    placeholder: 请粘贴小说详情页链接（http/https）...
    auto_pasted: "✓ 已自动粘贴剪贴板"
    paste_button: 粘贴               # en: Paste
    confirm: 解析                   # en: Resolve
    cancel: 取消                    # en: Cancel
    no_match: 未匹配到任何书源，请检查链接是否正确或在「书源」页启用对应源
    matched_source: "✓ 已匹配：{name}"  # en: "✓ Matched: {name}"
```

新增 key 必须在 `i18n::tests` 的相关注册表里同步注册，否则 i18n 测试 fail。
具体位置实现时确认（沿用现有 `Tasks` / `Search` 的注册方式）。

## 5. 错误处理

| 场景 | 行为 |
|------|------|
| TextInput 为空 / 纯空白 | OK 按钮禁用（isDisabled 按 value().trim().is_empty()） |
| URL 解析失败 或 `match_source_by_url` 返回 None | push_warning `Search.url_download.no_match`，Dialog 保持 |
| `Rule.disabled == true` | 当前 `match_source_by_url` 不过滤 disabled — 见 §7 决策点 |
| TOC 拉取失败 | 复用 range_dialog Failed 占位 + push_warning |
| range start/end 输入无效 | 复用 range_dialog Invalid 处理（保留 Dialog） |
| TOC 加载中点 OK | 复用 Pending 处理（保留 Dialog） |

## 6. 测试

### 6.1 单元测试

新增 `src/core/sources.rs::tests`：
- `match_source_by_url_realistic_no_search_json`：模拟 no-search.json 三条规则
  + 三种浏览器粘贴 URL（含 path / query / hash），校验命中 ID。
- `match_source_by_url_handles_port_difference`：rule `https://a.com:8080` 不会被
  `https://a.com` 命中（origin 含 port）。
- `match_source_by_url_returns_none_for_unknown_origin`。

### 6.2 集成 / 手动

- `cargo build` + `cargo clippy` 通过
- GUI 端到端：复制 no-search.json 站点（如 https://cn.ttkan.co/）某小说详情页 URL →
  点 Search 「下载链接」→ 校验 Dialog 弹出、自动填入、点解析 → 选章 Dialog
  加载章节数 → 点下载 → Tasks 列表新增 Running 任务。
- 复制一个不存在的 URL → 校验 warning toast + Dialog 保持。
- 复制一个 enabled 搜索源 + 来源真实小说 URL → 同样流程跑通。

## 7. 决策点

1. **`match_source_by_url` 是否过滤 disabled rule？**
   - 当前实现**不过滤**（见 `src/core/sources.rs:132` 的实现）。
   - 用户场景：no-search.json 三条规则 Rule.disabled 全是 false（只是
     RuleSearch.disabled=true），所以无影响。
   - 决策：**保持现状**，不另加 disabled 过滤（避免行为偏离 CLI）。如果用户
     显式禁用了规则，跳过即可。

2. **URL 输入 Dialog 的 OK 按钮在输入空时是否禁用？**
   - 决策：**禁用**。避免空提交触发无效 toast 噪音。

3. **位置：SearchPage Header（v2 最终决策）**
   - 用户从 v1 的 TasksPage Header 改为 SearchPage Header。
   - 理由：SearchPage 已有完整 range_dialog 状态机，复用零成本。

4. **是否要支持批量（一次粘贴多个 URL）？**
   - YAGNI。当前只单条 URL 入口；批量属于另一个 spec。

## 8. 不在范围内

- CLI 改动（已支持 `run_download --url`）
- TasksPage 改动
- Web 端入口（当前仅 desktop Search 页面）
- 多选 / 粘贴多个 URL 批量派发
- 剪贴板内容自动 validate（仅自动填入，不校验合法 URL）
- 收藏 / 历史 URL 列表（无持久化）

## 9. 风险与回滚

- **SearchPage `open_range_dialog` 不动** —— 复用现有方法，regression 风险点 = 0。
- **PageHeader `action` slot 已存在**（`src/desktop/components/page_header.rs:35`），
  改动面 = `search/mod.rs`：
  - 1 行 import 新增（Button / Icon / IconName）
  - 1 个新字段 `url_input` + `new()` 内 1 个 `cx.new(...)`
  - 1 个新方法 `open_url_dialog`（~80 行）
  - PageHeader 调用处加 `.action(...)`（~10 行）
- **剪贴板权限**：gpui `cx.read_from_clipboard()` 在 Windows / macOS / Linux
  均不需显式权限，失败时静默返回 None。

回滚：单 PR revert 即可，结构上无 schema / 持久化层改动。