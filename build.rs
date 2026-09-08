//! Build script: Vite frontend auto-build (web feature) + Windows icon resource.

use std::process::Command;

/// 前端产物目录（monorepo 后：`web-ui/apps/web/dist`）。
const FRONTEND_DIST: &str = "web-ui/apps/web/dist";

fn main() {
    // ── Frontend build: only when web feature is enabled ──────────────────
    println!("cargo:rerun-if-changed=web-ui/apps/web/src");
    println!("cargo:rerun-if-changed=web-ui/apps/web/package.json");
    println!("cargo:rerun-if-changed=web-ui/apps/web/vite.config.ts");
    println!("cargo:rerun-if-changed=web-ui/apps/web/index.html");
    println!("cargo:rerun-if-changed=web-ui/packages/ui/src");

    if std::env::var("CARGO_FEATURE_WEB").is_ok() {
        // SO_NOVEL_SKIP_WEB_BUILD=1: explicitly skip `bun run build` here.
        // Only intended for Rust static-analysis runs where the caller has
        // already produced the frontend dist. Release / Docker builds must
        // leave this unset so the latest frontend is compiled in.
        if std::env::var("SO_NOVEL_SKIP_WEB_BUILD").as_deref() == Ok("1") {
            let index = std::path::Path::new(FRONTEND_DIST).join("index.html");
            assert!(
                index.exists(),
                "SO_NOVEL_SKIP_WEB_BUILD=1 set but {FRONTEND_DIST}/index.html is missing; \
                 pre-build with `cd web-ui && bun run build` or unset the flag."
            );
            println!("cargo:warning=SO_NOVEL_SKIP_WEB_BUILD=1, reusing {FRONTEND_DIST}/");
        } else {
            run_bun_build();
        }
    }

    // ── Windows icon resource ────────────────────────────────────────────
    println!("cargo:rerun-if-changed=assets/logo.ico");

    #[cfg(target_os = "windows")]
    {
        let ico = std::path::Path::new("assets").join("logo.ico");
        if ico.exists() {
            let mut res = winres::WindowsResource::new();
            // ico path 是构建期固定常量，UTF-8 无效实际不会发生；保留 expect 行为
            // 配合局部 `#[allow]` 而非 crate-level 抑制，避免误伤业务代码。
            #[allow(clippy::expect_used)]
            let icon_str = ico.to_str().expect("ico path is valid utf-8");
            res.set_icon(icon_str);
            if let Err(e) = res.compile() {
                println!("cargo:warning=embed icon failed: {e}");
            }
        } else {
            println!("cargo:warning=assets/logo.ico not found, skip exe icon embed");
        }
    }
}

/// 在 web-ui 根跑 `bun run build`（turbo 全链构建 → apps/web/dist）。
fn run_bun_build() {
    let mut cmd = Command::new("bun");
    cmd.args(["run", "build"]).current_dir("web-ui");
    match cmd.status() {
        Ok(status) => {
            assert!(
                status.success(),
                "bun run build failed — check web-ui/ for errors"
            );
        }
        Err(e) => {
            // bun not found (e.g. CI without bun, or non-standard PATH).
            // Only fatal if the frontend dist doesn't already exist.
            let index = std::path::Path::new(FRONTEND_DIST).join("index.html");
            assert!(
                index.exists(),
                "bun not found ({e}) and {FRONTEND_DIST}/index.html is missing. \
                 Install bun or pre-build the frontend with `cd web-ui && bun run build`."
            );
            println!("cargo:warning=bun not found ({e}), using pre-built {FRONTEND_DIST}/");
        }
    }
}
