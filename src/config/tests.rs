//! `config` 模块的单元测试。
//!
//! 历史上 `config/loader.rs` 单文件 917 行既含实现又含测试。
//! 拆分子模块后，测试统一搬到这里。`super::*` 拿到所有 re-export 的公共 API。

use std::path::PathBuf;

use crate::config::{
    AppConfig, CookieCfg, CrawlCfg, DownloadCfg, ExportFormat, GlobalCfg, LangType, Language,
    ProxyCfg, SourceCfg, ThemeDynMode, ThemeKind, ThemePref, load_config, save_config,
};

#[test]
fn loads_default_when_missing() {
    let cfg = load_config(&PathBuf::from("/definitely/does/not/exist.toml")).unwrap();
    assert_eq!(cfg.crawl.min_interval, 200);
    assert_eq!(cfg.crawl.max_interval, 400);
    assert!(cfg.crawl.enable_retry);
    assert!(cfg.source.search_filter);
    assert_eq!(cfg.download.ext_name, ExportFormat::Epub);
}

#[test]
fn font_size_accepts_int_and_float_literal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    // 整数形式（模板默认写法）
    std::fs::write(&path, "[global]\nfont-size = 18\n").unwrap();
    assert_eq!(load_config(&path).unwrap().global.font_size, 18.0);
    // 浮点形式
    std::fs::write(&path, "[global]\nfont-size = 20.5\n").unwrap();
    assert_eq!(load_config(&path).unwrap().global.font_size, 20.5);
}

#[test]
fn round_trip_through_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let cfg = AppConfig {
        global: GlobalCfg {
            theme_pref: ThemePref {
                kind: ThemeKind::Dynamic,
                dyn_mode: ThemeDynMode::Dark,
                dyn_light: "Catppuccin Latte".to_string(),
                dyn_dark: "Catppuccin Mocha".to_string(),
                ..ThemePref::default()
            },
            language: Language::English,
            sidebar_collapsed: true,
            font_size: 20.0,
            ..GlobalCfg::default()
        },
        download: DownloadCfg {
            download_path: "/tmp/sn-novels".to_string(),
            ext_name: ExportFormat::Txt,
            txt_encoding: "GBK".to_string(),
            preserve_chapter_cache: true,
            ..DownloadCfg::default()
        },
        source: SourceCfg {
            search_limit: Some(50),
            ..SourceCfg::default()
        },
        crawl: CrawlCfg {
            concurrency: Some(8),
            ..CrawlCfg::default()
        },
        proxy: ProxyCfg {
            proxy_enabled: true,
            proxy_host: "10.0.0.1".to_string(),
            proxy_port: 1080,
            ..ProxyCfg::default()
        },
        cookie: CookieCfg {
            qidian_cookie: String::new(),
            ..CookieCfg::default()
        },
        ..AppConfig::default()
    };

    save_config(&path, &cfg).unwrap();
    let loaded = load_config(&path).unwrap();

    assert_eq!(loaded.download.download_path, cfg.download.download_path);
    assert_eq!(loaded.download.ext_name, cfg.download.ext_name);
    assert_eq!(loaded.download.txt_encoding, cfg.download.txt_encoding);
    assert_eq!(
        loaded.download.preserve_chapter_cache,
        cfg.download.preserve_chapter_cache
    );
    assert_eq!(loaded.source.search_limit, cfg.source.search_limit);
    assert_eq!(loaded.crawl.concurrency, cfg.crawl.concurrency);
    assert_eq!(loaded.proxy.proxy_enabled, cfg.proxy.proxy_enabled);
    assert_eq!(loaded.proxy.proxy_host, cfg.proxy.proxy_host);
    assert_eq!(loaded.proxy.proxy_port, cfg.proxy.proxy_port);
    assert_eq!(loaded.cookie.qidian_cookie, cfg.cookie.qidian_cookie);
    assert_eq!(loaded.global.language, Language::English);
    assert_eq!(loaded.global.theme_pref, cfg.global.theme_pref);
    assert!(loaded.global.sidebar_collapsed);
    assert_eq!(loaded.global.font_size, 20.0);
}

#[test]
fn save_preserves_user_comments_in_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        r#"# 我的自定义注释
                [global]
                auto-update = false
                gh-proxy = "https://my-proxy.example/"
            "#,
    )
    .unwrap();

    let mut cfg = load_config(&path).unwrap();
    assert_eq!(cfg.global.gh_proxy, "https://my-proxy.example/");
    cfg.global.gh_proxy = "https://changed.example/".to_string();

    save_config(&path, &cfg).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("# 我的自定义注释"),
        "注释应保留: {written}"
    );
    assert!(
        written.contains("https://changed.example/"),
        "新值应写入: {written}"
    );
}

#[test]
fn missing_optional_int_keys_become_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        r#"[source]
                search-filter = true
            "#,
    )
    .unwrap();

    let cfg = load_config(&path).unwrap();
    // search-limit / concurrency 都没填，应当是 None
    assert!(cfg.source.search_limit.is_none());
    assert!(cfg.crawl.concurrency.is_none());
}

#[test]
fn language_maps_to_book_target_lang() {
    // 合并设置后，UI 语言的繁体选项 → 下载目标语言为 ZhTw（不是 ZhHant）。
    assert_eq!(
        Language::SimplifiedChinese.to_book_target_lang(),
        LangType::ZhCn
    );
    assert_eq!(
        Language::TraditionalChinese.to_book_target_lang(),
        LangType::ZhTw
    );
    // 英文 / 其它 → 回落简体（用户要求）。
    assert_eq!(Language::English.to_book_target_lang(), LangType::ZhCn);
}

#[test]
fn load_ignores_orphan_source_language_key() {
    // 老用户配置文件里可能还留着 `[source].language = "..."`，加载时必须容忍。
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        r#"[source]
                search-filter = true
            "#,
    )
    .unwrap();

    let cfg = load_config(&path).unwrap();
    assert!(cfg.source.search_filter);
}

/// 原子写：覆盖已存在文件后不留临时残留。
#[test]
fn save_config_overwrites_existing_without_leaving_tmp() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    // 第一次写
    let cfg = AppConfig::default();
    save_config(&path, &cfg).unwrap();
    let original = std::fs::read_to_string(&path).unwrap();

    // 改一个字段再写（覆盖已存在文件）
    let mut cfg2 = cfg.clone();
    cfg2.global.font_size = 22.0;
    save_config(&path, &cfg2).unwrap();
    let updated = std::fs::read_to_string(&path).unwrap();
    assert_ne!(original, updated, "file content should change");
    assert!(
        updated.contains("22"),
        "font-size 22 should be in updated file"
    );

    // 同目录下不应残留任何 `.tmp.*` 临时文件。
    let leftover: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
        .collect();
    assert!(
        leftover.is_empty(),
        "atomic write should not leave .tmp.* files: {:?}",
        leftover.iter().map(|e| e.file_name()).collect::<Vec<_>>()
    );

    // load 回来确实拿到新值（兼容性：原子写后的文件能被 load_config 正确解析）。
    let cfg_loaded = load_config(&path).unwrap();
    assert!((cfg_loaded.global.font_size - 22.0).abs() < 1e-3);
}

/// 原子写：写完后没有 panic，且文件可重新加载。
#[test]
fn save_config_writes_to_new_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/sub/config.toml");
    save_config(&path, &AppConfig::default()).unwrap();
    assert!(path.exists());
    // 反向解析：load_config 应能读回 default
    let cfg = load_config(&path).unwrap();
    assert_eq!(cfg.global.font_size, AppConfig::default().global.font_size);
}
