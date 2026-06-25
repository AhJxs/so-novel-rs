// Windows release 下走 GUI subsystem，避免 GUI 启动时弹出控制台黑窗。
// CLI/Web 模式通过 `attach_parent_console` 挂载到父进程控制台；
// 若没有父控制台（如从 Explorer 启动），则自行 `AllocConsole` 分配一个新控制台，
// 确保 stdout/stderr 始终有效。
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use anyhow::Result;

/// Web 服务默认配置。
const DEFAULT_WEB_HOST: &str = "127.0.0.1";
const DEFAULT_WEB_PORT: u16 = 8080;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // 判断启动模式：--web → Web 服务；其它参数 → CLI；无参数 → GUI。
    // 环境变量 SO_NOVEL_WEB=1 也可触发 Web 模式（Docker 友好）。
    let is_web = args.iter().any(|a| a == "--web")
        || std::env::var("SO_NOVEL_WEB")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false);
    let is_cli = !is_web && args.len() > 1;

    // GUI subsystem 的 exe 默认没有 stdio 句柄；从 cmd / PowerShell 跑 CLI 子命令
    // 时附加到父进程的控制台。
    if is_cli || is_web {
        attach_parent_console();
    }

    if is_cli {
        return so_novel_rs::cli::run();
    } else {
        so_novel_rs::logging::init_tracing();
    }

    if is_web {
        let host = parse_arg_value(&args, "--host").unwrap_or_else(|| DEFAULT_WEB_HOST.to_string());
        let port = parse_arg_value(&args, "--port")
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(DEFAULT_WEB_PORT);
        return run_web(host, port);
    }

    // 启动 GPUI + gpui-component GUI。
    so_novel_rs::gpui_app::run()
}

/// 从命令行参数中提取 `--key value` 形式的值。
fn parse_arg_value(args: &[String], key: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == key {
            return iter.next().cloned();
        }
    }
    None
}

/// Web 服务模式：初始化共享资源并启动 axum 服务器。
fn run_web(host: String, port: u16) -> Result<()> {
    use so_novel_rs::config::{ConfigPaths, load_config};
    use so_novel_rs::http::HttpClients;
    use so_novel_rs::persistent::{SourcesConfig, init_rules_dir, load_active_rules, load_tasks};

    let paths = ConfigPaths::discover();
    let config = load_config(&paths.config_file).unwrap_or_default();

    // 初始化规则目录
    if let Err(e) = init_rules_dir(&paths.rules_dir) {
        tracing::warn!("规则目录初始化失败: {e:#}");
    }

    let sources_config = SourcesConfig::load(&paths.sources_config);
    let rules = load_active_rules(&paths.rules_dir, &sources_config).unwrap_or_default();
    let http = HttpClients::new(&config)?;

    // 加载历史任务，从历史最大 ID + 1 开始分配新 ID
    let task_history = load_tasks(&paths.tasks_file);
    let next_task_id = task_history.iter().map(|t| t.id).max().unwrap_or(0) + 1;

    let params = so_novel_rs::web::WebInitParams {
        sources_config,
        sources_config_path: paths.sources_config,
        task_history,
        tasks_file: paths.tasks_file,
        next_task_id,
    };
    so_novel_rs::web::run(config, http.into(), rules, params, host, port)
}

/// 把当前进程附加到父进程的控制台（仅 Windows），失败时自行分配。
///
/// `AttachConsole(ATTACH_PARENT_PROCESS)`：
/// - 从 cmd/PowerShell 跑时成功，stdout/stderr 直通父终端；
/// - 双击 / GUI shell 启动时父进程没有控制台，调用失败 →
///   回退 `AllocConsole()`` 分配一个新控制台窗口，确保 CLI/Web 仍有 stdio。
///
/// debug build 是 console subsystem，本来就有自己的窗口，此函数静默成功。
#[cfg(target_os = "windows")]
fn attach_parent_console() {
    unsafe {
        use windows_sys::Win32::System::Console::{
            ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole,
        };
        // 先尝试挂载到父进程控制台
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            // 没有父控制台（如从 Explorer 启动），自行分配一个
            AllocConsole();
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn attach_parent_console() {}
