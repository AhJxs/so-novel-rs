//! tracing 初始化（仅 stdout）。

use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// 初始化 tracing：仅 stdout layer。
pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,so_novel_rs=debug"));

    let stdout_layer = fmt::layer().with_target(false);

    tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer)
        .init();
}

#[cfg(test)]
mod tests {
    /// 确认 tracing 初始化不 panic（stdout-only 模式）。
    #[test]
    fn init_tracing_does_not_panic() {
        use tracing_subscriber::prelude::*;
        // set_default 会覆盖当前线程的 subscriber，测试结束自动恢复。
        let filter = tracing_subscriber::EnvFilter::new("info");
        let layer = tracing_subscriber::fmt::layer().with_target(false);
        let _guard = tracing_subscriber::registry()
            .with(filter)
            .with(layer)
            .set_default();

        tracing::info!("smoke test");
    }
}
