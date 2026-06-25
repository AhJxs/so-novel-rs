# CLI 模式

`so-novel-rs` 是 **CLI / GUI / Web 三模** 程序。`main.rs` 根据参数分发：

| 启动方式 | 行为 |
|---|---|
| `so-novel-rs` （无参数） | 启动 GPUI GUI 桌面客户端 |
| `so-novel-rs <子命令> ...` | 走 CLI 模式（本文档范围） |
| `so-novel-rs --web` | 启动 Web 服务器（[Web 模式](../README.md#-web-模式)） |

> CLI 与 GUI 共用同一份 `parser` / `crawler` / `export` / `http` 代码，行为完全一致。
> CLI 是"无人值守脚本 + 服务器 / 容器"场景的主力入口；GUI 用于交互浏览。

---

## 全局 flag

| Flag | 说明 |
|---|---|
| `-v, --verbose` | 打开内部 `tracing` 日志到 stdout（默认静默） |
| `-q, --quiet` | 抑制逐章进度 / 失败源 dump，脚本管道友好 |
| `-h, --help` | 简略帮助（只列子命令 + 主选项） |
| `--help` | 详细帮助（含 `long_about` + `after_help` 示例） |
| `-V, --version` | 打印版本号（`so-novel-rs X.Y.Z`）并退出 |

`-v` / `-q` 对 **所有子命令** 全局生效，位置不限（可在子命令前或后）。

---

## 子命令速查

| 子命令 | 用途 |
|---|---|
| [`search`](#search) | 搜索书源（聚合 / 单源 / `--json` 机器可读） |
| [`download`](#download) | 下载单本书（支持章节范围 / 输出目录 / 格式覆盖） |
| [`sources`](#sources) | 书源管理（list / enable / disable） |

`so-novel-rs --help` / `so-novel-rs <子命令> --help` 看完整选项和示例。

---

## `search`

按关键词在书源上跑搜索。默认 **聚合搜索**（所有启用书源并发跑），
`--source <ID>` 走单源。

```sh
# 聚合搜索（人类可读表格 + 失败源 dump 到 stderr）
so-novel-rs search "凡人修仙传"

# 单源（#5）
so-novel-rs search "凡人修仙传" --source 5

# 限制每源条数（覆盖 config.toml 的 search-limit）
so-novel-rs search "凡人修仙传" --limit 20

# 机器可读：纯 JSON 到 stdout，失败源信息走 stderr
so-novel-rs search "凡人修仙传" --json | jq '.[0].url'
```

### 行为

- **进度**：TTY 下原地刷新 `\r  🔍 搜索中… N/T 源 (P%)  关键词:《…》`，
  搜完自动擦除不留痕迹。管道 / 重定向 / `--quiet` 退回逐行。
- **结果过滤**：与 GUI 一致 —— `config.toml` 里 `search-filter = true` 时按相似度
  过滤 + 去重 + 排序。
- **失败源**：
  - ≤ 3 条 → 全部打印到 stderr（`✗ <name>#<id> 失败: <err>`）
  - > 3 条 → 打印前 3 + 摘要 `… 还有 N 条失败（用 --source 单源排查）`
  - `--quiet` → 完全跳过
- **`--json` 输出**：与 GUI `SearchResult` 同结构，可直接 `| jq` 处理。

---

## `download`

通过详情页 URL 下载整本书到本地。

```sh
# 全本下载
so-novel-rs download "https://www.xbiqugu.la/130/130718/"

# 指定书源（默认按 URL 自动匹配；不匹配则取第一个启用源）
so-novel-rs download "https://example.com/book/123" --source 5

# 覆盖下载目录 / 输出格式
so-novel-rs download "https://example.com/book/123" --output D:\novels --format epub

# 下载指定章节范围（1-based，闭区间）
so-novel-rs download "https://example.com/book/123" --from 100 --to 200

# 只指定起点 → 从 100 章下载到结尾
so-novel-rs download "https://example.com/book/123" --from 100

# 只指定终点 → 下载前 50 章
so-novel-rs download "https://example.com/book/123" --to 50
```

### 参数

| Flag | 行为 |
|---|---|
| `--source <ID>` | 强制走指定书源 |
| `--output <DIR>` | 覆盖 `config.toml` 的 `download-path` |
| `--format <epub\|txt\|html>` | 覆盖 `config.toml` 的 `ext-name` |
| `--from <N>` | 起始章节（1-based；省略 → 1） |
| `--to <N>` | 结束章节（1-based；省略 → 末章；**超出实际章数自动截断**） |

### 范围校验规则

| 输入 | 行为 |
|---|---|
| `--from 0` | ❌ 报错（1-based） |
| `--from 100` 但全书只 50 章 | ❌ 报错（明确越界） |
| `--to 9999` 但全书只 50 章 | ✅ 静默截断到 50（友好兜底） |
| `--from > --to` | 不会发生（`--from` 越界先 bail） |

### 行为

- **进度**：TTY 下原地刷新 `\r  ⏳ 已完成 N/T 章 (P%)  最新:《X》`，
  范围模式下会先打一行 `📖 《书名》by 作者 — 全 M 章，下载 A-B（共 C 章）`
  再开始。管道 / 重定向 / `--quiet` 退回逐行 / 静默。
- **打开方式**：下载完成后用系统默认应用打开（Windows `ShellExecute` /
  macOS `open` / Linux `xdg-open`）。`--quiet` 不影响这个行为。
- **取消**：TTY 下 `Ctrl-C` 触发 `CancelToken`，让 crawler 走 `Cancelled` 事件
  干净退出（不是硬杀进程）。**非 TTY 不会自动注册信号处理**（`Ctrl-C` 给父 shell）。
- **退出码**：`0` = 成功 / 取消；`1` = 下载失败（crawler 报错或
  `tasks.json` join 失败）。
- **章节文件目录**（`<output>/<book_dir_name>/`）在 `Cancelled` 早期会清理空目录；
  正常完成则由 exporter 合并后删除。

---

## `sources`

书源管理。书源状态（启用 / 禁用）持久化在
`~/.sonovel/sources_config.json` 的 `disabled_urls` 集合里（URL 为 key，因为
ID 在不同书源文件里可能不同 —— 见 [`persistent::sources_config`](../src/persistent/sources_config.rs)）。

```sh
# 列出所有书源（人类可读）
so-novel-rs sources list

# 列出所有书源（JSON 给 jq 用）
so-novel-rs sources list --json

# 旧版兼容：裸 `sources` 等价于 `sources list`
so-novel-rs sources
so-novel-rs sources --json    # 旧版兼容：等价于 `sources list --json`

# 禁用 / 启用指定书源
so-novel-rs sources disable 5
so-novel-rs sources enable 5
```

### `sources list` 输出格式

```
书源文件: /home/user/.sonovel/rules/main.json（启用 18 / 禁用 2）

  ✓ #  1 香书小说    [zh]  http://www.xbiqugu.la/
  ✗ #  5 顶点小说    [zh]  https://www.wxsy.net/
  ✓ #  7 悠久小说网  [zh]  http://www.ujxsw.org/
  ...
```

标记说明：
- `✓` / `✗` = 启用 / 禁用
- `[proxy]` = 该书源需要代理
- `[zh]` / `[en]` = 书源语言（来自规则文件）
- `[search]` = 该书源有 search 段

### `enable` / `disable` 行为

- **幂等**：重复 enable / disable 同一 ID 不报错（早退不写盘）。
- **未知 ID**：清晰报错 `找不到 ID=999 的书源`。
- **写盘**：原子写到 `~/.sonovel/sources_config.json`，GUI 侧
  `WebState::sources_config` 也会读到（共享同一文件）。

---

## 常用工作流

### 1. 在大量书源里找某一本书

```sh
so-novel-rs search "海贼王" --json | jq -r '.[] | "\(.source_name)\t\(.url)"'
```

### 2. 找到后直接下载

```sh
URL=$(so-novel-rs search "海贼王" --source 1 --json | jq -r '.[0].url')
so-novel-rs download "$URL" --format epub
```

### 3. 批量下载某个作者的所有书（jq + xargs）

```sh
so-novel-rs search "猫腻" --json \
  | jq -r '.[].url' \
  | xargs -n1 -I{} so-novel-rs download "{}" --quiet
```

### 4. 调试时打开 tracing 日志

```sh
so-novel-rs -v download "https://example.com/book/123" 2>&1 | tee download.log
```

### 5. CI / 容器内最小化输出

```sh
so-novel-rs -q download "$URL" --json-progress  # --json-progress 待实现
so-novel-rs -q download "$URL"                   # 当前：用 -q 完全静默，只看退出码
```

---

## 注意事项 / 已知坑

### 1. Windows 控制台与 ANSI 转义

- **TTY 模式才生效**：`in_place` 进度、Ctrl-C 取消注册等优化只在
  `stderr().is_terminal()` 为真时启用。管道 / 重定向 / IDE 终端里跑 CLI
  会自动退回"逐行打印"行为 —— 这是有意的（管道里进度行 `\r` 会污染 stdout）。
- **Ctrl-C 后的 prompt**：在原地进度模式下，`\r\x1b[K` 会改写光标位置。
  进程退出时我们补一个 `\n` 把光标推回新行（[src/cli/search.rs:154-160](../src/cli/search.rs#L154-L160)），
  shell prompt 会立刻回来，不需要再按一次 Enter。

### 2. `--json` 输出的稳定性

`search --json` 的 schema 与 GUI `SearchResult` 同结构（见
[`models::SearchResult`](../src/models/mod.rs)）。但 **没有承诺 v1.0 前的
schema 稳定性** —— 字段可能随迭代调整，CI 脚本要锁定具体版本号或
加容错（`jq -r '.[0] | .url // empty'`）。

### 3. 下载目录的副作用

- 下载会真的写文件到 `<config.toml: download-path>`，默认
  `~/.sonovel/`。
- 单章或中途取消的下载会留下 `<book_dir_name>/` 临时目录（crawler 内部
  `cleanup_chapters_dir_if_empty` 只清空目录）。
- 覆盖 `--output` 时要确认路径有写权限。

### 4. 章节范围 + 失败重试

- `crawler::EffectiveCrawl::max_retries` 控制单章失败重试次数（来自
  `config.toml: enable-retry`）。
- `--from` / `--to` 是按 `Chapter.order`（书源返回的章节号）过滤，
  不是按章节标题或下载顺序。如果某书源章节号不连续（比如 1-100 跳到
  150-200），过滤会跳过 101-149 的章节（这通常是书源没列全，不是 bug）。

### 5. `sources` 写盘时机

`sources enable` / `disable` 是 **同步阻塞** 写盘（一次 `SourcesConfig::save`
原子写）。在网络盘 / WSL 跨 fs 场景下可能略慢 —— `~/.sonovel/` 建议放
本地 fs。

### 6. 并发安全

- CLI 进程是单次执行，进程退出后所有 runtime 资源释放（`drop(rt)`）。
- `sources_config.json` 的并发写：GUI 和 CLI 同时跑 `sources enable` 时
  可能 last-write-wins。生产场景应该错峰（CI 单独跑 CLI，GUI 在 idle 时用）。

### 7. 退出码

| 退出码 | 含义 |
|---|---|
| `0` | 成功（含 `Cancelled` 早退） |
| `1` | 下载失败 / `tasks.json` join 失败 / 子命令内部 `bail!` |
| `2` | clap 解析失败（未知子命令、参数格式错误等） |
| `101` | panic（理论上不应该出现） |

### 8. 环境变量

CLI 不读 `SO_NOVEL_WEB`（那是 Web 模式的开关）。其他可调项都在
`config.toml` 里 —— 见 [`config.md`](./config.md)（如有）或直接
[`AppConfig` 默认值](../src/config/defaults.rs)。

---

## 故障排查

| 现象 | 可能原因 | 排查 |
|---|---|---|
| `找不到 ID=999 的书源` | ID 在 `sources_config.active_file` 文件里没匹配 | `so-novel-rs sources list` 查实际 ID |
| `没有可用的书源` | `rules_dir` 为空或全部禁用 | `so-novel-rs sources list` 看 `启用 0 / 禁用 N` |
| `--from 100 超出总章节数 (50)` | 不知道书的章数 | 用 GUI 预搜一次拿详情，或直接 `--to 9999` 让 CLI 截断 |
| `HTTP 错误: ... operation timed out` | 站点慢 / 网络问题 | 加 `--source <其它源ID>` 试别的源；或 `-v` 看 tracing |
| Ctrl-C 没反应（被硬杀） | 非 TTY（管道 / 后台跑） | 这是预期；用 `kill -INT <pid>` 走 POSIX 信号 |
| `--json` 输出多了一行 `🔍 搜索中… ...` | 在 TTY 跑 + 不带 `--quiet`，搜索中提示走 stderr；JSON 在 stdout 应该干净 | 加 `--quiet` 抑制；或 `2>/dev/null` |
| `Error: target ... already exists` | 同一 URL 下载两次，`<book_dir_name>/` 已存在 | 删旧文件或换 `--output` 目录 |

---

## 进一步阅读

- 顶层入口分发：[`src/main.rs`](../src/main.rs) `main()` 函数
- CLI 实现：[`src/cli/`](../src/cli/)
  - 参数定义：[`args.rs`](../src/cli/args.rs)
  - 入口：[`mod.rs`](../src/cli/mod.rs)
  - 共享工具（`truncate_chars` 已删除，章节范围校验）：[`util.rs`](../src/cli/util.rs)
- 底层复用：
  - 搜索聚合 / 流式：[`src/crawler/search.rs`](../src/crawler/search.rs)
  - 下载调度（`download_book` / `download_chapters` / `resolve_book`）：[`src/crawler/mod.rs`](../src/crawler/mod.rs)
  - 书源配置（`disabled_urls` 持久化）：[`src/persistent/sources_config.rs`](../src/persistent/sources_config.rs)
- 配置：[`src/config/`](../src/config/)
