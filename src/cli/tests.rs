//! `cli` 模块的单元测试。
//!
//! 历史上 `src/cli.rs` 单文件 549 行既含实现又含测试。
//! 拆分子模块后，测试统一搬到这里。`Cli` / `Cmd` 通过 `super::*` 拿到
//! 公共 re-export，`effective_cfg` 是内部 helper，按模块路径直接取。

use clap::{CommandFactory, Parser};

use crate::config::{AppConfig, ExportFormat, Language};

use super::{
    Cli, Cmd, SourcesAction, build_localized_command, subcommand_name, util::effective_cfg,
};

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

/// `build_localized_command` 与 derive `Cli::command()` 在 arg IDs / subcommand
/// 名称集合上必须等价 —— 这是手搓版"行为兼容 derive"的唯一机械性保证。
///
/// 任何对 `build_localized_command` 的改动如果漏了某个 arg / subcommand，或
/// 改了 arg id / short / long 标识，都会被这条测试抓住。
#[test]
fn localized_command_matches_derive_structure() {
    let derive = Cli::command();
    let localized = build_localized_command(Language::English);

    // 顶层 arg id 集合（顺序不要求——用 BTreeSet 排序比较）。
    let derive_args: std::collections::BTreeSet<String> = derive
        .get_arguments()
        .map(|a| a.get_id().to_string())
        .collect();
    let localized_args: std::collections::BTreeSet<String> = localized
        .get_arguments()
        .map(|a| a.get_id().to_string())
        .collect();
    assert_eq!(
        derive_args, localized_args,
        "顶层 arg ID 集合必须与 derive 完全一致"
    );

    // subcommand 名称集合。
    let derive_subs: std::collections::BTreeSet<String> = derive
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    let localized_subs: std::collections::BTreeSet<String> = localized
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    assert_eq!(
        derive_subs, localized_subs,
        "subcommand 名称集合必须与 derive 完全一致"
    );

    // sources 必须有 list / enable / disable 三个 sub-subcommand。
    let sources = localized
        .find_subcommand("sources")
        .expect("sources subcommand must exist");
    let source_subs: std::collections::BTreeSet<String> = sources
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    assert_eq!(
        source_subs,
        ["disable", "enable", "list"]
            .into_iter()
            .map(String::from)
            .collect(),
        "sources 必须有 list / enable / disable 三个 sub-subcommand"
    );
}

/// 三种 locale 下 `build_localized_command` 的顶层 `about` / `long_about`
/// 必须互不相同 —— 这是"真的拿到翻译"的最小可见断言。
///
/// `build_localized_command` 内部会调 `rust_i18n::set_locale` + 清缓存，
/// 整个进程全局生效。结尾恢复 en 避免污染后续测试（与 `i18n::tests::ts_and_ts_fmt_work`
/// 同样的收尾策略）。
#[test]
fn localized_command_about_changes_with_language() {
    let en = build_localized_command(Language::English);
    let zh = build_localized_command(Language::SimplifiedChinese);
    let hk = build_localized_command(Language::TraditionalChinese);

    let en_about = en.get_about().expect("en about").to_string();
    let zh_about = zh.get_about().expect("zh-CN about").to_string();
    let hk_about = hk.get_about().expect("zh-HK about").to_string();

    assert_ne!(en_about, zh_about, "en 与 zh-CN about 应不同");
    assert_ne!(en_about, hk_about, "en 与 zh-HK about 应不同");
    assert_ne!(zh_about, hk_about, "zh-CN 与 zh-HK about 应不同");

    // 简单自检：英文 about 包含 "So Novel"，繁简 about 包含 "批量下載"/"批量下载"。
    assert!(en_about.contains("So Novel"));
    assert!(zh_about.contains("批量下载"));
    assert!(hk_about.contains("批量下載"));

    // 收尾：恢复 en。
    rust_i18n::set_locale("en");
    crate::i18n::invalidate_cache();
}

/// `subcommand_name` 把 `Cmd` variant 映射到 clap 子命令名。
#[test]
fn subcommand_name_maps_variants() {
    let search = Cli::try_parse_from(["so-novel-rs", "search", "kw"]).unwrap();
    let download = Cli::try_parse_from(["so-novel-rs", "download", "http://x"]).unwrap();
    let sources = Cli::try_parse_from(["so-novel-rs", "sources"]).unwrap();

    assert_eq!(subcommand_name(search.command.as_ref().unwrap()), "search");
    assert_eq!(
        subcommand_name(download.command.as_ref().unwrap()),
        "download"
    );
    assert_eq!(
        subcommand_name(sources.command.as_ref().unwrap()),
        "sources"
    );
}

/// `locale_for` 是项目里 `Language → locale 字符串` 的唯一权威映射。
/// 测试三种 enum 的输出 + 关键差异（TraditionalChinese 返回 `zh-HK` 而
/// **不是** `Language::as_str()` 的 `zh-TW`）。
#[test]
fn locale_for_maps_to_rust_i18n_tags() {
    assert_eq!(
        crate::i18n::locale_for(Language::SimplifiedChinese),
        "zh-CN"
    );
    assert_eq!(
        crate::i18n::locale_for(Language::TraditionalChinese),
        "zh-HK"
    );
    assert_eq!(crate::i18n::locale_for(Language::English), "en");
}
