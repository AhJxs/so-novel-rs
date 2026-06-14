//! 聚合搜索。对应 Java `action.AggregatedSearchAction`。
//!
//! 在多个书源上并发执行 `search_one`，把结果合并成一个列表。
//! 不做相似度过滤 / 排序 — 那属于阶段 5（参考 Java `SearchResultsHandler`）。
//!
//! 由于 parser 是同步的，这里用 `tokio::task::spawn_blocking` 并发分发，
//! 与 `download_book` 的 spawn_blocking 模式一致。

use std::sync::Arc;
use std::time::Duration;

use reqwest::blocking::Client;
use tokio::task::JoinSet;

use crate::http::client::{build_blocking_client, ClientOptions};
use crate::models::SearchResult;
use crate::parser::{search_one, SearchError};
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
///
/// **注意**：此函数会**为每个源单独构造一个 reqwest Client**，因为不同书源
/// 可能有不同的 `ignore_ssl` 设置（rate-limit.json 里 0xs 书源就是这样）。
/// 客户端构造很轻（不发请求），代价可接受。
pub async fn search_aggregated(
    cfg: &crate::config::AppConfig,
    sources: Vec<Source>,
    keyword: String,
    limit: Option<usize>,
    cf_bypass_base: Option<String>,
) -> Vec<SourceSearchOutcome> {
    let mut set: JoinSet<SourceSearchOutcome> = JoinSet::new();

    let cfg = Arc::new(cfg.clone());
    let kw = Arc::new(keyword);
    let cf = Arc::new(cf_bypass_base);

    for src in sources {
        let cfg = Arc::clone(&cfg);
        let kw = Arc::clone(&kw);
        let cf = Arc::clone(&cf);
        set.spawn(async move {
            let source_id = src.rule.id;
            let source_name = src.rule.name.clone();
            let result = tokio::task::spawn_blocking(move || {
                let client_opts = ClientOptions {
                    unsafe_ssl: src.rule.ignore_ssl,
                };
                let client = match build_blocking_client(&cfg, &client_opts) {
                    Ok(c) => c,
                    Err(e) => {
                        return Err(SearchError::Http(format!("client: {e:#}")));
                    }
                };
                let cf_borrow: Option<&str> = cf.as_ref().as_ref().map(|s| s.as_str());
                run_one(&client, &src, kw.as_str(), limit, cf_borrow)
            })
            .await
            .unwrap_or_else(|join_err| {
                Err(SearchError::Http(format!("spawn_blocking: {join_err}")))
            });
            SourceSearchOutcome {
                source_id,
                source_name,
                result,
            }
        });
    }

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

fn run_one(
    client: &Client,
    source: &Source,
    keyword: &str,
    limit: Option<usize>,
    cf_bypass_base: Option<&str>,
) -> Result<Vec<SearchResult>, SearchError> {
    // 给单源加一个保险超时（独立于 reqwest 自身），防止某个慢源拖死整个聚合搜索。
    // 这里通过 client 自己的 timeout（已在 build_blocking_client 设置 10s）+ 上层
    // 用户可见的进度显式取消即可；不再叠加 tokio timeout（spawn_blocking 不支持 race）。
    let _ = Duration::from_secs(0); // 占位防止 unused import 警告，避免改动 use 行
    search_one(client, &source.rule, keyword, limit, cf_bypass_base)
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
        let outcomes = search_aggregated(&cfg, Vec::new(), "any".into(), Some(10), None).await;
        assert!(outcomes.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn each_source_yields_one_outcome_with_typed_error() {
        // 没有 search 段的规则会立即返回 SearchDisabled，不发起任何网络请求。
        let cfg = AppConfig::default();
        let sources = vec![make_source(1, "A"), make_source(2, "B")];
        let outcomes = search_aggregated(&cfg, sources, "any".into(), Some(10), None).await;
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
