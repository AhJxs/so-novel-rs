# 代理三态模式（含「使用系统代理」）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把设置页的「启用 HTTP 代理」开关换成三态「代理模式」下拉（不使用 / 手动配置 / 使用系统代理），并让「使用系统代理」在 Windows 上读 WinINET 注册表（Clash / v2ray 的「系统代理」开关写的就是它）、在其它平台读 `HTTPS_PROXY` / `HTTP_PROXY` 环境变量。

**Architecture:** 配置层把 `proxy_enabled: bool` 换成 `proxy_mode: ProxyMode`（`none` / `manual` / `system`），旧键自动迁移；新增 `src/http/system_proxy.rs` 作为唯一探测点，暴露 `detect()` + `resolve_proxy_url()` —— 后者把「三态模式」压成 `Option<String>`（`None` = 直连），`client.rs` 与 `clients.rs` 的 `ProxySignature` 都只认这个结果。UI 层 `AppModel` 持有一份探测快照供设置页展示，刷新触发点是启动 / 改代理模式 / 进入设置页三处（不做后台轮询）。

**Tech Stack:** Rust 2024（单一 lib crate）；`reqwest` 0.13（`Proxy::all`，**未**开 `system-proxy` feature，所以系统代理必须自己读）；`winreg` 0.55（Windows 注册表，已在 `Cargo.lock` 中，无新下载）；`toml_edit`（保留注释的配置读写）；`rust-i18n`（`%{name}` 插值）；gpui-kit 0.7.1 / gpui-component 0.7.1（`SettingItem::disabled` / `SettingField::dropdown` / `SettingField::render`）。

设计文档：`docs/superpowers/specs/2026-10-07-system-proxy-mode-design.md`

## Global Constraints

以下约束适用于**每一个** Task：

- **不提交**：`tasks/lessons.md` L1 规定「只有用户明确说"提交"或"commit"时才执行 `git commit`」；L12 进一步规定**任何 skill 里的 `git commit` / `git add` 步骤一律降级为"只写文件、不提交"**。**本计划的任何步骤都不含 `git commit` / `git add`**。每个 Task 完成后停下，等待用户确认。
- **质量门（每个 Task 收尾必跑）**：
  ```bash
  cargo fmt --all -- --check
  cargo clippy --all-targets -- -D warnings
  cargo test --lib
  ```
  `src/lib.rs` 里有 `#![warn(clippy::pedantic, clippy::nursery)]` + `#![warn(dead_code)]`，加了 `-D warnings` 后 **0 警告是硬要求**。
- **测试基线（2026-10-07 实测）**：`cargo test --lib` = **438 passed / 0 failed / 4 ignored**。每个 Task 结束时数字只能增加，不能减少。
- **平台可达性**：Windows 是主平台，但**非 Windows 也必须能编译**（CI 的 release workflow 与本地跨平台检查都要过），所以平台分支一律用 `#[cfg(target_os = ...)]` 门控，且不能触发 `dead_code`。
- **注释风格**：中文，只在「不显然」的地方写；不解释代码字面意思。所有新增 `pub fn` 不加 `#[tracing::instrument]`（本仓库只对 IO 边界函数加）。
- **依赖**：**不新增**除 `winreg`（仅 Windows，且已在 `Cargo.lock`，版本 0.55.0）以外的任何依赖。不加 feature flag。
- **命令都在仓库根执行**：`C:\Users\pc\Documents\GitHub\so-novel-rs`。

### 关于 TDD

本计划是**配置 schema 变更 + 新增探测模块 + UI 改造**的组合，TDD 循环只在能纯逻辑测试的地方成立：

1. **有纯逻辑可测的**：`parse_proxy_server`（WinINET 值解析）、`resolve_proxy_url`（三态 → URL）、`ProxyMode::parse`、`load_config` 的旧键迁移 —— 一律先写失败测试再实现。
2. **不可确定性测试的**：真实注册表探测结果（依赖用户机器上 Clash 开没开）。替代证据是**探测函数的输入被抽成纯函数**（`parse_proxy_server`），注册表读取只剩三行 `get_value` 调用。
3. **每 Task 结束跑质量门**并把 `cargo test --lib` 的数字与上一步对比 —— 数字不涨说明测试没接上。
4. UI 改动（Task 3）靠**手动验证步骤**收尾（GUI 无法自动化），手动步骤写在 Task 3 末尾，明确标注"需要人工执行"。

### 与设计文档的差异（已核实，计划按此执行）

| # | 设计文档写的 | 实际情况 | 处理 |
|---|---|---|---|
| 1 | i18n 键 `Settings.proxy_mode.{none,manual,system}` | 本仓库既有约定是**下拉选项文案统一放 `Settings.option.<字段>.<值>`**（见 `locales/app.yml:443-473` 的 `option.theme_kind.*` / `option.theme_dyn_mode.*`，`page_general.rs:51-60` 引用） | 改用 `Settings.option.proxy_mode.{none,manual,system}`，与既有约定一致 |
| 2 | `http/mod.rs` 里 `pub use system_proxy::{..., detect, ...}` | `crate::http::detect()` 命名过泛，读调用点看不出在探测什么 | re-export 时重命名为 `detect as detect_system_proxy`，调用点写 `crate::http::detect_system_proxy()`；模块内的函数名仍是 `detect` |
| 3 | 「设置页**常驻**显示探测结果」 | 若不选「使用系统代理」，这一行永远是「未检测到」的噪声，会让人以为功能坏了 | 该行**只在 `proxy_mode == System` 时渲染**。「常驻」按"内联、不弹对话框、不阻断"理解（这才是与"弹 error dialog"的对照面） |
| 4 | 未提 `SettingItem::disabled` 的实现方式 | `gpui-component-0.7.1/src/setting/item.rs:131` 确有 `disabled(bool)`，对 `Item` 变体转发给底层 field 的非交互态；`build_pages` 每帧被 `SettingsPage::render` 调用（`mod.rs:331`），所以每帧重算 `disabled` 是可行的 | 直接用 `SettingItem::disabled(mode != ProxyMode::Manual)`，不需要额外状态 |
| 5 | 未提 `parse_proxy_server` 的 `dead_code` 风险 | 仓库是 **lib crate**（`src/lib.rs` 有 `pub mod http;`），从 crate 公共 API 可达的 item **不报** `dead_code`；但 `parse_proxy_server` 是私有函数，其唯一生产调用点在 `#[cfg(target_os = "windows")]` 里，非 Windows 构建下只被测试用 | 给 `parse_proxy_server` 加 `#[cfg_attr(not(target_os = "windows"), allow(dead_code, reason = "..."))]`（`reason` 写法仓库已有先例：`src/parser/dom/transform.rs:47`） |
| 6 | 未提 `ProxySignature` 改存 URL 后 `HttpClients::empty()` 的行为变化 | 旧签名存 `(enabled, host, port)`，`empty()` 的 `(false, "", 0)` 与默认配置的 `(false, "127.0.0.1", 7890)` **不相等**，所以 `empty()` 之后第一次 `rebuild_proxy` 会真重建；新签名存解析后 URL，两者都是 `None`，**不重建** | 视为修复（`empty()` 本来就是兜底桩），无代码影响，无测试依赖该行为 |

---

## Task 1: 配置三态化 + 系统代理探测模块 + client 接线（原子切换）

必须原子完成：`ProxyCfg.proxy_enabled` 一改名，`client.rs` / `clients.rs` / `bootstrap.rs` / `config/tests.rs` / `page_proxy.rs` 同时编译失败。任何一步单独提交都会留下编译不过的中间态。

**Files:**
- Modify: `src/config/types.rs:277-330`
- Modify: `src/config/toml_io.rs:14`、`168-176`、`357-359`
- Modify: `src/config/mod.rs:17-20`
- Modify: `src/config/defaults.rs:79-82`
- Modify: `src/config/tests.rs:13-16`、`78-82`、`101`
- Create: `src/http/system_proxy.rs`
- Modify: `src/http/mod.rs:5-26`
- Modify: `src/http/client.rs:11-13`、`43-48`、`70-82`、`94-128`
- Modify: `src/http/clients.rs:1-38`、`57-68`、`159-206`、`234-259`
- Modify: `src/core/bootstrap.rs:74-89`
- Modify: `src/desktop/pages/settings/page_proxy.rs:14`、`27-35`（**临时改动，Task 3 会整段重写**）
- Modify: `Cargo.toml:108-110`（追加 target 依赖段）

**Interfaces:**
- Produces:
  - `crate::config::ProxyMode`（`None` / `Manual` / `System`，有 `as_str() -> &'static str` 与 `parse(&str) -> Self`）
  - `crate::config::ProxyCfg { proxy_mode: ProxyMode, proxy_host: String, proxy_port: u16 }`
  - `crate::http::SystemProxy`（`Found(String)` / `Absent { reason: AbsentReason }`）
  - `crate::http::AbsentReason`（`NotEnabled` / `PacOnly` / `SocksOnly` / `EnvUnset` / `ReadFailed`）
  - `crate::http::detect_system_proxy() -> SystemProxy`
  - `crate::http::resolve_proxy_url(&ProxyCfg) -> Option<String>`
- Consumes: 无（本 Task 是链路起点）。

- [ ] **Step 1: 记录改动前基线**

Run:
```bash
cargo test --lib 2>&1 | tail -3
```
Expected: `test result: ok. 438 passed; 0 failed; ... 4 ignored`（只关心 `438 passed`）。数字不是 438 说明工作区与计划基线有偏差 —— **停下并告知用户**。

- [ ] **Step 2: `src/config/types.rs` —— 新增 `ProxyMode`，改造 `ProxyCfg`**

把 277-285 行的 `ProxyCfg` 整段替换为（注意 `ProxyMode` 定义在 `ProxyCfg` **之前**）：

```rust
/// 代理模式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    /// 直连，不用代理。
    #[default]
    None,
    /// 用下面的 `proxy_host` / `proxy_port` 手动配置的 HTTP 代理。
    Manual,
    /// 读操作系统代理设置：Windows 读 WinINET 注册表（Clash / v2ray 的「系统代理」
    /// 开关写的就是它），其它平台读 `HTTPS_PROXY` / `HTTP_PROXY` 环境变量。
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

    /// 解析 TOML 里的字符串。无法识别（含空串）→ [`Self::None`]，与 `ThemeKind::parse` 风格一致。
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "manual" => Self::Manual,
            "system" => Self::System,
            _ => Self::None,
        }
    }
}

/// `[proxy]` 章节。代理模式 + 手动代理地址。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyCfg {
    /// 代理模式。决定 `crate::http::resolve_proxy_url` 的行为。
    pub proxy_mode: ProxyMode,
    /// 仅 `Manual` 模式使用的代理主机地址。
    pub proxy_host: String,
    /// 仅 `Manual` 模式使用的代理端口。
    pub proxy_port: u16,
}
```

再把 `with_defaults()` 里 324-328 行的 `proxy` 字段替换为：

```rust
            proxy: ProxyCfg {
                proxy_mode: ProxyMode::None,
                proxy_host: "127.0.0.1".to_string(),
                proxy_port: 7890,
            },
```

- [ ] **Step 3: `src/config/toml_io.rs` —— 读侧迁移 + 写侧换键**

(a) 第 14 行 import 加上 `ProxyMode`：

```rust
use super::types::{AppConfig, ExportFormat, Language, ProxyMode, ThemeDynMode, ThemeKind, ThemePref};
```

(b) 168-176 行的 proxy 读取块替换为：

```rust
    // 旧键 `[proxy].enabled`（bool）迁移到新键 `[proxy].mode`（字符串）：`mode` 优先。
    // 两个键都在时以 `mode` 为准（用户手改过 mode 说明他知道新键）。
    if let Some(v) = t_str(&doc, "proxy", "mode") {
        cfg.proxy.proxy_mode = ProxyMode::parse(&v);
    } else if let Some(v) = t_bool(&doc, "proxy", "enabled") {
        cfg.proxy.proxy_mode = if v {
            ProxyMode::Manual
        } else {
            ProxyMode::None
        };
    }
    if let Some(v) = t_str(&doc, "proxy", "host") {
        cfg.proxy.proxy_host = v;
    }
    if let Some(v) = t_int(&doc, "proxy", "port") {
        cfg.proxy.proxy_port = sat_u16(v);
    }
```

(c) 357-359 行的 proxy 写入块替换为：

```rust
    set_str(&mut doc, "proxy", "mode", cfg.proxy.proxy_mode.as_str());
    // 旧键删掉，避免 config.toml 里同时留着 enabled / mode 两份真值。
    unset(&mut doc, "proxy", "enabled");
    set_str(&mut doc, "proxy", "host", &cfg.proxy.proxy_host);
    set_int(&mut doc, "proxy", "port", cfg.proxy.proxy_port as i64);
```

- [ ] **Step 4: `src/config/mod.rs` —— 导出 `ProxyMode`**

17-20 行的 re-export 里加 `ProxyMode`（按字母序插在 `Language` 后）：

```rust
pub use types::{
    AppConfig, ConfigError, CookieCfg, CrawlCfg, DownloadCfg, ExportFormat, GlobalCfg, LangType,
    Language, ProxyCfg, ProxyMode, SourceCfg, ThemeDynMode, ThemeKind, ThemePref,
};
```

- [ ] **Step 5: `src/config/defaults.rs` —— 模板换键**

79-82 行的模板片段替换为：

```toml
[proxy]
# 代理模式：none = 直连；manual = 用下面的 host / port；system = 读系统代理
# （Windows 读注册表 WinINET，即 Clash / v2ray 的「系统代理」开关；其它平台读 HTTPS_PROXY 环境变量）
mode = "none"
host = "127.0.0.1"
port = 7890
```

> ⚠️ 这段在 `r#"... "#` 原始字符串字面量里（`defaults.rs:44-83`）。新增内容不含 `"#` 序列，不会提前终止字面量。改完跑 `cargo test --lib config` 确认模板仍能 parse（`default_template_doc` 里有 `panic!` 断言）。

- [ ] **Step 6: `src/config/tests.rs` —— 改为三态 + 新增迁移测试**

(a) 13-16 行 import 加 `ProxyMode`：

```rust
use crate::config::{
    AppConfig, CookieCfg, CrawlCfg, DownloadCfg, ExportFormat, GlobalCfg, LangType, Language,
    ProxyCfg, ProxyMode, SourceCfg, ThemeDynMode, ThemeKind, ThemePref, load_config, save_config,
};
```

(b) 78-82 行的 `proxy` 字段改为：

```rust
        proxy: ProxyCfg {
            proxy_mode: ProxyMode::Manual,
            proxy_host: "10.0.0.1".to_string(),
            proxy_port: 1080,
        },
```

(c) 第 101 行断言改为：

```rust
    assert_eq!(loaded.proxy.proxy_mode, cfg.proxy.proxy_mode);
```

(d) 文件**末尾**追加 3 个新测试：

```rust
#[test]
fn legacy_proxy_enabled_key_migrates_to_mode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    std::fs::write(
        &path,
        "[proxy]\nenabled = true\nhost = \"1.2.3.4\"\nport = 8080\n",
    )
    .unwrap();
    let cfg = load_config(&path).unwrap();
    assert_eq!(cfg.proxy.proxy_mode, ProxyMode::Manual);
    assert_eq!(cfg.proxy.proxy_host, "1.2.3.4");
    assert_eq!(cfg.proxy.proxy_port, 8080);

    std::fs::write(&path, "[proxy]\nenabled = false\n").unwrap();
    assert_eq!(load_config(&path).unwrap().proxy.proxy_mode, ProxyMode::None);
}

#[test]
fn proxy_mode_key_wins_over_legacy_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[proxy]\nmode = \"system\"\nenabled = false\n").unwrap();
    assert_eq!(
        load_config(&path).unwrap().proxy.proxy_mode,
        ProxyMode::System
    );
}

#[test]
fn save_rewrites_legacy_proxy_enabled_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[proxy]\nenabled = true\nhost = \"127.0.0.1\"\nport = 7890\n",
    )
    .unwrap();

    // 默认 config 是 ProxyMode::None → 写回后旧键必须消失，新键为 "none"。
    save_config(&path, &AppConfig::default()).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("enabled ="), "旧键 enabled 应被 unset：\n{text}");
    assert!(text.contains("mode = \"none\""), "新键 mode 应写入：\n{text}");
}

#[test]
fn proxy_mode_parse_is_lenient() {
    assert_eq!(ProxyMode::parse(" system "), ProxyMode::System);
    assert_eq!(ProxyMode::parse("MANUAL"), ProxyMode::Manual);
    assert_eq!(ProxyMode::parse("none"), ProxyMode::None);
    // 认不出来的一律直连，不 panic
    assert_eq!(ProxyMode::parse(""), ProxyMode::None);
    assert_eq!(ProxyMode::parse("garbage"), ProxyMode::None);
}
```

- [ ] **Step 7: `Cargo.toml` —— 加 Windows 专属依赖**

在 108-110 行的 `[target.'cfg(target_os = "windows")'.build-dependencies]` **之前**插入：

```toml
# Windows 系统代理探测：读 WinINET 注册表
# （HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings）。
# 版本与 Cargo.lock 里已有的 winreg 一致，不引入新下载。
[target.'cfg(target_os = "windows")'.dependencies]
winreg = "0.55"
```

- [ ] **Step 8: 新建 `src/http/system_proxy.rs`**

整文件内容：

```rust
//! 系统代理探测。
//!
//! reqwest **没有**开 `system-proxy` feature（见 `Cargo.toml`），所以"用系统代理"必须
//! 自己读：Windows 读 WinINET 注册表，其它平台读环境变量。
//!
//! 两个对外入口：
//! - [`detect`] —— 探测一次，结果给 UI 展示；
//! - [`resolve_proxy_url`] —— 把 [`ProxyMode`] 压成 `Option<String>`，`None` = 直连。
//!   这是 client 层唯一认的接口，三态语义只在这里展开一次。

use crate::config::{ProxyCfg, ProxyMode};

/// 系统代理探测结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemProxy {
    /// 探测到可用代理，值是可直接喂给 `reqwest::Proxy::all` 的 URL。
    Found(String),
    /// 没有可用代理。`reason` 用于设置页说明"为什么没生效"。
    Absent { reason: AbsentReason },
}

/// [`SystemProxy::Absent`] 的原因。平台各自只会产生其中一个子集
/// （Windows 用不到 `EnvUnset`，其它平台用不到 `PacOnly` / `SocksOnly`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsentReason {
    /// 系统代理开关是关的（Windows `ProxyEnable = 0`）。
    NotEnabled,
    /// 只配了 PAC（`AutoConfigURL`）—— reqwest 不解析 PAC，做不到透明转发。
    PacOnly,
    /// 只配了 SOCKS 代理。当前只支持 HTTP 代理。
    SocksOnly,
    /// 非 Windows：相关环境变量都没设。
    EnvUnset,
    /// 读系统设置失败（注册表打不开等）。
    ReadFailed,
}

/// 探测当前系统代理。
#[cfg(target_os = "windows")]
pub fn detect() -> SystemProxy {
    detect_windows()
}

/// 探测当前系统代理。
#[cfg(not(target_os = "windows"))]
pub fn detect() -> SystemProxy {
    detect_env()
}

/// 把配置解析成最终要用的代理 URL。`None` = 直连（调用方不碰 `ClientBuilder::proxy`）。
///
/// - `None` 模式 → 直连；
/// - `Manual` → `http://<host>:<port>`（host 为空视为直连）；
/// - `System` → 探测结果，探测不到同样退化为直连（不阻断 client 构造）。
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

/// 手动模式：host 为空 → 直连。避免拼出 `http://:7890` 这种畸形 URL 让 client 构造失败。
fn manual_url(p: &ProxyCfg) -> Option<String> {
    let host = p.proxy_host.trim();
    if host.is_empty() {
        return None;
    }
    Some(format!("http://{host}:{}", p.proxy_port))
}

/// Windows：读 WinINET 系统代理。
///
/// Clash / v2ray 的「系统代理」开关写的就是
/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings` 下的
/// `ProxyEnable` + `ProxyServer`。
#[cfg(target_os = "windows")]
fn detect_windows() -> SystemProxy {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    const INTERNET_SETTINGS: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

    let key = match RegKey::predef(HKEY_CURRENT_USER).open_subkey(INTERNET_SETTINGS) {
        Ok(k) => k,
        Err(e) => {
            tracing::warn!("读取系统代理注册表失败: {e}");
            return SystemProxy::Absent {
                reason: AbsentReason::ReadFailed,
            };
        }
    };

    // 值类型不是 DWORD / 键不存在都当 0（= 没开）。
    if key.get_value::<u32, _>("ProxyEnable").unwrap_or(0) == 0 {
        return SystemProxy::Absent {
            reason: AbsentReason::NotEnabled,
        };
    }

    match key.get_value::<String, _>("ProxyServer").ok().as_deref() {
        Some(raw) => match parse_proxy_server(raw) {
            Some(url) => SystemProxy::Found(url),
            // 有 ProxyServer 但里面只有 socks=... —— 当前不支持。
            None => SystemProxy::Absent {
                reason: AbsentReason::SocksOnly,
            },
        },
        // 开关开着却没有 ProxyServer：要么是 PAC 模式，要么是残留状态。
        None => {
            if key.get_value::<String, _>("AutoConfigURL").is_ok() {
                SystemProxy::Absent {
                    reason: AbsentReason::PacOnly,
                }
            } else {
                SystemProxy::Absent {
                    reason: AbsentReason::NotEnabled,
                }
            }
        }
    }
}

/// 解析 WinINET 的 `ProxyServer` 值。两种形态：
///
/// - `"127.0.0.1:7890"` —— 所有协议共用；
/// - `"http=127.0.0.1:7890;https=127.0.0.1:7891;ftp=..."` —— 按协议分列。
///
/// 只支持 HTTP 代理：优先 `http=` 项，其次第一个非 `socks*=` 项
/// （`https=` 的值也是 HTTP 代理 —— 走 CONNECT，所以仍拼 `http://`）；
/// 一个可用项都没有（例如只配了 `socks=127.0.0.1:1080`）→ `None`。
///
/// 非 Windows 构建下唯一调用点在 `detect_windows` 里（被 cfg 掉），只剩测试在用。
#[cfg_attr(
    not(target_os = "windows"),
    allow(dead_code, reason = "唯一生产调用点在 detect_windows 内，非 Windows 构建下只被单元测试使用")
)]
fn parse_proxy_server(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // 无 `=` → 裸 host:port，所有协议共用。
    if !raw.contains('=') {
        return Some(format!("http://{raw}"));
    }
    let mut fallback = None;
    for entry in raw.split(';') {
        let Some((scheme, addr)) = entry.split_once('=') else {
            continue;
        };
        let addr = addr.trim();
        if addr.is_empty() {
            continue;
        }
        if scheme.trim().eq_ignore_ascii_case("http") {
            return Some(format!("http://{addr}"));
        }
        let scheme = scheme.trim().to_ascii_lowercase();
        if !scheme.starts_with("socks") && fallback.is_none() {
            fallback = Some(format!("http://{addr}"));
        }
    }
    fallback
}

/// 非 Windows：读 `HTTPS_PROXY` → `https_proxy` → `HTTP_PROXY` → `http_proxy`，取第一个非空。
///
/// 值允许不带 scheme（`127.0.0.1:7890`），补 `http://` —— `reqwest::Proxy::all` 要求
/// 值是个能解析的 URL。
#[cfg(not(target_os = "windows"))]
fn detect_env() -> SystemProxy {
    for key in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"] {
        if let Ok(v) = std::env::var(key) {
            let v = v.trim();
            if v.is_empty() {
                continue;
            }
            return SystemProxy::Found(if v.contains("://") {
                v.to_string()
            } else {
                format!("http://{v}")
            });
        }
    }
    SystemProxy::Absent {
        reason: AbsentReason::EnvUnset,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    fn cfg(mode: ProxyMode, host: &str, port: u16) -> ProxyCfg {
        ProxyCfg {
            proxy_mode: mode,
            proxy_host: host.to_string(),
            proxy_port: port,
        }
    }

    #[test]
    fn parse_bare_host_port() {
        assert_eq!(
            parse_proxy_server("127.0.0.1:7890").as_deref(),
            Some("http://127.0.0.1:7890")
        );
    }

    #[test]
    fn parse_prefers_http_entry() {
        let raw = "https=127.0.0.1:7891;http=127.0.0.1:7890;ftp=127.0.0.1:7892";
        assert_eq!(
            parse_proxy_server(raw).as_deref(),
            Some("http://127.0.0.1:7890")
        );
    }

    #[test]
    fn parse_falls_back_to_first_non_socks_entry() {
        assert_eq!(
            parse_proxy_server("socks=127.0.0.1:1080;https=127.0.0.1:7891").as_deref(),
            Some("http://127.0.0.1:7891")
        );
    }

    #[test]
    fn parse_rejects_socks_only_and_blank() {
        assert_eq!(parse_proxy_server("socks=127.0.0.1:1080"), None);
        assert_eq!(parse_proxy_server("socks5=127.0.0.1:1080"), None);
        assert_eq!(parse_proxy_server("   "), None);
    }

    #[test]
    fn resolve_none_mode_is_direct() {
        assert_eq!(resolve_proxy_url(&cfg(ProxyMode::None, "127.0.0.1", 7890)), None);
    }

    #[test]
    fn resolve_manual_mode_builds_http_url() {
        assert_eq!(
            resolve_proxy_url(&cfg(ProxyMode::Manual, " 10.0.0.1 ", 1080)),
            Some("http://10.0.0.1:1080".to_string())
        );
    }

    #[test]
    fn resolve_manual_mode_with_blank_host_is_direct() {
        assert_eq!(resolve_proxy_url(&cfg(ProxyMode::Manual, "  ", 1080)), None);
    }
}
```

- [ ] **Step 9: `src/http/mod.rs` —— 挂模块 + re-export**

(a) 第 5-12 行的 `pub mod` 列表里按字母序插入 `pub mod system_proxy;`（在 `pub mod fetch;` 之后）：

```rust
pub mod cf;
pub mod client;
pub mod clients;
pub mod encoding;
pub mod fetch;
pub mod system_proxy;
pub mod ua;
pub mod url_join;
pub mod util;
```

(b) 在 `pub use clients::HttpClients;` 之后插入：

```rust
// `detect` 在本模块内重命名为 `detect_system_proxy`：`crate::http::detect()` 看不出在探什么。
pub use system_proxy::{AbsentReason, SystemProxy, detect as detect_system_proxy, resolve_proxy_url};
```

- [ ] **Step 10: `src/http/client.rs` —— 走 `resolve_proxy_url`**

(a) 11-13 行 import 区替换为：

```rust
use crate::config::AppConfig;
#[cfg(test)]
use crate::config::{ProxyCfg, ProxyMode};
use crate::http::system_proxy::resolve_proxy_url;
```

(b) 43-48 行的 proxy 块替换为：

```rust
    // 三态模式在 `resolve_proxy_url` 里展开：`None` = 直连，所以这里不需要 match。
    // 手动模式 host 为空、或系统模式探测不到代理，都返回 `None`（静默直连，不阻断 client 构造）。
    if let Some(proxy_url) = resolve_proxy_url(&cfg.proxy) {
        let proxy = reqwest::Proxy::all(&proxy_url)
            .with_context(|| format!("invalid proxy URL: {proxy_url}"))?;
        builder = builder.proxy(proxy);
    }
```

(c) 测试 70-82 行替换为：

```rust
    #[test]
    fn build_async_with_manual_proxy_still_constructs() {
        // reqwest 的 Proxy::all 只做 URL 解析；不真正连。
        let cfg = AppConfig {
            proxy: ProxyCfg {
                proxy_mode: ProxyMode::Manual,
                proxy_host: "127.0.0.1".to_string(),
                proxy_port: 1,
            },
            ..AppConfig::default()
        };
        let _client = build_async_client(&cfg, &ClientOptions::default()).unwrap();
    }

    #[test]
    fn build_async_with_manual_proxy_and_blank_host_constructs() {
        // host 为空 → resolve_proxy_url 返回 None → 直连，不构造 Proxy。
        let cfg = AppConfig {
            proxy: ProxyCfg {
                proxy_mode: ProxyMode::Manual,
                proxy_host: String::new(),
                proxy_port: 7890,
            },
            ..AppConfig::default()
        };
        let _client = build_async_client(&cfg, &ClientOptions::default()).unwrap();
    }
```

(d) 92-95 行的 doc 注释与测试名改一下（`proxy_enabled=true` → `Manual` 模式），121-127 行的 cfg 改为：

```rust
        let cfg = AppConfig {
            proxy: ProxyCfg {
                proxy_mode: ProxyMode::Manual,
                proxy_host: "127.0.0.1".into(),
                proxy_port: proxy_port as u16,
            },
            ..AppConfig::default()
        };
```

测试函数名改为 `manual_proxy_actually_routes_traffic_through_proxy`，注释里「这才能证明 `proxy_enabled=true` 不是只"URL 解析没报错"」改为「这才能证明 `Manual` 模式不是只"URL 解析没报错"」。

- [ ] **Step 11: `src/http/clients.rs` —— 签名改存解析后 URL**

(a) 22-38 行替换为：

```rust
/// 当前生效的 proxy 快照。`rebuild_proxy` 用它判断"配置是否真的变了"。
///
/// 存**解析后的 URL** 而不是原始字段：`System` 模式下 host / port 字段根本没动，
/// 但注册表里的值可能刚被 Clash 改过 —— 只比对字段会漏掉这种真实变化。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxySignature {
    resolved: Option<String>,
}

impl ProxySignature {
    fn from_cfg(cfg: &AppConfig) -> Self {
        Self {
            resolved: crate::http::system_proxy::resolve_proxy_url(&cfg.proxy),
        }
    }
}
```

(b) `HttpClients::empty()` 里 62-66 行的字面量替换为：

```rust
            proxy_signature: Mutex::new(ProxySignature { resolved: None }),
```

(c) 16-17 行的 `#[cfg(test)] use crate::config::{GlobalCfg, ProxyCfg};` 加上 `ProxyMode`：

```rust
#[cfg(test)]
use crate::config::{GlobalCfg, ProxyCfg, ProxyMode};
```

(d) 测试里的 3 处 `ProxyCfg { proxy_enabled: true, ... }`（235-239、253-257 行）改为：

```rust
            proxy: ProxyCfg {
                proxy_mode: ProxyMode::Manual,
                proxy_host: "127.0.0.1".into(),
                proxy_port: 9999,   // 第二处是 8080
            },
```

(e) 在测试模块末尾追加一个新测试，锁住「系统模式探不到时不重建」的语义：

```rust
    #[test]
    fn rebuild_proxy_from_mode_none_to_blank_host_manual_is_noop() {
        // None 模式与「Manual + 空 host」解析结果都是 None —— signature 相等，
        // 不该白重建 client（这也是 `empty()` 之后第一次 rebuild 不再重建的原因）。
        let clients = HttpClients::new(&default_cfg()).unwrap();
        let before = clients.safe_client_ptr().unwrap();

        let cfg = AppConfig {
            proxy: ProxyCfg {
                proxy_mode: ProxyMode::Manual,
                proxy_host: String::new(),
                proxy_port: 7890,
            },
            ..default_cfg()
        };
        clients.rebuild_proxy(&cfg).unwrap();
        assert_eq!(before, clients.safe_client_ptr().unwrap());
    }
```

- [ ] **Step 12: `src/core/bootstrap.rs` —— 兜底改新字段**

(a) import 区加 `use crate::config::ProxyMode;`（若已有 `use crate::config::...` 行则并入）。
(b) 第 80 行替换为：

```rust
            cfg_no_proxy.proxy.proxy_mode = ProxyMode::None;
```

- [ ] **Step 13: `src/desktop/pages/settings/page_proxy.rs` —— 临时改法保持编译**

> ⚠️ **这一步是过渡态，Task 3 会把整个文件重写掉。** 目的是让 Task 1 结束时整棵树编译通过、测试全绿，而不是留一个半改的 UI。

(a) 在 `use rust_i18n::t;`（第 11 行）之后插入：

```rust
use crate::config::ProxyMode;
```

第 14 行的 `use super::fields::{bool_field, number_field_u16, string_field};` **保持不动** —— 本步骤仍然用 `bool_field`，Task 3 才换成 `dropdown_field`。

(b) 27-35 行的 switch 改成「开关映射到 Manual / None」：

```rust
                    SettingItem::new(
                        t!("Settings.item.proxy_enabled"),
                        bool_field(
                            &m,
                            move |model| model.config.proxy.proxy_mode == ProxyMode::Manual,
                            move |model, val| {
                                model.config.proxy.proxy_mode = if val {
                                    ProxyMode::Manual
                                } else {
                                    ProxyMode::None
                                };
                            },
                        ),
                    )
                    .description(t!("Settings.desc.proxy_enabled").to_string()),
```

- [ ] **Step 14: 跑质量门**

Run:
```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --lib 2>&1 | tail -3
```
Expected: clippy 0 警告；`test result: ok. 451 passed; 0 failed; ... 4 ignored`。

> 451 的来源（基线 438 + 13）：
> - `src/config/tests.rs` **+4**（`legacy_proxy_enabled_key_migrates_to_mode` / `proxy_mode_key_wins_over_legacy_enabled` / `save_rewrites_legacy_proxy_enabled_key` / `proxy_mode_parse_is_lenient`）
> - `src/http/client.rs` **+1**（原来的 1 个测试拆成 2 个：`build_async_with_manual_proxy_still_constructs` + `build_async_with_manual_proxy_and_blank_host_constructs`）
> - `src/http/clients.rs` **+1**（`rebuild_proxy_from_mode_none_to_blank_host_manual_is_noop`）
> - `src/http/system_proxy.rs` **+7**
>
> 数字对不上就回头查哪组测试没被 `cargo test --lib` 收录 —— 尤其确认 `src/http/mod.rs` 里 `pub mod system_proxy;` 已加（模块没挂上时那 7 个测试一个都不会跑，数字仍是 444）。

- [ ] **Step 15: 停下等确认**

把改动清单（改 11 个文件 + 新建 1 个文件）与质量门输出报给用户，等待确认后再进 Task 2。**不要 `git commit`。**

---

## Task 2: `AppModel` 持系统代理快照 + 三个刷新触发点

**Files:**
- Modify: `src/desktop/model/mod.rs:42-52`、`57-136`
- Modify: `src/desktop/root.rs:78-83`
- Modify: `src/desktop/model/persistence.rs:32-34`（只改注释）

**Interfaces:**
- Consumes: `crate::http::{detect_system_proxy, SystemProxy}`、`crate::http::HttpClients::rebuild_proxy`（Task 1 已就位）。
- Produces:
  - `AppModel.system_proxy: SystemProxy`（pub 字段，Task 3 读它渲染状态行）
  - `AppModel::refresh_system_proxy(&mut self)`（无返回值；内部失败推 error notification）

- [ ] **Step 1: `src/desktop/model/mod.rs` —— 字段 + 启动快照 + 刷新方法**

(a) 第 51 行 import 改为：

```rust
use crate::http::{HttpClients, SystemProxy, detect_system_proxy};
```

(b) 在 `pub struct AppModel` 里、`http` 字段（68-70 行）之后插入：

```rust
    /// 最近一次系统代理探测结果（设置页展示用）。
    ///
    /// 启动时探一次，之后由 [`Self::refresh_system_proxy`] 在「改代理模式」和「进入设置页」
    /// 两处刷新 —— 不做后台轮询，用户在 Clash 里改完要下次触发才生效。
    pub system_proxy: SystemProxy,
```

(c) 在 `new_with_wakeup` 里，`let http = ctx.http;`（107 行）之后插入：

```rust
        // 启动快照。**不需要** rebuild：`HttpClients::new` 构造时已经按
        // `resolve_proxy_url` 把系统代理算进去了，这里只是给 UI 留一份可读结果。
        let system_proxy = detect_system_proxy();
```

(d) 结构体字面量（116-134 行）里 `http,` 之后插入 `system_proxy,`。

(e) 在 `fn push_event` 之前插入新方法：

```rust
    /// 重新探测系统代理，并让共享 HTTP client 跟上新结果。
    ///
    /// 触发点三处：启动（`new_with_wakeup` 快照，不重建）、改代理模式（`page_proxy`）、
    /// 进入设置页（`RootView::navigate`）。
    pub fn refresh_system_proxy(&mut self) {
        self.system_proxy = detect_system_proxy();
        // 结果变了的话，已构造的 client 里还钉着旧 proxy，必须整体换掉。
        // `rebuild_proxy` 按解析后的 URL 比对，没变即 no-op，所以多调不亏。
        if let Err(e) = self.http.rebuild_proxy(&self.config) {
            let msg = format!("HTTP client 重建失败: {e}");
            tracing::warn!("{msg}");
            self.push_error(msg);
        }
    }
```

> `push_error` 是 `AppModel` 的同文件私有方法（`persistence.rs:27` 在用），可直接调。

- [ ] **Step 2: `src/desktop/root.rs` —— 进设置页时重探**

78-83 行的 `navigate` 替换为：

```rust
    fn navigate(&mut self, page: NavPage, cx: &mut Context<Self>) {
        if self.current_page != page {
            self.current_page = page;
            // 进设置页时重读系统代理：用户可能刚在 Clash 里切过开关，打开设置页
            // 就是为了看到最新探测结果（也顺手让 client 跟上）。
            if page == NavPage::Settings {
                self.model.update(cx, |m, _| m.refresh_system_proxy());
            }
            cx.notify();
        }
    }
```

- [ ] **Step 3: `src/desktop/model/persistence.rs` —— 修正过时的注释**

32-34 行的注释替换为：

```rust
        // proxy / unsafe_ssl 改了 → 重建共享 HTTP client。`rebuild_proxy` 按
        // **解析后的代理 URL** 比对（见 `HttpClients::rebuild_proxy`），未变即 no-op。
        // 重建失败时 config 已写盘但客户端还是旧配置, 推 error 让用户知道。
```

- [ ] **Step 4: 跑质量门**

Run:
```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --lib 2>&1 | tail -3
```
Expected: clippy 0 警告；`451 passed`（与 Task 1 持平 —— 本 Task 只加状态与调用点，没有新测试；`refresh_system_proxy` 依赖真实注册表，不写不确定测试）。

- [ ] **Step 5: 停下等确认**

**不要 `git commit`。**

---

## Task 3: 代理页 UI（三态下拉 / 置灰 / 探测结果行）+ i18n

**Files:**
- Modify: `locales/app.yml:173-176`、`274-285`、`391-402`、`443-473`
- Rewrite: `src/desktop/pages/settings/page_proxy.rs`

**Interfaces:**
- Consumes: `AppModel.system_proxy`（Task 2）、`AppModel::refresh_system_proxy`（Task 2）、`crate::config::ProxyMode::{as_str, parse}`（Task 1）、`crate::http::{AbsentReason, SystemProxy}`（Task 1）、`super::fields::dropdown_field`（既有）。
- Produces: 无下游依赖（本 Task 是链路终点）。

- [ ] **Step 1: `locales/app.yml` —— 加 / 改词条**

(a) 173-176 行的 `http_proxy` group 改名换文案（键名一并改，页面引用同步改）：

```yaml
    proxy:
      en: Proxy
      zh-CN: 代理
      zh-TW: 代理
```

(b) 274-277 行的 `proxy_enabled` item **删除**，替换为 `proxy_mode`：

```yaml
    proxy_mode:
      en: "Proxy mode"
      zh-CN: 代理模式
      zh-TW: 代理模式
    system_proxy_status:
      en: "System proxy"
      zh-CN: 系统代理
      zh-TW: 系統代理
```

(c) 391-394 行的 `proxy_enabled` desc **删除**，替换为 `proxy_mode`；`proxy_host` / `proxy_port` 的 desc 补一句"仅手动模式生效"：

```yaml
    proxy_mode:
      en: "Direct, a manually configured HTTP proxy, or the OS proxy settings."
      zh-CN: 直连、手动配置的 HTTP 代理，或读取操作系统代理设置。
      zh-TW: 直連、手動設定的 HTTP 代理，或讀取作業系統代理設定。
    system_proxy_status:
      en: "Read from the Windows registry (WinINET), or from HTTPS_PROXY on other platforms."
      zh-CN: Windows 下读注册表 WinINET；其它平台读 HTTPS_PROXY 环境变量。
      zh-TW: Windows 下讀登錄檔 WinINET；其它平台讀 HTTPS_PROXY 環境變數。
    proxy_host:
      en: "Proxy server address (IP or domain). Used in Manual mode only."
      zh-CN: 代理服务器地址（IP 或域名）。仅在「手动配置」模式下生效。
      zh-TW: 代理伺服器地址（IP 或域名）。僅在「手動設定」模式下生效。
    proxy_port:
      en: "Proxy server port (1-65535). Used in Manual mode only."
      zh-CN: 代理服务器端口（1-65535）。仅在「手动配置」模式下生效。
      zh-TW: 代理伺服器埠（1-65535）。僅在「手動設定」模式下生效。
```

(d) `Settings.option` 段（443 行起）的 `theme_dyn_mode` 之后追加：

```yaml
    proxy_mode:
      none:
        en: Disabled
        zh-CN: 不使用
        zh-TW: 不使用
      manual:
        en: "Manual"
        zh-CN: 手动配置
        zh-TW: 手動設定
      system:
        en: "Use system proxy"
        zh-CN: 使用系统代理
        zh-TW: 使用系統代理
```

(e) 在 `Settings:` 段内、`language_restart_dialog`（476 行）之前追加探测结果的文案段：

```yaml
  proxy_status:
    detected:
      en: "Detected: %{url}"
      zh-CN: "已检测到：%{url}"
      zh-TW: "已偵測到：%{url}"
    not_detected:
      en: "Not detected"
      zh-CN: "未检测到系统代理"
      zh-TW: "未偵測到系統代理"
    reason:
      not_enabled:
        en: "the system proxy switch is off"
        zh-CN: 系统代理开关是关闭的
        zh-TW: 系統代理開關是關閉的
      pac_only:
        en: "only a PAC script is configured, which this app cannot use"
        zh-CN: 只配置了 PAC 脚本，本应用无法使用
        zh-TW: 只設定了 PAC 指令碼，本應用無法使用
      socks_only:
        en: "only a SOCKS proxy is configured, which this app does not support"
        zh-CN: 只配置了 SOCKS 代理，本应用不支持
        zh-TW: 只設定了 SOCKS 代理，本應用不支援
      env_unset:
        en: "no HTTPS_PROXY / HTTP_PROXY environment variable is set"
        zh-CN: 未设置 HTTPS_PROXY / HTTP_PROXY 环境变量
        zh-TW: 未設定 HTTPS_PROXY / HTTP_PROXY 環境變數
      read_failed:
        en: "system proxy settings could not be read"
        zh-CN: 读取系统代理设置失败
        zh-TW: 讀取系統代理設定失敗
```

> ⚠️ 缩进是硬要求：`Settings:` 下是 2 空格，`item:` / `desc:` / `option:` / `proxy_status:` 是 2 空格顶格同级，键名 4 空格，文案 6 空格。改完跑 `cargo test --lib i18n` 确认 `i18n::tests` 仍通过（该模块会枚举 key 做一致性检查）。

- [ ] **Step 2: 重写 `src/desktop/pages/settings/page_proxy.rs`**

整文件替换为：

```rust
//! 代理页（`Settings` 左侧 sidebar 第 3 项）。
//!
//! 2 个 group：代理（模式下拉 / Host / Port / 系统代理探测结果）与起点 Cookie（多行 textarea）。

use gpui_kit::component::{
    ActiveTheme as _, AxisExt, Sizable as _,
    input::Textarea,
    setting::{NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage},
};
use gpui_kit::{App, Entity, ParentElement, SharedString, Styled, div, px};
use rust_i18n::t;

use crate::config::ProxyMode;
use crate::desktop::model::AppModel;
use crate::http::{AbsentReason, SystemProxy};

use super::ctx::PageCtx;
use super::fields::{dropdown_field, number_field_u16, string_field};

pub(super) fn build(ctx: &PageCtx<'_>, cx: &App) -> SettingPage {
    let m = ctx.model.clone();
    // `build_pages` 每帧被 `SettingsPage::render` 调一次，所以这里的 `disabled` 状态
    // 每帧重算 —— 改完下拉立刻生效（配合 `after_proxy_mode` 的 refresh_windows）。
    let mode = m.read(cx).config.proxy.proxy_mode;
    let system_proxy = m.read(cx).system_proxy.clone();

    let mode_options: Vec<(SharedString, SharedString)> = vec![
        (
            ProxyMode::None.as_str().into(),
            t!("Settings.option.proxy_mode.none").into(),
        ),
        (
            ProxyMode::Manual.as_str().into(),
            t!("Settings.option.proxy_mode.manual").into(),
        ),
        (
            ProxyMode::System.as_str().into(),
            t!("Settings.option.proxy_mode.system").into(),
        ),
    ];

    let mut items = vec![
        SettingItem::new(
            t!("Settings.item.proxy_mode"),
            dropdown_field(
                mode_options,
                &m,
                |model| SharedString::from(model.config.proxy.proxy_mode.as_str()),
                |model, val| model.config.proxy.proxy_mode = ProxyMode::parse(&val),
                Some(after_proxy_mode),
            ),
        )
        .description(t!("Settings.desc.proxy_mode").to_string()),
        SettingItem::new(
            t!("Settings.item.proxy_host"),
            string_field(
                &m,
                move |model| SharedString::from(model.config.proxy.proxy_host.clone()),
                move |model, s| model.config.proxy.proxy_host = s,
            ),
        )
        .description(t!("Settings.desc.proxy_host").to_string())
        .disabled(mode != ProxyMode::Manual),
        SettingItem::new(
            t!("Settings.item.proxy_port"),
            number_field_u16(
                &m,
                NumberFieldOptions {
                    min: 1.0,
                    max: 65_535.0,
                    ..Default::default()
                },
                move |model| model.config.proxy.proxy_port,
                move |model, v| model.config.proxy.proxy_port = v,
            ),
        )
        .description(t!("Settings.desc.proxy_port").to_string())
        .disabled(mode != ProxyMode::Manual),
    ];

    // 只有选了「使用系统代理」才显示探测结果：它是这个模式唯一的反馈面 ——
    // 探测不到时代理静默退化为直连，不弹对话框、不阻断下载。
    if mode == ProxyMode::System {
        items.push(
            SettingItem::new(
                t!("Settings.item.system_proxy_status"),
                SettingField::render(move |_opts, _window, cx| {
                    let (text, color) = match &system_proxy {
                        SystemProxy::Found(url) => (
                            t!("Settings.proxy_status.detected", url = url.as_str()).to_string(),
                            cx.theme().success,
                        ),
                        SystemProxy::Absent { reason } => (
                            format!(
                                "{}（{}）",
                                t!("Settings.proxy_status.not_detected"),
                                reason_text(*reason)
                            ),
                            cx.theme().muted_foreground,
                        ),
                    };
                    div().text_sm().text_color(color).child(text)
                }),
            )
            .description(t!("Settings.desc.system_proxy_status").to_string()),
        );
    }

    SettingPage::new(t!("Settings.page.proxy"))
        .resettable(false)
        .default_open(true)
        .groups(vec![
            // ============ 代理 ============
            SettingGroup::new()
                .title(t!("Settings.group.proxy"))
                .items(items),
            // ============ Cookie ============
            // 起点 cookie 必须是**多行 textarea**（`Cookie:` 头是一整段多对 `k=v`），
            // 所以走 `SettingField::render` 挂 owner-cached 的 TextareaState。
            SettingGroup::new()
                .title(t!("Settings.group.cookie"))
                .items(vec![
                    SettingItem::new(
                        t!("Settings.item.qidian_cookie"),
                        SettingField::render({
                            let qidian_cookie_input = ctx.qidian_cookie_input.clone();
                            move |options, _window, _cx| {
                                let mut el = Textarea::new(&qidian_cookie_input)
                                    // 传 `options.size()` 让字号 / 内边距跟同页其它设置项对齐；
                                    // 高度仍由 `.h(px(80.))` 固定。
                                    .with_size(options.size())
                                    .h(px(80.));
                                // horizontal layout → 固定 256px；其它 → 占满整行
                                // （与 dl 设置项一致，见 page_general.rs）。
                                if options.layout().is_horizontal() {
                                    el = el.w_64();
                                } else {
                                    el = el.w_full();
                                }
                                el
                            }
                        }),
                    )
                    .description(t!("Settings.desc.qidian_cookie").to_string()),
                ]),
        ])
}

/// 代理模式 setter 的副作用。
///
/// `dropdown_field` 内部已经调过 `persist_settings()`（→ `rebuild_proxy`），这里只需要
/// 补两件它做不到的事：刷新 `AppModel.system_proxy` 快照，和强制整页重绘 ——
/// Host / Port 的 `disabled` 与探测结果行都在 `build()` 里算，只有 `SettingsPage::render`
/// 重跑才会更新（`dropdown_field` 只会让 Select 自己重绘，不会带起父级）。
///
/// 用模块内 `fn`（不是闭包）让它能当 `after_set` 的 fn pointer，同
/// `page_general::after_theme_kind`。
fn after_proxy_mode(m: &Entity<AppModel>, cx: &mut App) {
    m.update(cx, |model, _| model.refresh_system_proxy());
    cx.refresh_windows();
}

/// [`AbsentReason`] → 用户可读的一小句「为什么没生效」。
fn reason_text(reason: AbsentReason) -> String {
    match reason {
        AbsentReason::NotEnabled => t!("Settings.proxy_status.reason.not_enabled").to_string(),
        AbsentReason::PacOnly => t!("Settings.proxy_status.reason.pac_only").to_string(),
        AbsentReason::SocksOnly => t!("Settings.proxy_status.reason.socks_only").to_string(),
        AbsentReason::EnvUnset => t!("Settings.proxy_status.reason.env_unset").to_string(),
        AbsentReason::ReadFailed => t!("Settings.proxy_status.reason.read_failed").to_string(),
    }
}
```

> 上面这个 import 块是**逐项核对过**的，别凭感觉删：
> - `ActiveTheme as _` —— `cx.theme()` 需要（`page_general.rs:7` / `page_about.rs:7` 同款）。
> - `AxisExt` —— `.w_64()` / `.w_full()`。
> - `Sizable as _` —— `Textarea::with_size`。
> - `SharedString` —— 选项元组与 getter / setter 参数类型。
> - `Styled` / `ParentElement` / `div` —— 状态行的 `div().text_sm().text_color(..).child(..)`。
> - `Entity` —— `after_proxy_mode` 的签名 `fn(&Entity<AppModel>, &mut App)`。
> - `NumberFieldOptions` / `SettingField` / `SettingGroup` / `SettingItem` / `SettingPage` / `Textarea` —— 各字段构造。
>
> `ProxyMode::parse(&val)`（`val: SharedString`）依赖 `Deref<Target = str>` 自动解引用 ——
> `page_general.rs:95` 的 `ThemeKind::parse(&val)` 就是这个写法，已确认可行。

- [ ] **Step 3: 跑质量门**

Run:
```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --lib 2>&1 | tail -3
```
Expected: clippy 0 警告；`451 passed`（本 Task 纯 UI + 文案，无新单元测试）。

- [ ] **Step 4: 手动验证（需要人工执行，GUI 无法自动化）**

Run:
```bash
cargo run --bin so-novel-rs
```
逐项确认：
1. 侧栏进「设置」→「代理」：第一项是「代理模式」下拉，选项为 不使用 / 手动配置 / 使用系统代理。
2. 默认（不使用）：Host / Port 两行**半透明、点不动**。
3. 切到「手动配置」：Host / Port 立即可编辑（不需要切页离开再回来）；改成 `127.0.0.1:7890`（或你本地实际代理端口）后能正常搜索 / 下载。
4. 切到「使用系统代理」：多出一行「系统代理」——
   - Clash 开着「系统代理」时显示绿色的 `已检测到：http://127.0.0.1:7890`（端口以你实际配置为准）；
   - 在 Clash 里关掉系统代理，再切到别的页又切回「设置」：该行变成灰色的「未检测到系统代理（系统代理开关是关闭的）」。
5. 打开 `config.toml`：`[proxy]` 下是 `mode = "..."`，**没有** `enabled = ...`。
6. 把旧配置（含 `enabled = true`）手工写回去 → 重启 → 设置页应显示「手动配置」（自动迁移生效）。

- [ ] **Step 5: 停下等确认**

把截图或逐项结果报给用户，**不要 `git commit`。**

---

## Task 4: CHANGELOG

**Files:**
- Modify: `docs/CHANGELOG.md:3-21`（`## [Unreleased]` 段）

**Interfaces:**
- Consumes: 前 3 个 Task 的全部行为。
- Produces: 无（文档终点）。

- [ ] **Step 1: 在 `## [Unreleased]` 的 `### Changed` 段首插入一条**

```markdown
- **代理设置改为三态「代理模式」**：`[proxy].enabled`（bool）→ `[proxy].mode`
  （`"none"` / `"manual"` / `"system"`）。新增「使用系统代理」——Windows 读 WinINET
  注册表（Clash / v2ray 的「系统代理」开关写的就是它），其它平台读 `HTTPS_PROXY` /
  `HTTP_PROXY`。探测不到时静默退化为直连，并在设置页显示原因（开关关闭 / 只有 PAC /
  只有 SOCKS / 环境变量未设 / 读取失败）。旧键在首次保存配置时自动迁移并删除，
  手动改过 `config.toml` 的用户无需任何操作。
```

- [ ] **Step 2: 确认没有残留旧键文档**

Run:
```bash
grep -rn "proxy.*enabled\|enable.*http.*proxy" docs/ README.md AGENTS.md tasks/ 2>/dev/null
```
Expected: 只命中本次新增的 CHANGELOG 条目（含 "→ `[proxy].mode`" 的那条）。若命中其它文档描述旧的 `enabled` 键，顺手改成 `mode`。

- [ ] **Step 3: 跑质量门 + 收尾**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --lib 2>&1 | tail -3
```
Expected: 全绿，`451 passed / 4 ignored`。

- [ ] **Step 4: 全部完成后停下**

汇总 4 个 Task 的改动（含 `docs/superpowers/specs/2026-10-07-system-proxy-mode-design.md` 与本计划文件）交给用户，由用户决定是否提交。**不要 `git commit`。**

---

## 收尾自检（全部 Task 完成后执行）

- [ ] **Spec 覆盖**：设计文档 §一（schema + 迁移）→ Task 1；§二（探测模块）→ Task 1 Step 8；§三（client 集成 + rebuild 签名）→ Task 1 Step 10-11；§四（刷新时机）→ Task 2；§五（UI）→ Task 3；§六（i18n）→ Task 3 Step 1；§七（测试）→ Task 1 各 Step 的测试 + Task 3 手动验证；§八（明确不做）→ 无对应任务（正确，是"不做"清单）。
- [ ] **残留 grep**：`grep -rn "proxy_enabled" src/ locales/` → 应 0 命中（`docs/superpowers/` 下的本计划与设计文档会命中，那是预期内的，不用改）。注意 `src/config/toml_io.rs` 里保留的是 TOML 键名 `"enabled"`（迁移用），不是 Rust 字段名 `proxy_enabled`，所以不会误命中。
- [ ] **残留 grep**：`grep -rn "Settings.group.http_proxy\|Settings.item.proxy_enabled\|Settings.desc.proxy_enabled" src/ locales/` → 应 0 命中。
- [ ] **跨平台编译**：`cargo check --target x86_64-unknown-linux-gnu`（若本机装了该 target；没装就跳过，但要人工确认 `system_proxy.rs` 里每个 `#[cfg]` 分支都没有拼写错误 —— 非 Windows 分支在本机永远不会被编译到）。
- [ ] **`git status` 复核**：确认没有意外新增文件；`assets/rules/main.json` 是上一个任务的改动，与本计划无关。
