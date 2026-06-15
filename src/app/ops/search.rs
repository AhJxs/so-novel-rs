//! 搜索相关业务方法：spawn_search / select_search_result。

use tokio::sync::mpsc;

use crate::config::AppConfig;
use crate::models::Rule;

use super::super::search_state::{DetailEvent, DetailState, SourceSearchEvent, SourceStatus};

/// 派聚合搜索任务。返回是否成功派发。
pub fn spawn_search(
    rules: &[Rule],
    config: &AppConfig,
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
    search.pending_scroll_top = true;
    search.selected = None;
    search.detail_popup_for = None;

    let target_sources: Vec<crate::rules::Source> = if let Some(id) = search.source_id {
        rules
            .iter()
            .filter(|r| r.id == id)
            .cloned()
            .map(|r| crate::rules::Source::from(r, config))
            .collect()
    } else {
        rules
            .iter()
            .filter(|r| !r.disabled && r.search.as_ref().map(|s| !s.disabled).unwrap_or(false))
            .cloned()
            .map(|r| crate::rules::Source::from(r, config))
            .collect()
    };

    if target_sources.is_empty() {
        search.last_error = Some("没有可用的书源（请在 [书源管理] 检查规则文件）".to_string());
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

    runtime.spawn(async move {
        let (inner_tx, mut inner_rx) =
            mpsc::unbounded_channel::<crate::crawler::search::SourceSearchOutcome>();

        // 把搜索放在独立的 tokio task 里，与下面的桥接循环并发运行。
        // async move 会把 cfg 移入内部，&cfg 引用的是 task 自己拥有的值，
        // 满足 'static 要求。
        let search_handle = tokio::spawn(async move {
            crate::crawler::search::search_streaming(
                &cfg,
                target_sources,
                keyword,
                limit,
                cf_bypass,
                inner_tx,
            )
            .await
        });

        // 桥接循环：每收到一源结果就立即转发给 UI，与搜索并发。
        while let Some(o) = inner_rx.recv().await {
            let send_result = match o.result {
                Ok(list) => Ok(list),
                Err(e) => Err(format!("{e}")),
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

        let _ = search_handle.await;
    });

    true
}

/// 选中某条搜索结果；如果之前没拉过详情就 spawn 一次。
pub fn select_search_result(
    rules: &[Rule],
    config: &AppConfig,
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

    let cfg = config.clone();
    let url = r.url.clone();
    let source_id = r.source_id;
    let cf_bypass = if config.cf_bypass.trim().is_empty() {
        None
    } else {
        Some(config.cf_bypass.clone())
    };

    runtime.spawn(async move {
        let url_for_event = url.clone();
        let cf = cf_bypass.clone();
        let result: Result<crate::models::Book, String> = async {
            let opts = crate::http::client::ClientOptions {
                unsafe_ssl: rule.ignore_ssl,
            };
            let client = crate::http::client::build_async_client(&cfg, &opts)
                .map_err(|e| format!("client: {e:#}"))?;
            crate::parser::parse_book_detail(&client, &rule, &url, cf.as_deref())
                .await
                .map_err(|e| format!("{e}"))
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
