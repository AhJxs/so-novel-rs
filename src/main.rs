use std::sync::Arc;

use anyhow::Result;
use so_novel_rs::app::SoNovelApp;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// 嵌入式 PNG logo（编译时 include_bytes!，无运行时 IO）。
const LOGO_PNG: &[u8] = include_bytes!("../assets/logo.png");

fn main() -> Result<()> {
    init_tracing();

    // 任意 argv（除程序名外）→ CLI 模式；不带任何参数 → GUI。
    // 与 Unix 惯例一致：so-novel-rs search <kw> / so-novel-rs sources / etc.
    if std::env::args().len() > 1 {
        return so_novel_rs::cli::run();
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("So Novel")
        // 关键：关掉原生标题栏（去左上角图标 + 软件名）。
        // 关闭/最小化/最大化按钮改由自定义 nav 栏右侧的 svg 按钮承担。
        .with_decorations(false)
        .with_inner_size([1180.0, 760.0])
        .with_min_inner_size([900.0, 600.0]);

    // 解码 logo.png → IconData（RGBA），失败时不带图标启动而非崩。
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(Arc::new(icon));
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "So Novel",
        native_options,
        Box::new(|cc| Ok(Box::new(SoNovelApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe run_native failed: {e}"))?;

    Ok(())
}

/// 把嵌入的 PNG 解码为 egui IconData。失败返回 None，让应用以无图标启动。
fn load_icon() -> Option<egui::IconData> {
    use std::io::Cursor;
    let img = image::ImageReader::new(Cursor::new(LOGO_PNG))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?
        .to_rgba8();
    let (w, h) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    })
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,so_novel_rs=debug"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false))
        .init();
}
