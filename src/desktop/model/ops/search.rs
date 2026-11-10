//! 搜索相关业务方法: `spawn_search` / `select_search_result`。

use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::Instrument;

use super::super::events::WakeupHandle;
use super::super::search_state::{DetailEvent, DetailState, SourceSearchEvent, SourceStatus};
use super::super::trace::{TraceId, sub};
use crate::config::AppConfig;
use crate::core::{config_helpers, search as core_search};
use crate::http::HttpClients;
use crate::models::{Book, Rule};

/// 派聚合搜索任务。返回是否成功派发。
pub fn spawn_search(
    rules: &[Arc<Rule>],
    config: &AppConfig,
    http: &HttpClients,
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

    // 桥接循环结束后要给未出结果的源补失败事件，必须先于 `target_sources` 被 move 建好映射。
    let source_names: std::collections::HashMap<i32, String> = target_sources
        .iter()
        .map(|s| (s.rule.id, s.rule.name.clone()))
        .collect();

    // `JoinSet::spawn` 需要 Tokio 上下文；模型方法本身不在 async 任务里, 短暂 enter。
    // 每个搜索任务的 `reqwest::Client` 也在此刻取出并移入任务 —— 任务不借用 `HttpClients`。
    let set = {
        let _guard = runtime.enter();
        crate::crawler::search::spawn_search_tasks(http, target_sources, keyword, limit, cf_bypass)
    };

    runtime.spawn(
        async move {
            let mut set = set;
            let mut seen_ids: std::collections::HashSet<i32> = std::collections::HashSet::new();
            // 每完成一源立即转发给 UI；顺序不保证，UI 按 `source_id` 关联。
            while let Some(joined) = set.join_next().await {
                match joined {
                    Ok(o) => {
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
                            // UI 侧已 drop 接收端 → 放弃剩余任务（JoinSet drop 会 abort）。
                            break;
                        }
                    }
                    Err(e) => tracing::warn!("聚合搜索任务 join 失败: {e}"),
                }
            }

            // 仍有源未出结果（task panic / 提前退出）→ 补一条失败事件,
            // 否则该源停在 Pending, `received` 到不了 `expected`, 搜索卡死。
            let mut missing = 0usize;
            if !tx.is_closed() {
                for (id, name) in &source_names {
                    if !seen_ids.contains(id) {
                        missing += 1;
                        let _ = tx.send(SourceSearchEvent {
                            source_id: *id,
                            source_name: name.clone(),
                            result: Err(crate::error::AppError::internal(
                                "后台任务异常退出".to_string(),
                            )),
                        });
                        wakeup.notify();
                    }
                }
            }

            // 终止日志: 每次搜索恰好一行 trace_id=N 的尾记录。
            tracing::info!(received = seen_ids.len(), missing = missing, "搜索聚合完毕");
        }
        .instrument(span),
    );

    true
}

/// 选中某条搜索结果；如果之前没拉过详情就 spawn 一次。
pub fn select_search_result(
    rules: &[Arc<Rule>],
    config: &AppConfig,
    http: &HttpClients,
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
    // 任务不借用 HttpClients：client 在 spawn 前取出。
    let client = http.for_rule(&rule);

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
