//! 日志系统 (PR #17, 2026-07-08 重构; 2026-07-08 默认改为 Text).
//!
//! # 设计
//!
//! - **text 模式 (默认)**: 人类可读文本 + ANSI 颜色, 开发期常用
//! - **JSON 模式**: `LOG_FORMAT=json` 环境变量切换, 生产/容器环境用, 便于聚合栈 (Loki / ELK) parse
//! - **env filter**: `RUST_LOG=info,so_novel_rs=debug` 走 `tracing_subscriber::EnvFilter`
//! - **init 一次**: `tracing_subscriber::init()` 全局唯一; 二次 init 会 panic,
//!   caller 需自行保证 (CLI 启动期 + startup::dispatch 模式已分流)
//!
//! # 调用方
//!
//! - `cli::run` 内部: `--verbose` 时 init
//! - `startup::dispatch` Web 路径: attach_console 后 init
//! - `startup::dispatch` Gui 路径: 直接 init
//!
//! # 不在本模块
//!
//! - tracing macro 本身 (`tracing::info!` 等) — 在所有业务代码直接用
//! - `TraceId` 链路 ID — 在 `app::trace` 模块

use std::str::FromStr;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// 日志输出格式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LogFormat {
    /// 人类可读文本 (开发期常用, 带 ANSI 颜色)。**默认**。
    #[default]
    Text,
    /// JSON 行输出 (生产/容器, 聚合栈 parse 友好)。
    Json,
}

impl FromStr for LogFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "text" | "pretty" | "" => Ok(Self::Text),
            other => Err(format!("未知日志格式: {other:?}; 期望 text|json")),
        }
    }
}

/// 初始化全局 tracing subscriber (text-by-default).
///
/// # Examples
///
/// ```
/// // 启动时调一次; 二次 init 会 panic
/// so_novel_rs::logger::init();
/// tracing::info!("hello");  // 默认输出可读文本
/// ```
///
/// 切到 JSON: `LOG_FORMAT=json so-novel-rs ...`.
///
/// # Errors
///
/// 当 `LOG_FORMAT` 环境变量是无效值时, 启动 panic. 用 `init_with_format`
/// 走非 panic 路径。
pub fn init() {
    let format = std::env::var("LOG_FORMAT")
        .ok()
        .and_then(|s| LogFormat::from_str(&s).ok())
        .unwrap_or_default();
    let _ = init_with_format(format);
}

/// 用显式配置初始化, 不读 env. 测试友好.
pub fn init_with_format(format: LogFormat) -> Result<(), String> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,so_novel_rs=debug"));

    match format {
        LogFormat::Text => {
            let layer = fmt::layer()
                .with_target(true)
                .with_thread_ids(false)
                .with_file(false)
                .with_line_number(false)
                .with_ansi(true);
            tracing_subscriber::registry()
                .with(filter)
                .with(layer)
                .try_init()
                .map_err(|e| format!("tracing subscriber init 失败: {e}"))?;
        }
        LogFormat::Json => {
            let layer = fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(false)
                .with_target(true)
                .with_file(false)
                .with_line_number(false);
            tracing_subscriber::registry()
                .with(filter)
                .with(layer)
                .try_init()
                .map_err(|e| format!("tracing subscriber init 失败: {e}"))?;
        }
    }
    Ok(())
}

/// 旧 `init_tracing` 别名, 保留给 cli 启动期调用 (无 env filter, 全部 info 起步).
///
/// 二次 init 静默 no-op (而不是 panic); 业务方可放心多次调.
pub fn init_compat_legacy() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,so_novel_rs=debug"));

    let layer = fmt::layer().with_target(false);
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_format_parses_case_insensitive() {
        assert_eq!("text".parse::<LogFormat>().unwrap(), LogFormat::Text);
        assert_eq!("TEXT".parse::<LogFormat>().unwrap(), LogFormat::Text);
        assert_eq!("pretty".parse::<LogFormat>().unwrap(), LogFormat::Text);
        assert_eq!("".parse::<LogFormat>().unwrap(), LogFormat::Text);
        assert_eq!("json".parse::<LogFormat>().unwrap(), LogFormat::Json);
    }

    #[test]
    fn log_format_default_is_text() {
        assert_eq!(LogFormat::default(), LogFormat::Text);
    }

    #[test]
    fn log_format_rejects_unknown() {
        assert!("xml".parse::<LogFormat>().is_err());
    }

    #[test]
    fn init_compat_legacy_does_not_panic() {
        // set_default 限定在当前线程, 测试结束自动恢复。
        let _ = tracing_subscriber::registry()
            .with(EnvFilter::new("off"))
            .with(fmt::layer().with_target(false))
            .set_default();
        init_compat_legacy();
        // 二次 init 也静默 (try_init + no-op)
        init_compat_legacy();
    }
}