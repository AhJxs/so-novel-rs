//! Build script: Vite frontend auto-build (web feature) + Windows icon resource.

use std::process::Command;

fn main() {
    // ── Frontend build: only when web feature is enabled ──────────────────
    println!("cargo:rerun-if-changed=web-ui/src");
    println!("cargo:rerun-if-changed=web-ui/package.json");
    println!("cargo:rerun-if-changed=web-ui/vite.config.ts");
    println!("cargo:rerun-if-changed=web-ui/index.html");

    if std::env::var("CARGO_FEATURE_WEB").is_ok() {
        // SO_NOVEL_SKIP_WEB_BUILD=1: explicitly skip `npm run build` here.
        // Only intended for Rust static-analysis runs where the caller has
        // already produced web-ui/dist/. Release / Docker builds must leave
        // this unset so the latest frontend is compiled in.
        if std::env::var("SO_NOVEL_SKIP_WEB_BUILD").as_deref() == Ok("1") {
            let index = std::path::Path::new("web-ui/dist/index.html");
            if !index.exists() {
                panic!(
                    "SO_NOVEL_SKIP_WEB_BUILD=1 set but web-ui/dist/index.html is missing; \
                     pre-build with `npm run build --prefix web-ui` or unset the flag."
                );
            }
            println!("cargo:warning=SO_NOVEL_SKIP_WEB_BUILD=1, reusing web-ui/dist/");
        } else {
            #[cfg(target_os = "windows")]
            let mut cmd = {
                // On Windows, `npm` is `npm.cmd` — `cmd /c` resolves it reliably
                // through %PATHEXT%, even when cargo inherits a bash-modified PATH.
                let mut c = Command::new("cmd");
                c.args(["/c", "npm", "run", "build", "--prefix", "web-ui"]);
                c
            };
            #[cfg(not(target_os = "windows"))]
            let mut cmd = {
                let mut c = Command::new("npm");
                c.args(["run", "build", "--prefix", "web-ui"]);
                c
            };

            run_npm_build(&mut cmd);
        }
    }

    // ── Windows icon resource ────────────────────────────────────────────
    println!("cargo:rerun-if-changed=assets/logo.ico");

    #[cfg(target_os = "windows")]
    {
        let ico = std::path::Path::new("assets").join("logo.ico");
        if ico.exists() {
            let mut res = winres::WindowsResource::new();
            res.set_icon(ico.to_str().expect("ico path is valid utf-8"));
            if let Err(e) = res.compile() {
                println!("cargo:warning=embed icon failed: {e}");
            }
        } else {
            println!("cargo:warning=assets/logo.ico not found, skip exe icon embed");
        }
    }
}

/// Run `cmd` (npm / cmd) and handle failures gracefully.
fn run_npm_build(cmd: &mut Command) {
    match cmd.status() {
        Ok(status) => {
            if !status.success() {
                panic!("Vite build failed — check web-ui/ for errors");
            }
        }
        Err(e) => {
            // npm not found (e.g. CI without Node.js, or non-standard PATH).
            // Only fatal if web-ui/dist/ doesn't already exist.
            let index = std::path::Path::new("web-ui/dist/index.html");
            if !index.exists() {
                panic!(
                    "npm not found ({e}) and web-ui/dist/index.html is missing. \
                     Install Node.js or pre-build the frontend with `npm run build --prefix web-ui`."
                );
            }
            println!("cargo:warning=npm not found ({e}), using pre-built web-ui/dist/");
        }
    }
}
