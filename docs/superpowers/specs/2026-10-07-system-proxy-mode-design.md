# 代理模式：新增「使用系统代理」 — 设计文档

- 日期：2026-10-07
- 状态：已确认（用户对「Windows 系统代理作为来源 / 三态下拉 / 探不到即直连并常驻显示 / 非 Windows 读环境变量 / 启动 + 改设置 + 进设置页重读」全部接受）

## 背景与目标

`config.toml` 的 `[proxy]` 目前只有 `enabled = bool` + `host` + `port`：要开代理就得手抄一遍地址。
而用 Clash / v2ray 的用户，Windows「系统代理」开关早已把地址写进
`HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings`。让用户再抄一遍既啰嗦，
又容易在客户端换端口后失效（配置里还是旧端口，请求静默走直连/走死代理）。

目标：设置页代理项从「开关 + 地址」升级为**三态「代理模式」下拉**，新增
**使用系统代理**，直接复用系统里已生效的代理，并让用户看得到实际探到了什么。

**非目标（v1 明确不做）**：PAC 脚本下载与执行、`ProxyOverride` → `no_proxy` 的映射、
SOCKS 代理（`reqwest::Proxy::all` 走的是 http 代理）、`NO_PROXY` 环境变量的路由判断。

## 已确认决策

| 决策点 | 结论 |
|---|---|
| 「系统代理」指什么 | **Windows 系统代理** —— 读 WinINET 注册表（即 Clash/v2ray 切「系统代理」时写的那个）；非 Windows 读环境变量 |
| UI 形态 | **三态「代理模式」下拉**（不使用 / 手动配置 / 使用系统代理），替换原「启用 HTTP 代理」开关 |
| 配置键 | `proxy_mode = "none" \| "manual" \| "system"`，**取代** `enabled = true`；老配置自动迁移 |
| 选了「系统代理」但探不到 | **直连 + 设置页常驻显示探测结果**（不弹错误框、不阻塞）；探测结果无论当前模式都显示 |
| 探测到 PAC / 只探到 socks | **如实显示原因**（`PacOnly` / `SocksOnly`），不笼统说「未检测到」 |
| 非 Windows 平台 | 读 `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY`（含小写变体）；`NO_PROXY` 不参与判断 |
| 何时重新探测 | **启动时 + 改代理设置时 + 打开设置页时**（不做后台轮询） |

## 一、配置 schema 与迁移

### 1.1 `ProxyCfg` 改造（`src/config/types.rs:277`）

```rust
/// 代理模式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    /// 直连，不用代理。
    #[default]
    None,
    /// 用下方 `proxy_host` / `proxy_port`。
    Manual,
    /// 读系统代理（Windows 注册表 / 非 Windows 环境变量）。
    System,
}

impl ProxyMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Manual => "manual",
            Self::System => "system",
        }
    }

    /// 容错解析：无法识别一律当 `None`（手改 / 旧配置不 panic）。
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "manual" => Self::Manual,
            "system" => Self::System,
            _ => Self::None,
        }
    }
}

/// `[proxy]` 章节。代理配置。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyCfg {
    /// 代理模式。默认「不使用」。
    pub proxy_mode: ProxyMode,
    /// 代理主机地址（`Manual` 模式使用）。
    pub proxy_host: String,
    /// 代理端口（`Manual` 模式使用）。
    pub proxy_port: u16,
}
```

> `with_defaults()` 里 `proxy_enabled: false` → `proxy_mode: ProxyMode::None`；
> `proxy_host` / `proxy_port` 默认值（`127.0.0.1` / `7890`）不变。

> **不保留 `proxy_enabled` 字段**：留一个与 `proxy_mode` 语义重叠的 bool，就必须在每处读写点
> 维护「谁先谁后」的一致性，是典型的双真相源。迁移只在 IO 边界做一次（见 1.2）。

### 1.2 迁移（`src/config/toml_io.rs`）

`load_config` 中 `proxy` 段（现 168-176 行）改为：

```rust
// 新键 `mode` 优先；老配置只有 `enabled` 时按 true → manual / false → none 迁移。
if let Some(v) = t_str(&doc, "proxy", "mode") {
    cfg.proxy.proxy_mode = ProxyMode::parse(&v);
} else if let Some(v) = t_bool(&doc, "proxy", "enabled") {
    cfg.proxy.proxy_mode = if v { ProxyMode::Manual } else { ProxyMode::None };
}
if let Some(v) = t_str(&doc, "proxy", "host") {
    cfg.proxy.proxy_host = v;
}
if let Some(v) = t_int(&doc, "proxy", "port") {
    cfg.proxy.proxy_port = sat_u16(v);
}
```

`save_config` 中对应三行（现 357-359 行）改为：

```rust
set_str(&mut doc, "proxy", "mode", cfg.proxy.proxy_mode.as_str());
unset(&mut doc, "proxy", "enabled"); // 老键一次性清掉，避免留下失效项
set_str(&mut doc, "proxy", "host", &cfg.proxy.proxy_host);
set_int(&mut doc, "proxy", "port", cfg.proxy.proxy_port as i64);
```

`unset` 是已有的 helper（`source.search-limit` 在用），无需新增。
`toml_io.rs` 顶部 `use super::types::{...}` 需加 `ProxyMode`。

### 1.5 受影响的 `use` / 派生

| 文件 | 改动 |
|---|---|
| `src/config/mod.rs` | re-export `ProxyMode`（与 `ThemeKind` / `Language` 同处） |
| `src/config/toml_io.rs` | `use super::types::{..., ProxyMode}` |
| `src/http/client.rs` | 去掉 `#[cfg(test)] use crate::config::ProxyCfg`（测试改写后按需保留） |
| `src/http/clients.rs` | `ProxySignature` 改 `resolved` 后，`ProxyCfg` / `GlobalCfg` 的测试导入按新用法调整 |
| `src/core/bootstrap.rs` | `use crate::config::ProxyMode`（若该处尚未导入 types 里的枚举） |

### 1.3 默认模板（`src/config/defaults.rs:79`）

```toml
[proxy]
# 代理模式：none（直连）/ manual（用下面的 host、port）/ system（读 Windows 系统代理）
mode = "none"
host = "127.0.0.1"
port = 7890
```

### 1.4 兜底路径（`src/core/bootstrap.rs:74`）

`cfg_no_proxy.proxy.proxy_enabled = false;` → `cfg_no_proxy.proxy.proxy_mode = ProxyMode::None;`
（语义不变：兜底客户端走直连。）

## 二、新模块 `src/http/system_proxy.rs`

平台分支全部封在这个文件里，其余代码只看见 `resolve_proxy_url()` 与 `detect()`。
在 `src/http/mod.rs` 加 `pub mod system_proxy;` 并按该文件既有风格 re-export：
`pub use system_proxy::{AbsentReason, SystemProxy, detect, resolve_proxy_url};`，
调用点写 `crate::http::resolve_proxy_url(...)`（与 `pub use client::ClientOptions;` 一致）。

```rust
//! 系统代理探测。
//!
//! Windows：读 WinINET 注册表（Clash / v2ray 切「系统代理」写的就是这里）。
//! 其它平台：读环境变量。纯探测 + 纯解析，无网络 IO。

use crate::config::{ProxyCfg, ProxyMode};

/// 探测结果。`Found` 的字符串是可直接喂给 `reqwest::Proxy::all` 的 URL。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemProxy {
    Found(String),
    Absent { reason: AbsentReason },
}

/// 没探到代理的原因 —— 设置页如实展示，不糊成一句「未检测到」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsentReason {
    /// 系统代理开关是关的（`ProxyEnable = 0`）。
    NotEnabled,
    /// 只配了 PAC（`AutoConfigURL`），没有静态代理地址 —— PAC 脚本 v1 不执行。
    PacOnly,
    /// 只探到 socks 代理（`reqwest` 的 http 代理不支持）。
    SocksOnly,
    /// 非 Windows：`HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` 都没设。
    EnvUnset,
    /// 注册表 / 环境变量读取本身失败。
    ReadFailed,
}

/// 探测当前系统代理。任何失败都退化成 `Absent`，**不返回 Err**（调用方不应为探测失败而中断）。
pub fn detect() -> SystemProxy { /* cfg(windows) → detect_windows(), else → detect_env() */ }

/// 由配置算出实际要用的代理 URL。`None` = 直连。
pub fn resolve_proxy_url(p: &ProxyCfg) -> Option<String> {
    match p.proxy_mode {
        ProxyMode::None => None,
        ProxyMode::Manual => manual_url(p),
        ProxyMode::System => match detect() {
            SystemProxy::Found(url) => Some(url),
            SystemProxy::Absent { .. } => None,
        },
    }
}
```

`Manual` 的 URL 组装（host 空 = 用户没填，视为直连而不是构造 `http://:7890` 让 reqwest 报错）：

```rust
fn manual_url(p: &ProxyCfg) -> Option<String> {
    let host = p.proxy_host.trim();
    (!host.is_empty()).then(|| format!("http://{host}:{}", p.proxy_port))
}
```

### 2.1 Windows：注册表

`HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings` 下三个值：

| 值 | 类型 | 用途 |
|---|---|---|
| `ProxyEnable` | DWORD | `0` = 系统代理关 → `NotEnabled` |
| `ProxyServer` | REG_SZ | `host:port`，或 `http=h:p;https=h:p;...` 的按协议列表 |
| `AutoConfigURL` | REG_SZ | 只设了它 → `PacOnly` |

用 `winreg`（0.55.0 已在 `Cargo.lock` 里，经 `embed-resource` 的 build-dep 引入，
**不新增下载**）。Cargo.toml 新增：

```toml
[target.'cfg(windows)'.dependencies]
winreg = "0.55"
```

读取顺序：`ProxyEnable` 打不开/读不到 → `ReadFailed`；为 0 → `NotEnabled`；
`ProxyServer` 为空且有 `AutoConfigURL` → `PacOnly`；`ProxyServer` 为空且无 PAC → `NotEnabled`。

`ProxyServer` 的解析抽成**纯函数**，单元测试覆盖全部分支：

```rust
/// 解析注册表 `ProxyServer` 值，返回标准化的 `http://host:port`。
/// - `"127.0.0.1:7890"`           → `Some("http://127.0.0.1:7890")`
/// - `"https=1.2.3.4:8080;http=..."` → 取 https（无 https 则 http）
/// - `"socks=127.0.0.1:1080"`      → `None`（调用方据 `socks=` 前缀判 `SocksOnly`）
fn parse_proxy_server(raw: &str) -> Option<String>
```

判定 `SocksOnly`：`parse_proxy_server` 返回 `None` 且原串含 `socks=`（且不含 http/https 键）。

### 2.2 非 Windows：环境变量

按 `HTTPS_PROXY` → `HTTP_PROXY` → `ALL_PROXY` 顺序取第一个非空值（每项同时看大写的
`env::var` 和小写变体），直接作为代理 URL；全空 → `EnvUnset`。

> `NO_PROXY` 不解析：要正确实现得比对主机后缀，v1 不做（见「明确不做」）。
> 这是对「读环境变量」的一处收窄，spec 复核时可推翻。

## 三、client 集成与 rebuild 签名

### 3.1 `src/http/client.rs`（现 43-48 行）

```rust
if let Some(proxy_url) = crate::http::resolve_proxy_url(&cfg.proxy) {
    let proxy = reqwest::Proxy::all(&proxy_url)
        .with_context(|| format!("invalid proxy URL: {proxy_url}"))?;
    builder = builder.proxy(proxy);
}
```

`resolve_proxy_url` 对 `System` 模式每次调用都重新 `detect()` —— 这是「改设置 / 进设置页
时能感知到系统代理变化」的实现基础。

> `reqwest` 的 `system-proxy` feature 未启用（`default-features = false`），所以不调
> `.proxy()` 时是**纯直连**，环境变量不会被 reqwest 二次读取，语义确定。

### 3.2 `src/http/clients.rs`：`ProxySignature`（现 22-38 行）

```rust
/// 当前生效的**已解析**代理 URL。`None` = 直连。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxySignature {
    resolved: Option<String>,
}

impl ProxySignature {
    fn from_cfg(cfg: &AppConfig) -> Self {
        Self { resolved: resolve_proxy_url(&cfg.proxy) }
    }
}
```

**为什么存解析结果而不是原始配置**：`mode = system` 时 `(host, port)` 一个字都没变，但注册表
可能从「关」变成了「开」—— 比原始字段就会漏掉这次重建。比解析后的 URL 才能捕捉到。

`rebuild_proxy` 的其余逻辑（短路比较、双 client 原子替换、锁处理）不动。

## 四、刷新时机

### 4.1 `AppModel` 增加探测快照

```rust
/// 最近一次系统代理探测结果（设置页只读展示用，与 `proxy_mode` 无关）。
pub system_proxy: SystemProxy,

/// 重探系统代理并（必要时）重建 client。启动 / 改代理设置 / 进设置页时调用。
pub fn refresh_system_proxy(&mut self) {
    self.system_proxy = crate::http::detect();
    // 快照变了但 URL 没变时 rebuild_proxy 内部会短路，不会白白重建连接池。
    if let Err(e) = self.http.rebuild_proxy(&self.config) {
        let msg = format!("HTTP client 重建失败（配置已保存）: {e}");
        tracing::warn!("{msg}");
        self.push_error(msg);
    }
}
```

`system_proxy` 在 `AppModel::new` 时做首次 `detect()`，设置页第一帧就有内容。

### 4.2 三个触发点

| 时机 | 位置 | 说明 |
|---|---|---|
| 启动 | `HttpClients::new` 已按 `cfg` 建 client → `resolve_proxy_url` 自动生效 | 无需额外 hook；`AppModel::new` 里补一次 `detect()` 供 UI 展示 |
| 改代理设置 | `dropdown_field` 的 setter 已内含 `model.persist_settings()` → `rebuild_proxy()` | 切换模式**本身就会重建 client**；`after_set` 只负责刷新快照 + 令本页重绘（见五） |
| 打开设置页 | `desktop/root.rs::navigate()` | 见下 |

```rust
fn navigate(&mut self, page: NavPage, cx: &mut Context<Self>) {
    if self.current_page != page {
        self.current_page = page;
        if page == NavPage::Settings {
            // 用户可能在别处（Clash 等）刚改过系统代理，进设置页时重探一次。
            self.model.update(cx, |m, _| m.refresh_system_proxy());
        }
        cx.notify();
    }
}
```

> **为什么不在 `render()` 里探测**：`SettingsPage::render` 每帧执行、每帧都会调
> `build_pages`，把它当探测时机等于每帧一次注册表 IO。`navigate()` 是「打开设置页」这个
> 事件唯一的入口（只渲染当前页，切页必经此处）。

> **`cx.notify()` 足以让设置页重绘**：`RootView::render` → `render_current_page()` 产出的是
> `Entity<SettingsPage>` 的 `ViewElement`，其 `request_layout` 走 `_` 分支**无条件**调用子
> view 的 `render`（gpui-pre `src/view.rs:438`，`cached_style` 为 `None` 时不短路）。
> 因此 root 脏了，子树（含 `SettingsPage::render` → `build_pages`）同帧重跑，快照是新的。

## 五、UI（`src/desktop/pages/settings/page_proxy.rs`）

`build(ctx, cx)` 里把 `_cx` 改名为 `cx`，读一次当前模式：

```rust
let mode = ctx.model.read(cx).config.proxy.proxy_mode;
```

### 5.1 「代理模式」下拉

替换原 `bool_field` 那一项（现 27-35 行）：

```rust
SettingItem::new(
    t!("Settings.item.proxy_mode"),
    dropdown_field(
        vec![
            (SharedString::from("none"), t!("Settings.proxy_mode.none")),
            (SharedString::from("manual"), t!("Settings.proxy_mode.manual")),
            (SharedString::from("system"), t!("Settings.proxy_mode.system")),
        ],
        &m,
        move |model| SharedString::from(model.config.proxy.proxy_mode.as_str()),
        move |model, v| model.config.proxy.proxy_mode = ProxyMode::parse(&v),
        Some(after_proxy_mode),
    ),
)
.description(t!("Settings.desc.proxy_mode").to_string()),
```

`after_set` 是 `fn` 指针（非捕获闭包）：

```rust
/// 代理模式切完：刷新探测快照，并强制本帧重绘，让 host/port 的置灰态立即跟上。
fn after_proxy_mode(model: &Entity<AppModel>, cx: &mut App) {
    model.update(cx, |m, _| m.refresh_system_proxy());
    cx.refresh_windows();
}
```

> `refresh_windows()` 推 `Effect::RefreshWindows` → `window.refresh()` → 窗口整体 dirty，
> 子树同帧重跑（同 4.2 的机制）；只靠 `model.update` 的 notify 不保证 `SettingsPage` 被标记。

### 5.2 host / port 置灰

两项包一层 `SettingItem::disabled(!matches!(mode, ProxyMode::Manual))`
（`opacity(0.5)` + 透传给自定义 renderer，见 gpui-component `setting/item.rs:131`）：

```rust
SettingItem::new(t!("Settings.item.proxy_host"), string_field(/* 原样 */))
    .description(t!("Settings.desc.proxy_host").to_string())
    .disabled(!matches!(mode, ProxyMode::Manual)),
```

`build_pages` 每帧调用，`mode` 每帧重读 —— 置灰态天然是动态的。

### 5.3 探测结果只读行

同 group 追加一项，标题「系统代理检测」，内容为快照渲染（`SettingField::render` + 一段 `div`）：

| 快照 | 展示 |
|---|---|
| `Found(url)` | `已检测到：http://127.0.0.1:7890` |
| `Absent { NotEnabled }` | `系统代理未开启，将直连` |
| `Absent { PacOnly }` | `只检测到 PAC 配置（暂不支持），将直连` |
| `Absent { SocksOnly }` | `只检测到 SOCKS 代理（暂不支持），将直连` |
| `Absent { EnvUnset }` | `未设置 HTTPS_PROXY / HTTP_PROXY / ALL_PROXY，将直连` |
| `Absent { ReadFailed }` | `读取系统代理失败，将直连` |

**无论当前模式**都显示 —— 用户切到「手动配置」时也能看见系统代理里到底有什么。

## 六、i18n（`locales/app.yml`）

三语同步（`en` / `zh-CN` / `zh-TW`）。

| 段 | key | 处理 |
|---|---|---|
| `Settings.group.http_proxy`（~173） | — | 保留 |
| `Settings.item.proxy_enabled`（274） | `proxy_mode` | **改键**：标题「代理模式」/ "Proxy mode" |
| `Settings.item.proxy_host` / `proxy_port`（278/282） | — | 保留 |
| `Settings.desc.proxy_enabled`（391） | `desc.proxy_mode` | **改键**：「直连 / 手填地址 / 复用系统代理」 |
| `Settings.proxy_mode.{none,manual,system}` | 新增 | 三个下拉项文案 |
| `Settings.item.system_proxy_status` | 新增 | 「系统代理检测」 |
| `Settings.desc.system_proxy_status` | 新增 | 上面那张表的 6 条结果文案 |

`Settings.desc.proxy_host` / `proxy_port` 措辞微调为「仅手动配置模式生效」。

## 七、测试

纯函数层（不开窗口、不碰网络）：

| 位置 | 用例 |
|---|---|
| `http/system_proxy.rs` | `parse_proxy_server` 裸 `host:port` / `https=h:p;http=h:p`（取 https）/ 仅 `http=` / 仅 `socks=`（`None`）/ 空串 / 带空格 |
| `http/system_proxy.rs` | `manual_url`：正常 host、空 host（`None`）、host 带空格（trim） |
| `http/system_proxy.rs` | `resolve_proxy_url`：`None` 模式恒 `None`（与 host/port 无关） |
| `config/toml_io.rs` 或 `config/tests.rs` | 迁移：只有 `enabled = true` 的老文件 → `Manual`；`enabled = false` → `None`；`mode = "system"` 优先于 `enabled`；save 后文件里 `enabled` 消失、`mode` 存在 |
| `config/tests.rs:78` | 往返测试改 `proxy_mode: ProxyMode::Manual`，断言 `mode`；加一条 `ProxyMode::System` 不落 host/port 也往返相等 |

已有测试 `client.rs::proxy_enabled_actually_routes_traffic_through_proxy` 改为构造
`ProxyMode::Manual` 的配置 —— 逻辑不变，仍是「真起一个 mock proxy 看请求行」。

## 八、明确不做

- PAC 脚本下载 / 执行（`AutoConfigURL`）—— 只如实报告 `PacOnly`
- `ProxyOverride` → `no_proxy` 的映射
- SOCKS 代理 —— 只报告 `SocksOnly`
- `NO_PROXY` 环境变量的主机后缀匹配
- 后台轮询系统代理变化（只在既有三个时机重探）
- 代理健康检查 / 连通性测试 UI

## 九、验证方式

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --lib
grep -rn "proxy_enabled" src/ assets/ docs/ locales/    # 应无输出
uv run python -c "import yaml,sys; yaml.safe_load(open('locales/app.yml',encoding='utf-8'))"
```

手工（Windows，Clash 开着系统代理）：

1. 启动 → 设置页「系统代理检测」显示 `已检测到：http://127.0.0.1:7890`
2. 模式选「使用系统代理」→ 在 Clash 里换端口 + 关开系统代理 → 再次进入设置页 → 展示同步更新
3. 模式选「手动配置」→ host/port 可编辑；选「不使用」/「使用系统代理」→ 两项置灰
4. 关掉系统代理后选「使用系统代理」→ 显示「系统代理未开启，将直连」，搜书/下载正常（走直连）
5. 老 `config.toml`（`enabled = true` + host/port）→ 启动后被迁移成 `mode = "manual"`，`enabled` 键消失

## 风险与对策

| 风险 | 对策 |
|---|---|
| 注册表 `ProxyServer` 格式花样多（按协议列表、大小写、空格） | 解析抽成纯函数，用例覆盖裸串 / 按协议 / socks-only / 空串；解析不出来一律 `ReadFailed`/`Absent`，不 panic |
| 探测失败被当成硬错误阻塞 UI | `detect()` 恒返回 `SystemProxy`（不返回 `Result`），UI 侧只展示不报错 |
| 系统代理变了但没重探 | 触发点三处齐全；`ProxySignature` 比**解析后 URL** 而非原始字段，`refresh_system_proxy` 调用即能感知 |
| `mode = system` 时端口变了仍用旧 client | 同上一行：签名含解析结果，`rebuild_proxy` 会真重建 |
| `loading` 期注册表 IO 拖慢进设置页 | 只在 `navigate()` 事件触发（非每帧），一次注册表读是微秒级 |
| winreg 引入新依赖 | 0.55.0 已在 `Cargo.lock`（`embed-resource` build-dep），仅提升为 Windows 运行时依赖 |
| 老配置静默失效 | `load_config` 的 `enabled` 迁移 + 九节的 5 号手工用例显式覆盖 |
