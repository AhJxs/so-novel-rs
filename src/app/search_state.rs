//! 搜索状态：搜索页的全部状态 + 后台通信。

use std::collections::{HashMap, HashSet};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use crate::models::{Book, Chapter, SearchResult};

use super::cover::{CoverEntry, cover_entry_from_bytes};

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

/// 搜索状态（搜索下载页用）。
#[derive(Default)]
pub struct SearchState {
    /// 用户输入。
    pub keyword: String,
    /// `None` = 聚合搜索；`Some(rule_id)` = 仅当前书源。
    pub source_id: Option<i32>,

    /// 上次搜索的关键词（用于结果列表的标题展示，知道是哪次搜的）。
    pub last_keyword: Option<String>,
    /// 已收到的结果（按 source_id 升序）。
    pub results: Vec<SearchResult>,
    /// 各源的搜索状态：true = 跑完，false = 还在跑。
    /// 用 (source_id, source_name, status) 让 UI 显示哪个源还在等。
    pub source_status: Vec<(i32, String, SourceStatus)>,
    /// 整体是否在跑（true 时禁用搜索按钮）。
    pub running: bool,
    /// 最近一次错误（顶部红条）。
    pub last_error: Option<String>,

    /// 后台搜索通过此通道汇报"单源完成"。
    pub rx: Option<mpsc::UnboundedReceiver<SourceSearchEvent>>,
    /// 总共要等多少源（含错误源）。
    pub expected: usize,
    /// 已收到多少源。
    pub received: usize,

    /// 当前选中的搜索结果（行索引）；用于右侧详情面板。
    pub selected: Option<usize>,
    /// 详情缓存：(source_id, url) → DetailState。后台 spawn 后回写。
    pub detail_cache: HashMap<(i32, String), DetailState>,
    /// 详情后台任务的接收端（每条结果一个事件）。
    pub detail_rx: Option<mpsc::UnboundedReceiver<DetailEvent>>,

    /// spawn_search 时拷自 cfg.search_filter；全部源返回后用它决定是否调用 filter_sort。
    pub filter_after_done: bool,

    /// TOC 预取缓存：(source_id, url) → TocState。
    pub toc_cache: HashMap<(i32, String), TocState>,
    /// TOC 预取后台任务的接收端。
    pub toc_rx: Option<mpsc::UnboundedReceiver<TocEvent>>,
    /// 用户选择的章节起始序号（1-based）。
    pub chapter_range_start: u32,
    /// 用户选择的章节结束序号（1-based）。
    pub chapter_range_end: u32,

    // ---- 封面（5b 增强） ----
    /// 封面下载完成通道的发送端：保留以便多次 spawn 复用同一通道。
    pub cover_tx: Option<mpsc::UnboundedSender<CoverEvent>>,
    /// 封面下载完成通道的接收端。
    pub cover_rx: Option<mpsc::UnboundedReceiver<CoverEvent>>,
    /// 封面结果缓存：(source_id, cover_url) → CoverEntry。
    pub cover_cache: HashMap<(i32, String), CoverEntry>,
    /// 正在下载中的封面 URL；防止重复 spawn。
    pub cover_in_flight: HashSet<(i32, String)>,
    /// drain_detail 期间收集到的待 prefetch 封面 URL，drain 后由 AppModel 取出统一派发。
    pub pending_cover_prefetch: Vec<(i32, String)>,
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
            DetailState::Loaded(b) => Some(b),
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
    pub result: Result<Vec<SearchResult>, String>,
}

impl SearchState {
    /// rule 集合整体变化时（切活跃书源文件 / 导入触发 active 重载）调用。
    /// `SearchResult.source_id: i32` 是数值弱匹配 —— 同一 id 在新文件里可能
    /// 指向完全不同的源，留着旧 results 会让用户点到错源下载，所以整体退到
    /// `Default`；`keyword` 是用户连续输入，必须保留。
    pub fn clear_results_and_caches(&mut self) {
        let keyword = std::mem::take(&mut self.keyword);
        *self = Self::default();
        self.keyword = keyword;
    }

    /// 排空通道；返回是否有事件（触发 repaint）。
    pub fn drain(&mut self) -> bool {
        let mut any = false;
        if let Some(rx) = self.rx.as_mut() {
            loop {
                match rx.try_recv() {
                    Ok(ev) => {
                        any = true;
                        self.received += 1;
                        let status = match ev.result {
                            Ok(list) => {
                                let n = list.len();
                                self.results.extend(list);
                                SourceStatus::Ok(n)
                            }
                            Err(e) => {
                                let line = e.lines().next().unwrap_or("(空错误)");
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
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        self.rx = None;
                        break;
                    }
                }
            }
        }
        if self.received >= self.expected && self.expected > 0 {
            self.running = false;
            self.rx = None;
            if self.filter_after_done {
                if let Some(kw) = self.last_keyword.as_deref() {
                    let new_results = crate::parser::filter_sort(&self.results, kw);
                    self.selected = None;
                    self.results = new_results;
                }
            }
        }

        any |= self.drain_detail();
        any |= self.drain_cover();
        any |= self.drain_toc();
        any
    }

    /// 排空详情后台通道。
    fn drain_detail(&mut self) -> bool {
        let Some(rx) = self.detail_rx.as_mut() else {
            return false;
        };
        let mut any = false;
        loop {
            match rx.try_recv() {
                Ok(ev) => {
                    any = true;
                    if let DetailState::Loaded(book) = &ev.state {
                        if let Some(cover_url) = book
                            .cover_url
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                        {
                            self.pending_cover_prefetch
                                .push((ev.source_id, cover_url.to_string()));
                        }
                    }
                    self.detail_cache.insert((ev.source_id, ev.url), ev.state);
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.detail_rx = None;
                    break;
                }
            }
        }
        any
    }

    /// 排空封面下载完成事件通道。
    fn drain_cover(&mut self) -> bool {
        let Some(rx) = self.cover_rx.as_mut() else {
            return false;
        };
        let mut any = false;
        loop {
            match rx.try_recv() {
                Ok(ev) => {
                    any = true;
                    self.cover_in_flight.remove(&(ev.source_id, ev.url.clone()));
                    let entry = cover_entry_from_bytes(ev.source_id, &ev.url, ev.bytes);
                    self.cover_cache.insert((ev.source_id, ev.url), entry);
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.cover_rx = None;
                    self.cover_tx = None;
                    break;
                }
            }
        }
        any
    }

    /// 排空 TOC 预取后台通道。
    fn drain_toc(&mut self) -> bool {
        let Some(rx) = self.toc_rx.as_mut() else {
            return false;
        };
        let mut any = false;
        loop {
            match rx.try_recv() {
                Ok(ev) => {
                    any = true;
                    // 首次加载完成时初始化章节范围
                    if let TocState::Loaded(_, chapters) = &ev.state {
                        if self.chapter_range_start == 0 || self.chapter_range_end == 0 {
                            self.chapter_range_start = 1;
                            self.chapter_range_end = chapters.len() as u32;
                        }
                    }
                    self.toc_cache.insert((ev.source_id, ev.url), ev.state);
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.toc_rx = None;
                    break;
                }
            }
        }
        any
    }

    /// 派一个封面下载任务。已有缓存 / 正在下载 / url 为空时直接返回（幂等）。
    /// `client` 是共享 HTTP client 集合，封面下载固定走 safe 通道（unsafe_ssl=false
    /// 的常规请求），不复用 `cfg` 自己造 client。
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
        if self.cover_cache.contains_key(&key) || self.cover_in_flight.contains(&key) {
            return;
        }
        self.cover_in_flight.insert(key.clone());

        let tx = match self.cover_tx.as_ref() {
            Some(t) => t.clone(),
            None => {
                let (t, r) = mpsc::unbounded_channel();
                self.cover_tx = Some(t.clone());
                self.cover_rx = Some(r);
                t
            }
        };

        let url_owned = url.to_string();
        let source_id_send = source_id;
        // `client` 是 `&reqwest::Client` 借自 caller，不能跨 `.await` move。
        // reqwest::Client::clone 是廉价 Arc clone（共享底层连接池），
        // 这正是 Phase 3.1 想要的"跨任务复用连接池"语义。
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
                    if !status.is_success() {
                        tracing::warn!("封面下载失败（已忽略）: HTTP {} for {}", status, url_owned);
                        None
                    } else {
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

    /// 回归测试：跑完 spawn 后 drop multi_thread runtime 不应触发
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

    /// 回归测试：切活跃书源文件后，旧 results / 缓存 全部清空，但用户输入
    /// 的 `keyword` 保留（用户连续输入不应被打断）。`source_id` 重置为 `None`，
    /// 避免 dropdown 指着新文件里不存在的 id 让下一次搜索派空。
    ///
    /// 场景对应：`SearchResult.source_id: i32` 弱匹配，切文件后旧 id 可能指向
    /// 完全不同的 rule —— 直接清空比"保留 + 静默错源下载"安全得多。
    #[test]
    fn clear_results_and_caches_resets_rule_bound_state() {
        let mut s = SearchState {
            keyword: "校花".to_string(),
            ..Default::default()
        };
        // 模拟"已搜过一轮"：往每个 rule-bound 容器里塞一条样本
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
        s.cover_cache.insert(
            (3, "http://example.com/c.jpg".to_string()),
            CoverEntry::Failed("test".to_string()),
        );

        s.clear_results_and_caches();
        // 二次 clear 在 default 状态上不应 panic
        s.clear_results_and_caches();

        // keyword 保留 —— 其他字段都回到 Default，靠类型系统保证
        assert_eq!(s.keyword, "校花");
        assert!(s.source_id.is_none(), "source_id 应重置为 None");
        assert!(s.results.is_empty(), "results 应清空");
        assert!(s.detail_cache.is_empty(), "detail_cache 应清空");
        assert!(s.toc_cache.is_empty(), "toc_cache 应清空");
        assert!(s.cover_cache.is_empty(), "cover_cache 应清空");
    }
}
