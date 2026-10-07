//! 系统代理探测。
//!
//! reqwest **没有**开 `system-proxy` feature（见 `Cargo.toml`），所以「用系统代理」必须
//! 自己读：Windows 读 `WinINET` 注册表，其它平台读环境变量。
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
    /// 没有可用代理。`reason` 用于设置页说明「为什么没生效」。
    Absent { reason: AbsentReason },
}

/// [`SystemProxy::Absent`] 的原因。平台各自只会产生其中一个子集
/// （Windows 用不到 `EnvUnset`，其它平台用不到 `PacOnly` / `SocksOnly`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsentReason {
    /// 系统代理开关是关的（Windows `ProxyEnable` = 0）。
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
    let port = p.proxy_port;
    Some(format!("http://{host}:{port}"))
}

/// Windows：读 `WinINET` 系统代理。
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

    let Ok(raw) = key.get_value::<String, _>("ProxyServer") else {
        // 开关开着却没有 `ProxyServer`：要么是 PAC 模式，要么是残留状态。
        return if key.get_value::<String, _>("AutoConfigURL").is_ok() {
            SystemProxy::Absent {
                reason: AbsentReason::PacOnly,
            }
        } else {
            SystemProxy::Absent {
                reason: AbsentReason::NotEnabled,
            }
        };
    };

    let raw = raw.trim();
    if raw.is_empty() {
        return SystemProxy::Absent {
            reason: AbsentReason::NotEnabled,
        };
    }

    // 有 `ProxyServer` 但解析不出 HTTP 代理（例如只有 `socks=...`）—— 当前不支持。
    parse_proxy_server(raw).map_or(
        SystemProxy::Absent {
            reason: AbsentReason::SocksOnly,
        },
        SystemProxy::Found,
    )
}

/// 解析 `WinINET` 的 `ProxyServer` 值。两种形态：
///
/// - `"127.0.0.1:7890"` —— 所有协议共用；
/// - `"http=127.0.0.1:7890;https=127.0.0.1:7891;ftp=..."` —— 按协议分列。
///
/// 只支持 HTTP 代理：优先 `http=` 项，其次第一个非 `socks*=` 项
/// （`https=` 的值也是 HTTP 代理 —— 走 CONNECT，所以仍拼 `http://`）；
/// 一个可用项都没有（例如只配了 `socks=127.0.0.1:1080`）→ `None`。
///
/// 非 Windows 构建下唯一生产调用点在 `detect_windows` 里（被 cfg 掉），只剩测试在用。
#[cfg_attr(
    not(target_os = "windows"),
    allow(
        dead_code,
        reason = "唯一生产调用点在 detect_windows 内，非 Windows 构建下只被单元测试使用"
    )
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
        assert_eq!(
            resolve_proxy_url(&cfg(ProxyMode::None, "127.0.0.1", 7890)),
            None
        );
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
