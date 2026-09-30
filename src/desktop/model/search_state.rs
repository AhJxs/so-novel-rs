use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use lru::LruCache;

use crate::core::{DrainOutcome, try_drain_all};
use crate::models::{Book, Chapter, SearchResult};

use super::cover::{CoverEntry, cover_entry_from_bytes};

/// 封面结果缓存最大条目数。64 条 ≈ 5-10MB, 让长会话的内存有界
/// （旧实现是无上限 `HashMap`, 会累积所有查看过的封面）。
const COVER_CACHE_CAPACITY: NonZeroUsize = match NonZeroUsize::new(64) {
    Some(n) => n,
    None => unreachable!(),
};

/// TOC 预取加载状态。
#[derive(Debug, Clone)]
pub enum TocState {
    Pending,
    Loaded(Box<Book>, Vec<Chapter>),
    Failed(String),
}

/// TOC 预取后台 → UI 通道事件。
#[derive(Debug)]
pub struct TocEvent {
    pub source_id: i32,
    pub url: String,
    pub state: TocState,
}

/// 搜索页状态（含后台通道）。
pub struct SearchState {
    pub keyword: String,
    /// `None` = 聚合搜索；`Some(rule_id)` = 仅当前书源。
    pub source_id: Option<i32>,

    pub last_keyword: Option<String>,
    pub results: Vec<SearchResult>,
    pub results_version: u64,
    /// 各源搜索状态 (true = 跑完), 让 UI 显示哪个源还在等。
    pub source_status: Vec<(i32, String, SourceStatus)>,
    pub running: bool,
    pub last_error: Option<String>,

    /// 后台搜索通过此通道汇报"单源完成"。
    pub rx: Option<mpsc::UnboundedReceiver<SourceSearchEvent>>,
    pub expected: usize,
    pub received: usize,

    pub selected: Option<usize>,
    pub detail_cache: HashMap<(i32, String), DetailState>,
    pub detail_rx: Option<mpsc::UnboundedReceiver<DetailEvent>>,

    pub filter_after_done: bool,

    pub toc_cache: HashMap<(i32, String), TocState>,
    pub toc_rx: Option<mpsc::UnboundedReceiver<TocEvent>>,
    pub chapter_range_start: u32,
    pub chapter_range_end: u32,

    pub cover_tx: Option<mpsc::UnboundedSender<CoverEvent>>,
    pub cover_rx: Option<mpsc::UnboundedReceiver<CoverEvent>>,
    /// 封面结果缓存; 上限 `COVER_CACHE_CAPACITY`, 切 active rule 时整表 `clear()`。
    pub cover_cache: LruCache<(i32, String), CoverEntry>,
    /// 正在下载中的封面 URL; 防止重复 spawn。
    pub cover_in_flight: HashSet<(i32, String)>,
    /// `drain_detail` 期间收集的待 prefetch 封面 URL, 之后由 `AppModel` 统一派发。
    pub pending_cover_prefetch: Vec<(i32, String)>,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            keyword: String::new(),
            source_id: None,
            last_keyword: None,
            results: Vec::new(),
            results_version: 0,
            source_status: Vec::new(),
            running: false,
            last_error: None,
            rx: None,
            expected: 0,
            received: 0,
            selected: None,
            detail_cache: HashMap::new(),
            detail_rx: None,
            filter_after_done: false,
            toc_cache: HashMap::new(),
            toc_rx: None,
            chapter_range_start: 0,
            chapter_range_end: 0,
            cover_tx: None,
            cover_rx: None,
            cover_cache: LruCache::new(COVER_CACHE_CAPACITY),
            cover_in_flight: HashSet::new(),
            pending_cover_prefetch: Vec::new(),
        }
    }
}

/// 详情面板加载状态。
#[derive(Debug, Clone)]
pub enum DetailState {
    Pending,
    Loaded(Box<Book>),
    Failed(String),
}

impl DetailState {
    /// 仅当 Loaded 状态可取书；Pending/Failed 返回 None。
    pub fn book(&self) -> Option<&Book> {
        match self {
            Self::Loaded(b) => Some(b),
            _ => None,
        }
    }
}

/// 详情后台 → UI 通道事件。
#[derive(Debug)]
pub struct DetailEvent {
    pub source_id: i32,
    pub url: String,
    pub state: DetailState,
}

/// 封面下载完成事件。后台 HTTP 下载 → UI 构造 `CoverEntry`。
#[derive(Debug)]
pub struct CoverEvent {
    pub source_id: i32,
    pub url: String,
    /// 下载成功：Some(bytes)；失败：None。
    pub bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub enum SourceStatus {
    Pending,
    Ok(usize),
    /// 错误简短文案
    Err(String),
}

/// 后台聚合搜索向 UI 推送的事件（每源 1 条）。
#[derive(Debug)]
pub struct SourceSearchEvent {
    pub source_id: i32,
    pub source_name: String,
    /// 单源搜索结果。`AppError` 承载 [`crate::parser::SearchError`] / 任务异常退出;
    /// 调用方用 `e.message()` 拿 i18n 渲染文本。
    pub result: crate::error::AppResult<Vec<SearchResult>>,
}

impl SearchState {
    /// rule 集合整体变化时（切活跃书源文件 / 导入触发 active 重载）调用。
    /// `source_id: i32` 是数值弱匹配 —— 同一 id 在新文件里可能指向完全不同的源,
    /// 留着旧 results 会让用户点到错源下载, 所以整体退到 `Default`; `keyword` 必须保留。
    pub fn clear_results_and_caches(&mut self) {
        let keyword = std::mem::take(&mut self.keyword);
        *self = Self::default();
        self.keyword = keyword;
    }

    /// 排空通道；返回是否有事件（触发 repaint）。
    pub fn drain(&mut self) -> bool {
        let mut any = false;
        let _ = try_drain_all(&mut self.rx, |ev: SourceSearchEvent| {
            any = true;
            self.received += 1;
            let status = match ev.result {
                Ok(list) => {
                    let n = list.len();
                    self.results.extend(list);
                    SourceStatus::Ok(n)
                }
                Err(e) => {
                    let line = e.message();
                    let truncated: String = line.chars().take(60).collect();
                    SourceStatus::Err(truncated)
                }
            };
            if let Some(slot) = self
                .source_status
                .iter_mut()
                .find(|(id, _, _)| *id == ev.source_id)
            {
                slot.2 = status;
            } else {
                self.source_status
                    .push((ev.source_id, ev.source_name, status));
            }
        });
        if self.received >= self.expected && self.expected > 0 {
            self.running = false;
            self.rx = None;
            if self.filter_after_done
                && let Some(kw) = self.last_keyword.as_deref()
            {
                let new_results = crate::parser::filter_sort(&self.results, kw);
                self.selected = None;
                self.results = new_results;
            }
        }

        let detail_changed = self.drain_detail();
        let cover_changed = self.drain_cover();
        let toc_changed = self.drain_toc();
        any |= detail_changed || cover_changed || toc_changed;

        // 搜索结果 / 详情 / 封面 / TOC 有事件时 bump 一档，让含 `results_version`
        // 的缓存 key 失效（一次 bump 即够，无需按变化次数计数）。
        if any {
            self.results_version = self.results_version.wrapping_add(1);
        }
        any
    }

    /// 排空详情后台通道。
    fn drain_detail(&mut self) -> bool {
        let mut any = false;
        let _ = try_drain_all(&mut self.detail_rx, |ev: DetailEvent| {
            any = true;
            // 详情带回 cover_url 时先入队, 由 `AppModel` 统一下载（见 `events::drain`）。
            if let DetailState::Loaded(book) = &ev.state
                && let Some(cover_url) = book
                    .cover_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            {
                self.pending_cover_prefetch
                    .push((ev.source_id, cover_url.to_string()));
            }
            self.detail_cache.insert((ev.source_id, ev.url), ev.state);
        });
        any
    }

    /// 排空封面下载完成事件通道。
    fn drain_cover(&mut self) -> bool {
        let mut any = false;
        let outcome = try_drain_all(&mut self.cover_rx, |ev: CoverEvent| {
            any = true;
            self.cover_in_flight.remove(&(ev.source_id, ev.url.clone()));
            let entry = cover_entry_from_bytes(ev.source_id, &ev.url, ev.bytes);
            self.cover_cache.put((ev.source_id, ev.url), entry);
        });
        // sender 已 drop: 清掉 `cover_tx`, 免得之后 `spawn_cover_download` 误用旧 tx。
        if matches!(outcome, DrainOutcome::Disconnected) {
            self.cover_tx = None;
        }
        any
    }

    /// 排空 TOC 预取后台通道。
    fn drain_toc(&mut self) -> bool {
        let mut any = false;
        let _ = try_drain_all(&mut self.toc_rx, |ev: TocEvent| {
            any = true;
            // 首次加载完成时初始化章节范围。
            if let TocState::Loaded(_, chapters) = &ev.state
                && (self.chapter_range_start == 0 || self.chapter_range_end == 0)
            {
                self.chapter_range_start = 1;
                self.chapter_range_end = chapters.len() as u32;
            }
            self.toc_cache.insert((ev.source_id, ev.url), ev.state);
        });
        any
    }

    /// 派一个封面下载任务。已有缓存 / 正在下载 / url 为空时直接返回（幂等）。
    pub fn spawn_cover_download(
        &mut self,
        source_id: i32,
        url: &str,
        client: &reqwest::Client,
        runtime: &Runtime,
    ) {
        let url = url.trim();
        if url.is_empty() {
            return;
        }
        let key = (source_id, url.to_string());
        // 用 `peek` 而非 `contains`: 这里只判断"是否要发请求", 不需要 promote 顺序。
        if self.cover_cache.peek(&key).is_some() || self.cover_in_flight.contains(&key) {
            return;
        }
        self.cover_in_flight.insert(key);

        let tx = if let Some(t) = self.cover_tx.as_ref() {
            t.clone()
        } else {
            let (t, r) = mpsc::unbounded_channel();
            self.cover_tx = Some(t.clone());
            self.cover_rx = Some(r);
            t
        };

        let url_owned = url.to_string();
        let source_id_send = source_id;
        // `client` 借自 caller 不能跨 `.await` move; `reqwest::Client::clone` 是廉价
        // Arc clone (共享连接池), 正是"跨任务复用"要的语义。
        let client = client.clone();
        runtime.spawn(async move {
            let key_send = (source_id_send, url_owned.clone());
            let referer = crate::http::origin_or_self(&url_owned);
            let ua = crate::http::ua::random_ua();
            let result: Option<Vec<u8>> = match client
                .get(&url_owned)
                .timeout(std::time::Duration::from_secs(15))
                .header(reqwest::header::USER_AGENT, ua)
                .header(reqwest::header::REFERER, referer)
                .header(reqwest::header::ACCEPT, "image/*,*/*;q=0.8")
                .send()
                .await
            {
                Ok(r) => {
                    let status = r.status();
                    if status.is_success() {
                        match r.bytes().await {
                            Ok(b) if !b.is_empty() => Some(b.to_vec()),
                            Ok(_) => {
                                tracing::warn!("封面下载失败（已忽略）: 空 body for {}", url_owned);
                                None
                            }
                            Err(e) => {
                                tracing::warn!("封面下载失败（已忽略）: {e} for {}", url_owned);
                                None
                            }
                        }
                    } else {
                        tracing::warn!("封面下载失败（已忽略）: HTTP {} for {}", status, url_owned);
                        None
                    }
                }
                Err(e) => {
                    tracing::warn!("封面请求失败（已忽略）: {e} for {}", url_owned);
                    None
                }
            };

            let _ = tx.send(CoverEvent {
                source_id: key_send.0,
                url: key_send.1,
                bytes: result,
            });
        });
    }
}

#[cfg(test)]
mod search_state_tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use crate::config::AppConfig;

    #[test]
    fn cover_cache_initially_empty() {
        let s = SearchState::default();
        assert!(s.cover_cache.is_empty());
        assert!(s.cover_in_flight.is_empty());
        assert!(s.cover_rx.is_none());
        assert!(s.cover_tx.is_none());
        assert!(s.pending_cover_prefetch.is_empty());
    }

    fn make_test_client() -> reqwest::Client {
        crate::http::client::build_async_client(
            &AppConfig::default(),
            &crate::http::client::ClientOptions::default(),
        )
        .unwrap()
    }

    #[test]
    fn spawn_cover_download_is_idempotent() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let client = make_test_client();
        let mut s = SearchState::default();
        let url = "https://example.com/cover.png";

        s.spawn_cover_download(1, url, &client, &rt);
        let in_flight_after_first = s.cover_in_flight.len();
        assert_eq!(in_flight_after_first, 1);

        s.spawn_cover_download(1, url, &client, &rt);
        assert_eq!(s.cover_in_flight.len(), 1, "重复调用不应重复入队");

        s.spawn_cover_download(1, "  https://example.com/cover.png  ", &client, &rt);
        assert_eq!(
            s.cover_in_flight.len(),
            1,
            "带空格的同一 URL 也不应重复入队"
        );
    }

    #[test]
    fn spawn_cover_download_skips_empty_url() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let client = make_test_client();
        let mut s = SearchState::default();

        s.spawn_cover_download(1, "", &client, &rt);
        s.spawn_cover_download(1, "   ", &client, &rt);
        assert!(s.cover_in_flight.is_empty());
    }

    /// 回归测试: 跑完 spawn 后 drop `multi_thread` runtime 不应触发
    /// "Cannot drop a runtime in a context where blocking is not allowed"。
    #[test]
    fn cover_runtime_drop_does_not_panic() {
        use std::sync::Arc;
        let rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .thread_name("so-novel-rt-test")
                .build()
                .unwrap(),
        );
        let client = make_test_client();
        let mut s = SearchState::default();

        s.spawn_cover_download(1, "https://example.com/cover.png", &client, &rt);
        std::thread::sleep(std::time::Duration::from_millis(500));
        drop(rt);
    }

    /// 回归测试：切活跃书源文件后旧 results / 缓存全部清空, 但用户输入的
    /// `keyword` 保留 —— `source_id` 是数值弱匹配, 旧 id 可能指向完全不同的
    /// rule, 留旧结果会让用户点到错源下载。
    #[test]
    fn clear_results_and_caches_resets_rule_bound_state() {
        let mut s = SearchState {
            keyword: "校花".to_string(),
            ..Default::default()
        };
        s.source_id = Some(3);
        s.last_keyword = Some("校花".to_string());
        s.results.push(SearchResult {
            source_id: 3,
            source_name: "梦书中文".to_string(),
            url: "http://www.mcxs.la/148_148487/".to_string(),
            book_name: "校花别追了".to_string(),
            ..Default::default()
        });
        s.detail_cache.insert(
            (3, "http://www.mcxs.la/148_148487/".to_string()),
            DetailState::Loaded(Box::default()),
        );
        s.toc_cache.insert(
            (3, "http://www.mcxs.la/148_148487/".to_string()),
            TocState::Loaded(Box::default(), vec![]),
        );
        s.cover_cache.put(
            (3, "http://example.com/c.jpg".to_string()),
            CoverEntry::Failed("test".to_string()),
        );

        s.clear_results_and_caches();
        // 二次 clear 在 default 状态上不应 panic。
        s.clear_results_and_caches();

        assert_eq!(s.keyword, "校花");
        assert!(s.source_id.is_none(), "source_id 应重置为 None");
        assert!(s.results.is_empty(), "results 应清空");
        assert!(s.detail_cache.is_empty(), "detail_cache 应清空");
        assert!(s.toc_cache.is_empty(), "toc_cache 应清空");
        assert!(s.cover_cache.is_empty(), "cover_cache 应清空");
    }
}
