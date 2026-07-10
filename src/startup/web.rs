//! Web 服务模式启动：解析 `--host` / `--port`，初始化共享资源，
//! 构造 axum 服务并阻塞运行。
//!
//! 仅在 `feature = "web"` 启用时提供真实实现；其它构建走 `bail!`
//! 友好提示用户加 `--features web` 重编。

/// 从命令行参数中提取 `--key value` 形式的值。
///
/// `pub(super)` —— 只允许 `startup::mod.rs`（用于 `detect` 解析 `--host` / `--port`
/// 构造 `LaunchMode::Web`）和本模块内 `run` 之外的调用方使用。`run` 内部已
/// 通过 `LaunchMode::Web { host, port }` 拿到解析后的值，不再读 argv。
pub(super) fn parse_arg_value_pub(args: &[String], key: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == key {
            return iter.next().cloned();
        }
    }
    None
}

/// Web 服务模式：初始化共享资源并启动 axum 服务器。
///
/// `host` / `port` 已由 [`super::detect`] 从 argv + `SO_NOVEL_WEB` env
/// 解出并通过 `LaunchMode::Web` 传过来，这里不再做参数解析。
#[cfg(feature = "web")]
pub fn run(host: String, port: u16) -> anyhow::Result<()> {
    // Phase 3.3：启动期公共资源（paths / config / sources_config / rules / http）
    // 统一收敛到 `core::bootstrap::load_context`，三端共享同一套兜底矩阵。
    let ctx = crate::core::bootstrap::load_context();

    // 加载历史任务 → `Vec<DownloadTask>`。复用 `db::load_tasks_from_file`：
    // 它已经把 `finished.is_none()` 的历史记录标成 `AppRestarted` 并落盘（上次
    // 退出时还在跑的任务），跟 GUI 启动走完全同一条路径。
    let (tasks, next_task_id) = crate::db::load_tasks_from_file(&ctx.paths.tasks_file);

    let params = crate::web::WebInitParams {
        sources_config: ctx.sources_config,
        sources_config_path: ctx.paths.sources_config,
        tasks,
        tasks_file: ctx.paths.tasks_file,
        next_task_id,
    };
    crate::web::run(ctx.config, ctx.http, ctx.rules, params, host, port)
}

/// 当前构建不含 Web 功能。用户给了 `--web` / `SO_NOVEL_WEB=1` 但 binary
/// 是 `--no-default-features --features gui` 或 `--no-default-features` 编出来的。
#[cfg(not(feature = "web"))]
pub fn run(_host: String, _port: u16) -> anyhow::Result<()> {
    anyhow::bail!("当前构建不含 Web 功能（需 --features web）")
}
