//! User-Agent 池。对应 Java `util.RandomUA`。
//!
//! 与 Java 端逻辑一致：随机选 OS + 浏览器 + 主版本号，按 4 种格式拼接。
//! 主要用途是规避一部分书源对单一 UA 的速率限制。

use rand::RngExt;

const DESKTOP_OS: &[&str] = &[
    "Windows NT 6.1; Win64; x64",
    "Windows NT 10.0; Win64; x64",
    "Windows NT 11.0; Win64; x64",
    "Macintosh; Intel Mac OS X 10_15_7",
    "X11; Linux x86_64",
    "X11; Ubuntu; Linux x86_64",
];

const BROWSERS: &[&str] = &["Chrome", "Firefox", "Safari", "Edge"];

const MIN_VERSION: u32 = 86;
const MAX_VERSION: u32 = 145;

pub fn random_ua() -> String {
    let mut rng = rand::rng();
    let os = DESKTOP_OS[rng.random_range(0..DESKTOP_OS.len())];
    let browser = BROWSERS[rng.random_range(0..BROWSERS.len())];
    let major = rng.random_range(MIN_VERSION..=MAX_VERSION);
    let minor = rng.random_range(0..10);
    let build = rng.random_range(0..1000);

    match browser {
        "Chrome" => format!(
            "Mozilla/5.0 ({os}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{major}.0.{build} Safari/537.36"
        ),
        "Firefox" => format!("Mozilla/5.0 ({os}; rv:{major}.0) Gecko/20100101 Firefox/{major}.0"),
        "Safari" => format!(
            "Mozilla/5.0 ({os}) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/{major}.1 Safari/537.36"
        ),
        "Edge" => format!(
            "Mozilla/5.0 ({os}) AppleWebKit/537.36 (KHTML, like Gecko) Edge/{major}.0.{minor}.0 Safari/537.36"
        ),
        _ => "Unknown User-Agent".to_string(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn ua_has_mozilla_prefix_and_browser_token() {
        for _ in 0..50 {
            let ua = random_ua();
            assert!(ua.starts_with("Mozilla/5.0"), "bad UA: {ua}");
            // 任一浏览器关键字必须出现
            assert!(
                BROWSERS.iter().any(|b| ua.contains(b)) || ua.contains("Version/"),
                "no browser token in {ua}"
            );
        }
    }
}
