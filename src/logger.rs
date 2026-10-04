//! 日志系统：`tracing_subscriber` 的全局初始化 + 输出格式选择。
//!
//! - text 模式（默认）：人类可读文本 + ANSI 颜色；JSON 模式由 `LOG_FORMAT=json` 切换，
//!   生产/容器环境用，便于聚合栈 (Loki / ELK) parse
//! - env filter 走 `RUST_LOG`（默认 `info,so_novel_rs=debug`）
//! - **`tracing_subscriber::init` 全局唯一，二次 init 会 panic**，须由 caller 自行保证：
//!   `main` 只在启动时调一次 `logger::init()`
//!
//! tracing macro 本身与 `TraceId` 链路（`app::trace`）不在本模块。

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
/// `LOG_FORMAT` 是无效值时 panic；需非 panic 路径用 `init_with_format`。
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
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
}
