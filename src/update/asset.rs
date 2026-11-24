//! 按当前平台从 release 产物清单里挑出要下载的包。
//!
//! 命名约定来自 `.github/workflows/release.yml`：
//! `so-novel-rs-<version>-<os>-<arch>.<zip|tar.gz>`，`<version>` 不带 `v` 前缀。

use crate::core::update::ReleaseAsset;

/// 当前平台对应的产物文件名；不发布该平台（如 Intel macOS）时返回 `None`。
#[must_use]
pub fn current_asset_name(version: &str) -> Option<String> {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        return None;
    };
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    Some(format!("so-novel-rs-{version}-{os}-{arch}.{ext}"))
}

/// 在产物清单里找当前平台的包；找不到返回 `None`（调用方回退到浏览器下载）。
#[must_use]
pub fn pick_asset<'a>(assets: &'a [ReleaseAsset], version: &str) -> Option<&'a ReleaseAsset> {
    let name = current_asset_name(version)?;
    assets.iter().find(|asset| asset.name == name)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    fn asset(name: &str) -> ReleaseAsset {
        ReleaseAsset {
            name: name.to_string(),
            url: format!("https://example.com/{name}"),
            size: 1,
        }
    }

    #[test]
    fn current_name_matches_release_workflow_naming() {
        let name = current_asset_name("0.7.2").expect("常见平台应支持自更新");
        let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
        assert!(name.starts_with("so-novel-rs-0.7.2-"));
        assert!(name.ends_with(ext));
    }

    #[test]
    fn pick_asset_finds_exact_name_only() {
        let expected = current_asset_name("1.2.3").expect("支持当前平台");
        let assets = vec![
            asset("so-novel-rs-1.2.3-unknown-arch.zip"),
            asset(&expected),
            asset("so-novel-rs-1.2.4-linux-x86_64.tar.gz"),
        ];
        assert_eq!(
            pick_asset(&assets, "1.2.3").map(|a| a.name.as_str()),
            Some(expected.as_str())
        );
    }

    #[test]
    fn pick_asset_returns_none_when_missing() {
        let assets = vec![asset("something-else.zip")];
        assert!(pick_asset(&assets, "1.2.3").is_none());
    }
}
