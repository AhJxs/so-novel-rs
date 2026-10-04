//! Build script: Windows icon resource.

fn main() {
    // ── Windows icon resource ────────────────────────────────────────────
    println!("cargo:rerun-if-changed=assets/logo.ico");

    #[cfg(target_os = "windows")]
    {
        let ico = std::path::Path::new("assets").join("logo.ico");
        if ico.exists() {
            let mut res = winres::WindowsResource::new();
            // 局部 `#[allow]` 而非 crate-level 抑制，避免误伤业务代码。
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
