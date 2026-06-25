//! `cli` 模块的单元测试。
//!
//! 历史上 `src/cli.rs` 单文件 549 行既含实现又含测试。
//! 拆分子模块后，测试统一搬到这里。`Cli` / `Cmd` 通过 `super::*` 拿到
//! 公共 re-export，`effective_cfg` 是内部 helper，按模块路径直接取。

use clap::Parser;

use crate::config::{AppConfig, ExportFormat};

use super::{Cli, Cmd, SourcesAction, util::effective_cfg};

#[test]
fn cli_rejects_version_subcommand() {
    // version 子命令已移除，改用 clap 自动注入的 -V / --version。
    let result = Cli::try_parse_from(["so-novel-rs", "version"]);
    assert!(result.is_err(), "version 应不再是子命令");
}

#[test]
fn cli_accepts_version_flag() {
    // --version 现在走手动分发（`SetTrue` + `mod.rs::run` 打印并退出），
    // 不再让 clap 抛 DisplayVersion error。验证 flag 被正确解析为 true 即可。
    let cli = Cli::try_parse_from(["so-novel-rs", "--version"]).unwrap();
    assert!(cli.version_flag, "--version 应被解析为 true");
}

#[test]
fn cli_parses_sources_subcommand() {
    // 裸 `sources` → action=None（兼容旧版，等价于 list）
    let cli = Cli::try_parse_from(["so-novel-rs", "sources"]).unwrap();
    match cli.command.expect("subcommand present") {
        Cmd::Sources { action, json } => {
            assert!(action.is_none());
            assert!(!json);
        }
        _ => panic!("expected Sources"),
    }
}

#[test]
fn cli_parses_sources_list_subcommand() {
    let cli = Cli::try_parse_from(["so-novel-rs", "sources", "list"]).unwrap();
    match cli.command.expect("subcommand present") {
        Cmd::Sources {
            action: Some(SourcesAction::List { json }),
            ..
        } => assert!(!json),
        _ => panic!("expected Sources List"),
    }
}

#[test]
fn cli_parses_sources_list_json_flag() {
    let cli = Cli::try_parse_from(["so-novel-rs", "sources", "list", "--json"]).unwrap();
    match cli.command.expect("subcommand present") {
        Cmd::Sources {
            action: Some(SourcesAction::List { json }),
            ..
        } => assert!(json),
        _ => panic!("expected Sources List with --json"),
    }
}

#[test]
fn cli_parses_sources_bare_json_flag() {
    // 旧版兼容：`sources --json` 仍能解析（action=None, json=true）
    let cli = Cli::try_parse_from(["so-novel-rs", "sources", "--json"]).unwrap();
    match cli.command.expect("subcommand present") {
        Cmd::Sources { action, json } => {
            assert!(action.is_none());
            assert!(json);
        }
        _ => panic!("expected Sources"),
    }
}

#[test]
fn cli_parses_sources_enable() {
    let cli = Cli::try_parse_from(["so-novel-rs", "sources", "enable", "5"]).unwrap();
    match cli.command.expect("subcommand present") {
        Cmd::Sources {
            action: Some(SourcesAction::Enable { id }),
            ..
        } => assert_eq!(id, 5),
        _ => panic!("expected Sources Enable"),
    }
}

#[test]
fn cli_parses_sources_disable() {
    let cli = Cli::try_parse_from(["so-novel-rs", "sources", "disable", "12"]).unwrap();
    match cli.command.expect("subcommand present") {
        Cmd::Sources {
            action: Some(SourcesAction::Disable { id }),
            ..
        } => assert_eq!(id, 12),
        _ => panic!("expected Sources Disable"),
    }
}

#[test]
fn cli_rejects_unknown_sources_subcommand() {
    // `sources help` 等未知子命令应被 clap 拒绝
    let result = Cli::try_parse_from(["so-novel-rs", "sources", "bogus"]);
    assert!(result.is_err(), "未知子命令应被拒绝");
}

#[test]
fn cli_parses_search_with_keyword() {
    let cli = Cli::try_parse_from(["so-novel-rs", "search", "凡人修仙传"]).unwrap();
    match cli.command.expect("subcommand present") {
        Cmd::Search {
            keyword,
            source,
            limit,
            json,
        } => {
            assert_eq!(keyword, "凡人修仙传");
            assert_eq!(source, None);
            assert_eq!(limit, None);
            assert!(!json);
        }
        _ => panic!("expected Search"),
    }
}

#[test]
fn cli_parses_search_with_source_and_limit() {
    let cli = Cli::try_parse_from([
        "so-novel-rs",
        "search",
        "斗破苍穹",
        "--source",
        "3",
        "--limit",
        "10",
    ])
    .unwrap();
    match cli.command.expect("subcommand present") {
        Cmd::Search {
            keyword,
            source,
            limit,
            json,
        } => {
            assert_eq!(keyword, "斗破苍穹");
            assert_eq!(source, Some(3));
            assert_eq!(limit, Some(10));
            assert!(!json);
        }
        _ => panic!("expected Search"),
    }
}

#[test]
fn cli_parses_search_json_flag() {
    let cli = Cli::try_parse_from(["so-novel-rs", "search", "凡人修仙传", "--json"]).unwrap();
    match cli.command.expect("subcommand present") {
        Cmd::Search { json, .. } => assert!(json),
        _ => panic!("expected Search"),
    }
}

#[test]
fn cli_parses_download_with_url_and_overrides() {
    let cli = Cli::try_parse_from([
        "so-novel-rs",
        "download",
        "https://example.com/book/123.html",
        "--source",
        "5",
        "--output",
        "D:\\novels",
        "--format",
        "epub",
    ])
    .unwrap();
    match cli.command.expect("subcommand present") {
        Cmd::Download {
            url,
            source,
            output,
            format,
            from,
            to,
        } => {
            assert_eq!(url, "https://example.com/book/123.html");
            assert_eq!(source, Some(5));
            assert_eq!(output.as_deref(), Some("D:\\novels"));
            assert_eq!(format.as_deref(), Some("epub"));
            assert!(from.is_none());
            assert!(to.is_none());
        }
        _ => panic!("expected Download"),
    }
}

#[test]
fn cli_parses_download_with_from_to() {
    let cli = Cli::try_parse_from([
        "so-novel-rs",
        "download",
        "https://example.com/book/123.html",
        "--from",
        "100",
        "--to",
        "200",
    ])
    .unwrap();
    match cli.command.expect("subcommand present") {
        Cmd::Download { from, to, .. } => {
            assert_eq!(from, Some(100));
            assert_eq!(to, Some(200));
        }
        _ => panic!("expected Download"),
    }
}

#[test]
fn cli_parses_download_from_only() {
    let cli = Cli::try_parse_from([
        "so-novel-rs",
        "download",
        "https://example.com/book",
        "--from",
        "50",
    ])
    .unwrap();
    match cli.command.expect("subcommand present") {
        Cmd::Download { from, to, .. } => {
            assert_eq!(from, Some(50));
            assert!(to.is_none());
        }
        _ => panic!("expected Download"),
    }
}

#[test]
fn cli_parses_download_to_only() {
    let cli = Cli::try_parse_from([
        "so-novel-rs",
        "download",
        "https://example.com/book",
        "--to",
        "30",
    ])
    .unwrap();
    match cli.command.expect("subcommand present") {
        Cmd::Download { from, to, .. } => {
            assert!(from.is_none());
            assert_eq!(to, Some(30));
        }
        _ => panic!("expected Download"),
    }
}

#[test]
fn cli_rejects_download_from_zero() {
    // 1-based：--from 0 应该是合法整数但语义无效；clap 接受，validate_range 阶段报错
    let cli = Cli::try_parse_from([
        "so-novel-rs",
        "download",
        "https://example.com/book",
        "--from",
        "0",
    ])
    .unwrap();
    match cli.command.expect("subcommand present") {
        Cmd::Download { from, .. } => assert_eq!(from, Some(0)),
        _ => panic!("expected Download"),
    }
}

#[test]
fn cli_rejects_unknown_subcommand() {
    let result = Cli::try_parse_from(["so-novel-rs", "bogus"]);
    assert!(result.is_err(), "未知子命令应被 clap 拒绝");
}

#[test]
fn effective_cfg_overrides_output_and_format() {
    let cfg = AppConfig::default();
    let new_cfg = effective_cfg(cfg, Some("D:/out".into()), Some("txt".into()));
    assert_eq!(new_cfg.download_path, "D:/out");
    assert_eq!(new_cfg.ext_name, ExportFormat::Txt);
}

#[test]
fn effective_cfg_keeps_originals_when_no_overrides() {
    let cfg = AppConfig {
        download_path: "orig".into(),
        ext_name: ExportFormat::Html,
        ..AppConfig::default()
    };
    let new_cfg = effective_cfg(cfg, None, None);
    assert_eq!(new_cfg.download_path, "orig");
    assert_eq!(new_cfg.ext_name, ExportFormat::Html);
}

#[test]
fn cli_quiet_long_flag() {
    let cli = Cli::try_parse_from(["so-novel-rs", "--quiet", "sources"]).unwrap();
    assert!(cli.quiet, "--quiet 应被解析为 true");
    assert!(matches!(
        cli.command,
        Some(Cmd::Sources {
            action: None,
            json: false
        })
    ));
}

#[test]
fn cli_quiet_short_flag() {
    // 短选项 `-q` 必须同样有效（与 `-v` 对齐）。
    let cli = Cli::try_parse_from(["so-novel-rs", "-q", "search", "kw"]).unwrap();
    assert!(cli.quiet, "-q 应被解析为 true");
    match cli.command.expect("subcommand present") {
        Cmd::Search { keyword, .. } => assert_eq!(keyword, "kw"),
        _ => panic!("expected Search"),
    }
}

#[test]
fn cli_quiet_global_position() {
    // global flag：放在子命令前 / 后两种位置都应生效。
    let pre = Cli::try_parse_from(["so-novel-rs", "-q", "sources"]).unwrap();
    let post = Cli::try_parse_from(["so-novel-rs", "sources", "-q"]).unwrap();
    assert!(pre.quiet && post.quiet, "global flag 在子命令前后都应生效");
}

#[test]
fn cli_quiet_default_false() {
    // 默认 false：脚本里不传 --quiet 时不应被意外抑制。
    let cli = Cli::try_parse_from(["so-novel-rs", "sources"]).unwrap();
    assert!(!cli.quiet);
}

#[test]
fn cli_verbose_and_quiet_combine() {
    // --verbose 显式打开日志，--quiet 仅影响 stderr 用户输出，两者不冲突。
    let cli = Cli::try_parse_from(["so-novel-rs", "--verbose", "-q", "search", "kw"]).unwrap();
    assert!(cli.verbose);
    assert!(cli.quiet);
}
