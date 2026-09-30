//! Web 服务模式启动: 解析 `--host` / `--port`, 初始化共享资源, 构造 axum 服务并阻塞运行。
//! 仅在 `feature = "web"` 启用时提供真实实现; 其它构建走 `bail!` 提示加 `--features web` 重编。

/// 从命令行参数中提取 `--key value` 形式的值。
/// `pub(super)`: 只给 `startup::mod.rs` 的 `detect` 用 —— `run` 内部已从 `LaunchMode::Web`
/// 拿到解析后的值, 不再读 argv。
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
/// `host` / `port` 已由 [`super::detect`] 解出并通过 `LaunchMode::Web` 传过来，这里不再解析参数。
#[cfg(feature = "web")]
pub fn run(host: String, port: u16) -> anyhow::Result<()> {
    // 启动期公共资源（paths / config / sources_config / rules / http）统一由 `load_context` 装载。
    let ctx = crate::core::bootstrap::load_context();

    // 加载历史任务 → `Vec<DownloadTask>`。复用 `db::load_tasks_from_file`：它已把
    // `finished.is_none()` 的历史记录标成 `AppRestarted` 并落盘，与 GUI 启动同一条路径。
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

/// 当前构建不含 Web 功能（binary 是 `--no-default-features` 编出来的）。
#[cfg(not(feature = "web"))]
pub fn run(_host: String, _port: u16) -> anyhow::Result<()> {
    anyhow::bail!("当前构建不含 Web 功能（需 --features web）")
}
