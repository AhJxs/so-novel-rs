//! 搜索相关业务方法：spawn_search / select_search_result。

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::config::AppConfig;
use crate::http::HttpClients;
use crate::models::{Book, Rule};
use crate::rules::Source;

use super::super::search_state::{DetailEvent, DetailState, SourceSearchEvent, SourceStatus};

/// 派聚合搜索任务。返回是否成功派发。
pub fn spawn_search(
    rules: &[Rule],
    config: &AppConfig,
    http: Arc<HttpClients>,
    runtime: &tokio::runtime::Runtime,
    search: &mut super::super::search_state::SearchState,
) -> bool {
    let keyword = search.keyword.trim().to_string();
    if keyword.is_empty() {
        search.last_error = Some("请输入关键词".to_string());
        return false;
    }
    if search.running {
        return false;
    }

    search.last_error = None;
    search.last_keyword = Some(keyword.clone());
    search.results.clear();
    search.source_status.clear();
    search.received = 0;
    search.selected = None;

    let target_sources: Vec<Source> = if let Some(id) = search.source_id {
        rules
            .iter()
            .filter(|r| r.id == id)
            .cloned()
            .map(|r| Source::from(r, config))
            .collect()
    } else {
        rules
            .iter()
            .filter(|r| r.is_search_enabled())
            .cloned()
            .map(|r| Source::from(r, config))
            .collect()
    };

    if target_sources.is_empty() {
        search.last_error = Some("没有可用的书源（请在 [书源管理] 检查规则文件）".to_string());
        if let Some(id) = search.source_id {
            tracing::warn!(keyword = %keyword, source_id = id, "搜索派发失败：选中的书源被禁用或已删除");
        } else {
            tracing::warn!(keyword = %keyword, "搜索派发失败：无可用书源（全部被禁用或无规则）");
        }
        return false;
    }

    search.source_status = target_sources
        .iter()
        .map(|s| (s.rule.id, s.rule.name.clone(), SourceStatus::Pending))
        .collect();
    search.expected = target_sources.len();
    search.running = true;
    search.filter_after_done = config.search_filter;

    let (tx, rx) = mpsc::unbounded_channel::<SourceSearchEvent>();
    search.rx = Some(rx);

    let cfg = config.clone();
    let cf_bypass = if config.cf_bypass.trim().is_empty() {
        None
    } else {
        Some(config.cf_bypass.clone())
    };
    let limit = config
        .search_limit
        .map(|v| v.max(0) as usize)
        .filter(|v| *v > 0);

    let target_ids: Vec<i32> = target_sources.iter().map(|s| s.rule.id).collect();
    tracing::info!(
        source_ids = ?target_ids,
        source_count = target_sources.len(),
        source_id = ?search.source_id,
        limit = ?limit,
        cf_bypass = cf_bypass.is_some(),
        "搜索派发: 关键词={:?}, 目标源 {} 个",
        keyword,
        target_sources.len(),
    );

    runtime.spawn(async move {
        let (inner_tx, mut inner_rx) =
            mpsc::unbounded_channel::<crate::crawler::search::SourceSearchOutcome>();

        // 把搜索放在独立的 tokio task 里，与下面的桥接循环并发运行。
        // async move 会把 cfg 移入内部，&cfg 引用的是 task 自己拥有的值，
        // 满足 'static 要求。
        // 在 target_sources move 进 search_streaming 前，建好 id→name 映射，
        // 用于桥接循环结束后给"未出结果的源"补失败事件。
        let source_names: std::collections::HashMap<i32, String> = target_sources
            .iter()
            .map(|s| (s.rule.id, s.rule.name.clone()))
            .collect();

        let http = Arc::clone(&http);
        let search_handle = tokio::spawn(async move {
            crate::crawler::search::search_streaming(
                &cfg,
                http,
                target_sources,
                keyword,
                limit,
                cf_bypass,
                inner_tx,
            )
            .await
        });

        // 桥接循环：每收到一源结果就立即转发给 UI，与搜索并发。
        let mut seen_ids: std::collections::HashSet<i32> = std::collections::HashSet::new();
        while let Some(o) = inner_rx.recv().await {
            seen_ids.insert(o.source_id);
            let send_result = match o.result {
                Ok(list) => Ok(list),
                Err(e) => Err(format!("{e:#}")),
            };
            if tx
                .send(SourceSearchEvent {
                    source_id: o.source_id,
                    source_name: o.source_name,
                    result: send_result,
                })
                .is_err()
            {
                break;
            }
        }

        // 通道关闭后，仍有源未出结果（task panic / 提前退出）→ 给它们补一条失败事件，
        // 否则该源在 UI 永远停在 Pending，且 received 永远到不了 expected（搜索卡死）。
        // source_names 在 target_sources move 进 search_streaming 前已建好，这里直接用它。
        let mut missing = 0usize;
        if !tx.is_closed() {
            for (id, name) in &source_names {
                if !seen_ids.contains(id) {
                    missing += 1;
                    let _ = tx.send(SourceSearchEvent {
                        source_id: *id,
                        source_name: name.clone(),
                        result: Err("后台任务异常退出".to_string()),
                    });
                }
            }
        }

        if let Err(e) = search_handle.await {
            tracing::warn!("search task panicked: {e}");
        }

        tracing::debug!(
            received = seen_ids.len(),
            missing = missing,
            "搜索桥接循环结束"
        );
    });

    true
}

/// 选中某条搜索结果；如果之前没拉过详情就 spawn 一次。
pub fn select_search_result(
    rules: &[Rule],
    config: &AppConfig,
    http: Arc<HttpClients>,
    runtime: &tokio::runtime::Runtime,
    search: &mut super::super::search_state::SearchState,
    idx: usize,
) {
    if idx >= search.results.len() {
        return;
    }
    search.selected = Some(idx);

    let r = &search.results[idx];
    let key = (r.source_id, r.url.clone());
    if search.detail_cache.contains_key(&key) {
        return;
    }

    let Some(rule) = rules.iter().find(|x| x.id == r.source_id).cloned() else {
        search.detail_cache.insert(
            key,
            DetailState::Failed(format!("找不到 ID 为 {} 的书源规则", r.source_id)),
        );
        return;
    };

    search
        .detail_cache
        .insert(key.clone(), DetailState::Pending);

    let tx = match &search.detail_rx {
        Some(_) => {
            let (tx, rx) = mpsc::unbounded_channel();
            search.detail_rx = Some(rx);
            tx
        }
        None => {
            let (tx, rx) = mpsc::unbounded_channel();
            search.detail_rx = Some(rx);
            tx
        }
    };

    let url = r.url.clone();
    let source_id = r.source_id;
    let cf_bypass = if config.cf_bypass.trim().is_empty() {
        None
    } else {
        Some(config.cf_bypass.clone())
    };
    let qidian_cookie = if config.qidian_cookie.trim().is_empty() {
        None
    } else {
        Some(config.qidian_cookie.clone())
    };

    runtime.spawn(async move {
        let url_for_event = url.clone();
        let cf = cf_bypass.clone();
        let qc = qidian_cookie.clone();
        let result: Result<Book, String> = async {
            let client = http.for_rule(&rule);
            crate::parser::parse_book_detail(client, &rule, &url, cf.as_deref(), qc.as_deref())
                .await
                .map_err(|e| format!("{e:#}"))
        }
        .await;

        let state = match result {
            Ok(book) => DetailState::Loaded(Box::new(book)),
            Err(e) => DetailState::Failed(e),
        };
        let _ = tx.send(DetailEvent {
            source_id,
            url: url_for_event,
            state,
        });
    });
}
