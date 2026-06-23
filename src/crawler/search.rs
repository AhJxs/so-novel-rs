//! 聚合搜索。对应 Java `action.AggregatedSearchAction`。
//!
//! 在多个书源上并发执行 `search_one`，把结果合并成一个列表。
//! 本层只负责聚合；相似度过滤 / 排序由调用方在结果聚合后调
//! `crate::parser::filter_sort`（对应 Java `SearchResultsHandler`）。
//!
//! parser 是 async 的（基于 `reqwest::Client`），这里直接 spawn async task，
//! 不再走 spawn_blocking。

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::http::HttpClients;
use crate::models::SearchResult;
use crate::parser::{SearchError, search_one};
use crate::rules::Source;

/// 单源搜索的输出条目（绑定到原 Source，便于 UI 出错提示）。
#[derive(Debug)]
pub struct SourceSearchOutcome {
    pub source_id: i32,
    pub source_name: String,
    pub result: Result<Vec<SearchResult>, SearchError>,
}

/// 在所有给定 sources 上并发执行搜索，返回每源的结果。
///
/// `cf_bypass_base` 与各 parser 中的同名参数一致：CF 命中时若非空则
/// 调用外部 bypass 服务。
#[tracing::instrument(skip_all, fields(sources = sources.len(), keyword = %crate::util::fs::truncate_log(&keyword, 10)))]
pub async fn search_aggregated(
    http: Arc<HttpClients>,
    sources: Vec<Source>,
    keyword: String,
    limit: Option<usize>,
    cf_bypass_base: Option<String>,
) -> Vec<SourceSearchOutcome> {
    let mut set = spawn_search_tasks(http, sources, keyword, limit, cf_bypass_base);

    let mut out = Vec::with_capacity(set.len());
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(o) => out.push(o),
            Err(e) => tracing::warn!("聚合搜索任务 join 失败: {e}"),
        }
    }
    // 按 source_id 升序，UI 显示稳定
    out.sort_by_key(|o| o.source_id);
    out
}

/// 流式聚合搜索：每源完成后立即通过 `tx` 推送，而不是等全部完成再返回。
///
/// 与 `search_aggregated` 的区别：
/// - `search_aggregated` 收集所有结果到 Vec，适合测试和一次性批量处理。
/// - `search_streaming` 每完成一源就推送，适合 UI 逐源更新进度。
#[tracing::instrument(skip_all, fields(sources = sources.len(), keyword = keyword))]
pub async fn search_streaming(
    http: Arc<HttpClients>,
    sources: Vec<Source>,
    keyword: String,
    limit: Option<usize>,
    cf_bypass_base: Option<String>,
    tx: mpsc::UnboundedSender<SourceSearchOutcome>,
) {
    let mut set = spawn_search_tasks(http, sources, keyword, limit, cf_bypass_base);

    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(o) => {
                // 接收端已关闭（UI 侧 drop）→ 放弃剩余任务
                if tx.send(o).is_err() {
                    break;
                }
            }
            Err(e) => tracing::warn!("聚合搜索任务 join 失败: {e}"),
        }
    }
}

/// 为每个 source spawn 一个异步搜索任务，返回 `JoinSet`。
///
/// 调用方决定如何消费结果（收集到 Vec 或逐个推送 channel）。
/// 共享逻辑：日志、Client 复用、per-source 计时、结果包装。
fn spawn_search_tasks(
    http: Arc<HttpClients>,
    sources: Vec<Source>,
    keyword: String,
    limit: Option<usize>,
    cf_bypass_base: Option<String>,
) -> JoinSet<SourceSearchOutcome> {
    let mut set: JoinSet<SourceSearchOutcome> = JoinSet::new();

    let http = Arc::clone(&http);
    let kw = Arc::new(keyword);
    let cf = Arc::new(cf_bypass_base);

    for src in sources {
        let http = Arc::clone(&http);
        let kw = Arc::clone(&kw);
        let cf = Arc::clone(&cf);
        set.spawn(async move {
            let source_id = src.rule.id;
            let source_name = src.rule.name.clone();
            let client = http.for_rule(&src.rule);
            let cf_borrow: Option<&str> = cf.as_ref().as_ref().map(|s| s.as_str());
            let result = search_one(&client, &src.rule, kw.as_str(), limit, cf_borrow).await;
            SourceSearchOutcome {
                source_id,
                source_name,
                result,
            }
        });
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::models::Rule;

    fn make_source(id: i32, name: &str) -> Source {
        let rule = Rule {
            id,
            url: format!("https://demo{id}.test/"),
            name: name.into(),
            ..Rule::default()
        };
        let cfg = AppConfig::default();
        Source::from(rule, &cfg)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_sources_returns_empty() {
        let cfg = AppConfig::default();
        let http = Arc::new(crate::http::HttpClients::new(&cfg).unwrap());
        let outcomes = search_aggregated(http, Vec::new(), "any".into(), Some(10), None).await;
        assert!(outcomes.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn each_source_yields_one_outcome_with_typed_error() {
        // 没有 search 段的规则会立即返回 SearchDisabled，不发起任何网络请求。
        let cfg = AppConfig::default();
        let http = Arc::new(crate::http::HttpClients::new(&cfg).unwrap());
        let sources = vec![make_source(1, "A"), make_source(2, "B")];
        let outcomes = search_aggregated(http, sources, "any".into(), Some(10), None).await;
        assert_eq!(outcomes.len(), 2);
        assert!(matches!(
            outcomes[0].result,
            Err(SearchError::SearchDisabled)
        ));
        assert!(matches!(
            outcomes[1].result,
            Err(SearchError::SearchDisabled)
        ));
        // 顺序按 source_id
        assert_eq!(outcomes[0].source_id, 1);
        assert_eq!(outcomes[1].source_id, 2);
    }
}
