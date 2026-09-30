//! 搜索相关业务方法: `spawn_search` / `select_search_result`。

use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::Instrument;

use crate::config::AppConfig;
use crate::core::{config_helpers, search as core_search};
use crate::http::HttpClients;
use crate::models::{Book, Rule};

use super::super::events::WakeupHandle;
use super::super::search_state::{DetailEvent, DetailState, SourceSearchEvent, SourceStatus};
use super::super::trace::{TraceId, sub};

/// 派聚合搜索任务。返回是否成功派发。
pub fn spawn_search(
    rules: &[Rule],
    config: &AppConfig,
    http: Arc<HttpClients>,
    runtime: &tokio::runtime::Runtime,
    wakeup: &WakeupHandle,
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

    let target_sources = core_search::select_sources(rules, config, search.source_id);

    if target_sources.is_empty() {
        search.last_error = Some("没有可用的书源（请在 [书源管理] 检查规则文件）".to_string());
        if let Some(id) = search.source_id {
            tracing::warn!(keyword = %crate::utils::fs::truncate_log(&keyword, 10), source_id = id, "搜索派发失败：选中的书源被禁用或已删除");
        } else {
            tracing::warn!(keyword = %crate::utils::fs::truncate_log(&keyword, 10), "搜索派发失败：无可用书源（全部被禁用或无规则）");
        }
        return false;
    }

    search.source_status = target_sources
        .iter()
        .map(|s| (s.rule.id, s.rule.name.clone(), SourceStatus::Pending))
        .collect();
    search.expected = target_sources.len();
    search.running = true;
    search.filter_after_done = config.source.search_filter;

    let (tx, rx) = mpsc::unbounded_channel::<SourceSearchEvent>();
    search.rx = Some(rx);

    let cf_bypass = config_helpers::cf_bypass(config);
    let limit = core_search::effective_limit(None, config);

    // 顶层 trace_id: 在所有 spawn 之前 mint, 经 `info_span!` 跨 .await 传播。
    let wakeup = wakeup.clone();

    let trace_id = TraceId::mint();
    let span = tracing::info_span!(
        sub::SEARCH,
        trace_id = %trace_id,
        keyword = %crate::utils::fs::truncate_log(&keyword, 10),
        sources = target_sources.len(),
        single_source = search.source_id.is_some(),
    );

    // 内部 `async move` 会 move 走一份 span，.instrument 还要一份。
    let span_for_spawn = span.clone();
    runtime.spawn(
        async move {
            let (inner_tx, mut inner_rx) =
                mpsc::unbounded_channel::<crate::crawler::search::SourceSearchOutcome>();

            // 搜索跑在独立 tokio task 里, 与下面的桥接循环并发。在 target_sources 被
            // move 走之前建好 id→name 映射, 供桥接循环结束后补失败事件。
            let source_names: std::collections::HashMap<i32, String> = target_sources
                .iter()
                .map(|s| (s.rule.id, s.rule.name.clone()))
                .collect();

            let http = Arc::clone(&http);
            // 必须 `.instrument(span)` 让 search_streaming 跑在 search span 的子上下文里,
            // 否则 crawler 内部的 `tracing::*!` 拿不到 trace_id。
            let search_handle = tokio::spawn(
                async move {
                    crate::crawler::search::search_streaming(
                        http,
                        target_sources,
                        keyword,
                        limit,
                        cf_bypass,
                        inner_tx,
                    )
                    .await;
                }
                .instrument(span),
            );

            // 桥接循环: 每收到一源结果立即转发给 UI, 与搜索并发
            // （`runtime.spawn` 上已挂 span, 内部 tracing 自动带 trace_id）。
            let mut seen_ids: std::collections::HashSet<i32> = std::collections::HashSet::new();
            while let Some(o) = inner_rx.recv().await {
                seen_ids.insert(o.source_id);
                let send_result = match o.result {
                    Ok(list) => Ok(list),
                    Err(e) => Err(crate::error::AppError::from(e)),
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

            // 通道关闭后仍有源未出结果（task panic / 提前退出）→ 补一条失败事件,
            // 否则该源停在 Pending, `received` 到不了 `expected`, 搜索卡死。
            let mut missing = 0usize;
            if !tx.is_closed() {
                for (id, name) in &source_names {
                    if !seen_ids.contains(id) {
                        missing += 1;
                        let _ = tx.send(SourceSearchEvent {
                            source_id: *id,
                            source_name: name.clone(),
                            result: Err(crate::error::AppError::internal("后台任务异常退出")),
                        });
                        wakeup.notify();
                    }
                }
            }

            if let Err(e) = search_handle.await {
                tracing::warn!("search task panicked: {e}");
            }
            // 终止日志: 每次搜索恰好一行 trace_id=N 的尾记录。
            tracing::info!(received = seen_ids.len(), missing = missing, "搜索聚合完毕");
        }
        .instrument(span_for_spawn),
    );

    true
}

/// 选中某条搜索结果；如果之前没拉过详情就 spawn 一次。
pub fn select_search_result(
    rules: &[Rule],
    config: &AppConfig,
    http: Arc<HttpClients>,
    runtime: &tokio::runtime::Runtime,
    wakeup: &WakeupHandle,
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

    search.detail_cache.insert(key, DetailState::Pending);

    let (tx, rx) = mpsc::unbounded_channel();
    search.detail_rx = Some(rx);

    let url = r.url.clone();
    let source_id = r.source_id;
    let cf_bypass = config_helpers::cf_bypass(config);
    let qidian_cookie = config_helpers::qidian_cookie(config);

    // 详情拉取单独 mint 一个 trace_id —— 它是一次独立动作, 不与搜索共享,
    // 方便 grep `trace_id=N` 时只看这一次详情拉取。
    let wakeup = wakeup.clone();

    let trace_id = TraceId::mint();
    let span = tracing::info_span!(
        sub::DETAIL,
        trace_id = %trace_id,
        source_id = source_id,
        %url,
    );
    // 用 `.instrument(span)` 包住整个 future 再 spawn: `tracing::Span` 是 `!Send`,
    // 但 Instrumented future 仍是 `Send`, 可以穿过 spawn 边界。
    let span_for_task = span;
    let task = async move {
        let url_for_event = url.clone();
        let cf = cf_bypass.clone();
        let qc = qidian_cookie.clone();
        let client = http.for_rule(&rule);
        let result: Result<Book, String> =
            crate::parser::parse_book_detail(&client, &rule, &url, cf.as_deref(), qc.as_deref())
                .await
                .map_err(|e| format!("{e:#}"));

        let state = match result {
            Ok(book) => {
                tracing::info!(book = %book.book_name, "详情拉取成功");
                DetailState::Loaded(Box::new(book))
            }
            Err(e) => {
                tracing::warn!(error = %e, "详情拉取失败");
                DetailState::Failed(e)
            }
        };
        let _ = tx.send(DetailEvent {
            source_id,
            url: url_for_event,
            state,
        });
        wakeup.notify();
    }
    .instrument(span_for_task);
    runtime.spawn(task);
}
