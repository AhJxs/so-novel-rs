# 项目 lessons

## 不要在 `cx.spawn` 内部用 tokio 原语

**症状**：`panic at ...: there is no reactor running, must be called from the context of a Tokio 1.x runtime` + `STATUS_STACK_BUFFER_OVERRUN`（Windows 上 panic 跨 FFI 边界 unwind 时退化成 abort）。

**根因**：`gpui::Context::spawn` / `cx.spawn(async move |...| {...})` 把 future 交给 gpui 自己的 executor —— 这个 executor 是基于 **smol**（不是 tokio）实现的（`gpui-0.2.2/src/executor.rs` 顶层 `spawn_local_with_source_location`，底层 `async-task` + smol 风格的 `Runnable`）。

future 内部 await 的 timer / channel 必须用 smol 系：
- 定时器：`async_cx.background_executor().timer(Duration).await` —— 走 smol reactor。
- 通道：`smol::channel::bounded<T>(cap)` —— 内部基于 `async-channel`，收发都是 `async/await` 友好的。
- `try_recv()` 返回 `Result<T, smol::channel::TryRecvError>`，`TryRecvError` 有 `Empty` / `Closed` 两个变体（`Closed` 表示 sender 已 drop → 任务退出）。

**不要用**：
- `tokio::time::sleep` — 没有 tokio reactor。
- `tokio::sync::mpsc::channel` — 同上，且 `tokio::sync::mpsc::Sender::try_send` 的语义跟 smol 不同。

**当前 runtime 现状（已与早期 lessons 更新）**：GUI 模式下两套 executor 并存——gpui 的 smol executor 跑 `cx.spawn`（UI 侧），一个 leaked tokio runtime（`src/app/runtime.rs` 的 `build_shared_runtime`，`Box::leak` 永不 drop）跑网络任务（搜索/下载/封面/健康检查）。tokio↔smol 边界靠 `try_recv()`（runtime 无关）桥接：tokio 侧 spawn 后向 mpsc 发事件，smol 侧的 drain 循环 `try_recv` 排空。**关键约束不变**：`cx.spawn` 内部（smol 侧）绝不能用 `tokio::time::sleep` / `tokio::sync::mpsc::recv().await`，会 panic。CLI 模式各自建临时 tokio runtime（`src/cli.rs`），进程退出即销毁。

**正确 pattern（参考）**：
- `src/app/events.rs:97-119` —— `spawn_drain_loop` 用 `async_cx.background_executor().timer(...)` 做 100 ms 心跳。
- `gpui-component-0.5.1/src/theme/registry.rs:171-206` —— `ThemeRegistry::_watch_themes_dir` 用 `smol::channel::bounded(100)` 给 `notify::recommended_watcher` 回调通信。

**Cargo.toml 需要的直接依赖**：`smol = "2"`。项目目前只有间接依赖（gpui-component 0.5.1 → smol 2）。

## `tokio::sync::mpsc::Sender` 不能跨 smol ↔ tokio

跟上一条相关：即便在普通 Rust 函数（非 `cx.spawn` 内部）里建 `tokio::sync::mpsc::Sender`，如果 sender 的对端 receiver 在 `cx.spawn` future 里 `await tokio::sync::mpsc::Receiver::recv()`，同样 panic。始终用 smol 通道。

## `select!` 宏跨 runtime 不可用

smol 没有自己的 `select!` 宏，tokio 的 `tokio::select!` 在 smol future 里也用不上。两种替代：
- 轮询：`loop { timer.await; try_recv().for_each(...) }`（结构化差但简单够用）。
- 真正的 select：`futures::future::FutureExt` 提供 `select`，但 `futures` crate 不在直接依赖里。需要时再加。

## 在 `InputEvent::Change` 订阅里调 `set_value` 会死循环，耗尽 Windows 句柄配额崩溃

gpui-component 0.5.1 的 `InputState::set_value` → `replace_text` → `cx.emit(InputEvent::Change)`（state.rs:2009）。所以**订阅 `InputEvent::Change` 后在处理器里无条件 `set_value` 会形成 Change→set_value→Change 死循环**。

症状：日志狂刷 `Error { code: HRESULT(0x80070718), message: "配额不足，无法处理此命令。" }`，进程 `exit code: 0xcfffffff`。`0x80070718` = `ERROR_NOT_ENOUGH_QUOTA`（Windows 桌面堆 / 句柄配额耗尽）——死循环每轮创建句柄，几秒内打满。

**正确 pattern**：Change 处理器里只在 clamp / 规整后的值与当前显示值**不同**时才 `set_value`：
```rust
InputEvent::Change => {
    let cur = input.read(cx).value().to_string();
    let want = normalize(&cur);            // clamp / 格式化
    if want != cur {                        // 相等就跳过，断开重入循环
        input.update(cx, |s, cx| s.set_value(want, window, cx));
        cx.notify();
    }
}
```
`set_value` 写回的值已是规整后的，二次 Change 进来时 `want == cur` 直接跳过，循环立即终止。

**对照**：`search.rs` 关键词输入框在 Change 里只更新 model、不调 `set_value`，所以没事；选章起止输入框要在 Change 里 clamp 写回，踩了这个坑。`NumberInputEvent::Step`（按 +/-）不受影响——`set_value` 只 emit `Change` 不 emit `Step`，不会回环。

**通用教训**：任何「订阅事件 A → 在处理器里调用会再次 emit 事件 A 的 API」都要加去重守卫，否则就是隐式递归。GPUI / gpui-component 里 `set_value` / `set_placeholder` 这类带 emit 的 setter 都适用。

## 累积分式信号会被"无上限输入"噪声稀释翻车（已用 per-result hybrid 彻底规避）

**症状**：用户报告"设置 `search-limit=30` 能搜到正确结果，`-1` 反而过滤掉一堆"。第一反应以为是数量问题，实际是**判定算法**被噪声稀释。

**根因（旧算法）**：判定"用户在搜哪个字段"用 `book_score = sum(similar(kw, book_name_i))` vs `author_score = sum(similar(kw, author_i))`，纯累积分。`search-limit=-1` → `Option<usize>` = `None` → parser 返回**所有**结果，单源 100+ 条。噪声条目作者名碰巧与 kw 共享字符（如搜"凡人修仙传"，噪声作者"忘传说"/"凡人传"），作者侧累积分轻松盖过书名侧 → `by_book_name` 翻 `false` → 正确书名匹配 `field_similar(sr, kw, false) ≈ 0` 被 0.3 阈值砍掉。

**最终修复**（v0.2.5，`parser::search_filter.rs`）：
1. **彻底砍掉累积分** —— 没有任何 `aggregate_score`，每条结果独立打分。
2. **per-result hybrid score**（`calculate_hybrid_score`）：`max_sim * 0.9 + min_sim * 0.1`，主信号 90% + 辅信号 10%。完全匹配特判 1.0。
3. **`similar` 加子串包含度**：如果 `target.contains(kw)`，基础分 `0.6 + (kw_len/tg_len) * 0.4`（0.6 ~ 1.0 区间），避免长书名（如"史上第一混混之凡人修仙前传"）用纯编辑距离算出 0.14 被 0.3 阈值误杀。
4. **硬阈值放宽到 0.25**：子串保底 0.6 后阈值可以更宽。

为什么这个方案优于"加单条最佳信号 + 累积分兜底"的中间方案：根本问题不是"如何让累积分不被翻"，而是"任何累积分都是错的" —— 正确结果天然独立，噪声稀释不该影响它。直接 per-result 打分就消除了"数量"这个维度，结构上免疫。

**通用教训**：
- 任何对**输入数量敏感**的信号（累积分 / 平均分 / 投票数 / 全局聚合）都该警惕"数量稀释"问题。第一反问："我能不能 per-item 打分，最后只 sort 不过 aggregate？" —— 如果可以，per-item 是更优的。
- "取 max + min" 的 hybrid（90/10）处理多字段独立检索的常见 pattern：主信号（max）保强匹配不被弱信号拖累，辅信号（min）提供"两字段都沾点边"的轻微 boost。比"二选一判定字段"更鲁棒。
- `similar(a, b)` 函数**永远要带子串包含度优化**：纯编辑距离在长 target + 短 kw 时会被长度稀释（如 `similar("凡人", "史上第一混混之凡人修仙前传") ≈ 0.14`）。这种"kw 是 target 子串"语义是用户最常见的搜索意图，必须有保底分。
- 配置项 sentinel 设计：`-1` 当 sentinel 表示"未指定"是个老坑，必要时把"无上限"和"未指定"分开（用 `None`/`Some(0)` 分别表达，0 = "无上限且允许"，None = "用户没设"）。
- 修这类 bug 的判定测试要用**真实构造的场景**（百条噪声 + 1 条正确匹配），断言正确匹配不被切 —— 比"两个 mock 比相似度"覆盖度高得多。

## 跨层 helper 提取：`i18n::ts` 取 `&'static str`，不是 `&str`

**症状**：在 `src/util/formatting.rs` 写 `pub fn format_local_unix_secs(secs: i64, unknown_key: &str, ...)` 编译报 3 个 `E0521 borrowed data escapes outside of function`，指 `ts(unknown_key)` 要求参数 `'static`。

**根因**：`rust_i18n::t!(key)` / 我们的 `crate::i18n::ts(key)` 在底层宏展开成静态查表，签名要 `&'static str`（字面量）。`&str` 默认是任意生命周期，rustc 没法在调用点证明它 `'static`。

**正确 pattern**：helper 签名直接 `unknown_key: &'static str` —— 调用方传字符串字面量（"Library.time.unknown"）就过；想传 String 的 caller 必须 `.leak()` 或改用 `String` key（我们不要）。同理 `ts_fmt` 占位符值可以是 `&str`（运行时拼），但 key 必须是字面量。

**通用教训**：
- 任何 wrapper 接受 i18n key 的 helper，参数类型都用 `&'static str`，省一次踩坑。
- 写完后先 `cargo build` 再 commit —— 这次 2 个错（缺 `StatusKind` import + `&str` 生命周期）都是 5 秒能发现的，不该留到 PR 末端。

## 跨层 UI 枚举映射：domain enum 不返回 `StatusKind`

**场景**：把 `health_status_kind` 从 `gpui_app/pages/sources.rs` 提到 `crawler/health.rs` 作为 `impl SourceHealth` 方法。

**反模式**：让 `crawler` 模块 `use crate::gpui_app::components::StatusKind` —— 引入 `gpui_app` 依赖，**破坏 layering**（domain 不知道 GUI 的存在）。

**正模式**：domain 层定义自己的小枚举（`HealthStatus { Ok, Redirect, BadResponse, ProbeError, NetworkError }`），UI 层做一行 `match` 翻译成 `StatusKind`。domain 可以 `use crate::i18n`（顶层，无 GUI 依赖），但不能 `use crate::gpui_app::*`。

**优点**：
- layering 干净（crawler 是纯 domain，未来要换 UI 框架 / 加 CLI 前端都不动）
- 5 个语义状态在 domain 里有"一等公民"地位（比直接 1:1 映射到 StatusKind 多保留了一个 NetworkError vs ProbeError 的区分）
- 单元测试不需要起 GPUI App，纯域值断言即可

**通用教训**：DRY 不等于"把所有逻辑都搬到 domain"。能搬到 domain 的只有**业务语义**；**UI 表现**（颜色、动画、布局）永远留在 UI 层。两层用"小 enum"沟通，不让 domain 直接吐 UI 类型。

