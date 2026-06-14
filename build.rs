//! Windows 资源嵌入：把 assets/logo.ico 编译进 exe 的资源段，
//! 这样资源管理器、任务栏、Alt-Tab 都能看到图标。
//!
//! 非 Windows 目标 (Linux / macOS) 此 build.rs 是 no-op。

fn main() {
    #[cfg(target_os = "windows")]
    {
        // 仅当 ico 存在时才嵌入，缺文件不阻止 cargo build。
        let ico = std::path::Path::new("assets").join("logo.ico");
        if ico.exists() {
            let mut res = winres::WindowsResource::new();
            res.set_icon(ico.to_str().expect("ico path is valid utf-8"));
            if let Err(e) = res.compile() {
                // 通常是 rc.exe 缺失（用户没装 Windows SDK）—
                // warn 但不 panic，让 build 仍能产出 exe（无图标）。
                println!("cargo:warning=embed icon failed: {e}");
            }
        } else {
            println!("cargo:warning=assets/logo.ico not found, skip exe icon embed");
        }
    }

    println!("cargo:rerun-if-changed=assets/logo.ico");
}
