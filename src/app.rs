//! 应用状态、页面路由、生命周期。
//!
//! 关键设计：
//!
//! - `eframe::run_native` 占用主循环，**不能**用 `#[tokio::main]`。
//!   因此 `SoNovelApp` 自己持有一个 `&'static tokio::runtime::Runtime`
//!   （**通过 `Box::leak` 故意泄漏**，避免 runtime drop 时机不当触发
//!   "Cannot drop a runtime in a context where blocking is not allowed" panic）。
//!   所有后台任务（聚合搜索、`download_book`）都用 `runtime.spawn(...)`。
//! - 后台 → UI 通信走 `mpsc::UnboundedSender<Progress>`；
//!   UI 在 `update` 循环 `try_recv` 排空，绝不阻塞。
//! - UI → 后台取消走 `crawler::CancelToken`（`Arc<AtomicBool>`）。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use crate::config::{load_config, AppConfig, ConfigPaths};
use crate::crawler::{CancelToken, Progress};
use crate::models::{Book, Rule, SearchResult};
use crate::rules::load_rules_from_path;
use crate::ui::nav::NavPage;
use crate::ui::theme;

// 共享的 tokio runtime：leak 后得到 `&'static Runtime`，永不 drop，
// 彻底规避 "Cannot drop a runtime in a context where blocking is not allowed"
// panic（即便 eframe 退出 / 某些边界场景下 runtime 在 worker 线程上 drop）。
//
// 进程退出时 OS 自动回收所有线程与内存，所以 leak 不影响清理。
fn build_shared_runtime() -> &'static Runtime {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("so-novel-rt")
        .build()
        .expect("build tokio runtime");
    Box::leak(Box::new(rt))
}

/// 一个正在跑的下载任务（由搜索页"下载"按钮触发，下载页/任务页消费）。
pub struct DownloadTask {
    /// 任务唯一 id（递增）。
    pub id: u64,
    /// 触发时拿到的搜索结果，包含 source / book_url / 书名作者等信息。
    pub origin: SearchResult,
    /// 后台推送进度的接收端；每帧 `try_recv` 排空。
    pub rx: mpsc::UnboundedReceiver<Progress>,
    /// 后台任务的取消令牌。
    pub cancel: CancelToken,

    // ---- 累计状态（每帧 try_recv 时更新） ----
    pub started_at: Instant,
    pub book_meta: Option<Book>,
    pub total_chapters: usize,
    pub completed: u32,
    pub failed: u32,
    pub last_chapter_title: String,
    /// `Some(Ok(path))` 完成；`Some(Err(reason))` 失败 / 取消；`None` 还在跑。
    pub finished: Option<Result<std::path::PathBuf, String>>,
    /// 失败章节明细（用于任务页详情显示）。
    pub failures: Vec<(u32, String, String)>,
}

impl DownloadTask {
    /// 排空进度通道；返回是否产生过事件（用于触发 repaint）。
    pub fn drain(&mut self) -> bool {
        let mut any = false;
        loop {
            match self.rx.try_recv() {
                Ok(ev) => {
                    any = true;
                    match ev {
                        Progress::BookResolved {
                            book,
                            total_chapters,
                        } => {
                            self.book_meta = Some(book);
                            self.total_chapters = total_chapters;
                        }
                        Progress::ChapterDone { index, title } => {
                            self.completed += 1;
                            self.last_chapter_title = title;
                            let _ = index;
                        }
                        Progress::ChapterFailed {
                            index,
                            title,
                            reason,
                        } => {
                            self.failed += 1;
                            self.failures.push((index, title, reason));
                        }
                        Progress::Finished { output_path } => {
                            self.finished = Some(Ok(output_path));
                        }
                        Progress::Cancelled => {
                            self.finished = Some(Err("用户已取消".to_string()));
                        }
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    if self.finished.is_none() {
                        // 后台 panic 或异常退出，没来得及发 Finished/Cancelled。
                        self.finished = Some(Err("后台任务异常退出（通道已断开）".to_string()));
                    }
                    break;
                }
            }
        }
        any
    }

    pub fn is_running(&self) -> bool {
        self.finished.is_none()
    }

    pub fn book_name(&self) -> &str {
        self.book_meta
            .as_ref()
            .map(|b| b.book_name.as_str())
            .unwrap_or(self.origin.book_name.as_str())
    }
}

/// 本地书库的一个条目。对应 `download_path` 下一个已生成的电子书文件。
#[derive(Debug, Clone)]
pub struct LibraryEntry {
    pub path: PathBuf,
    pub file_name: String,
    pub size_bytes: u64,
    /// 文件修改时间。Unix 时间戳（秒）；获取失败时为 0。
    pub modified_unix_secs: u64,
    /// 扩展名（小写、不含点）：epub / txt / zip / html / pdf / 其它。
    pub ext: String,
}

/// 本地书库 UI 状态。
#[derive(Default)]
pub struct LibraryState {
    /// 当前扫描结果（已按修改时间倒序）。
    pub entries: Vec<LibraryEntry>,
    /// 用户输入的搜索关键字（按文件名过滤）。
    pub filter_text: String,
    /// 用户选的格式过滤（None = 全部）。
    pub filter_ext: Option<String>,
    /// 上次扫描时的下载目录绝对路径（变化时自动重扫）。
    pub scanned_dir: Option<PathBuf>,
    /// 待删除确认中的条目路径；点删除后置位，再次点确认才真正删除。
    pub pending_delete: Option<PathBuf>,
    /// 上次扫描 / 操作失败提示。
    pub last_error: Option<String>,
}

/// 书源管理页状态：连通性检测的结果与运行标记。
#[derive(Default)]
pub struct SourcesState {
    /// source_id → 探测结果（按到达顺序覆盖；不要求全部都到齐）。
    pub health: HashMap<i32, crate::crawler::health::SourceHealth>,
    /// 是否正在跑探测（true 时禁用按钮 + 显示 spinner）。
    pub running: bool,
    /// 总共要等多少源；用于 UI 显示 "M/N 已返回"。
    pub expected: usize,
    pub received: usize,
    /// 后台推送的接收端，update 循环 drain。
    pub rx: Option<mpsc::UnboundedReceiver<crate::crawler::health::SourceHealth>>,
}

impl SourcesState {
    /// 排空通道；返回是否产生过事件（触发 repaint）。
    pub fn drain(&mut self) -> bool {
        let Some(rx) = self.rx.as_mut() else {
            return false;
        };
        let mut any = false;
        loop {
            match rx.try_recv() {
                Ok(h) => {
                    any = true;
                    self.received += 1;
                    self.health.insert(h.source_id, h);
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.rx = None;
                    self.running = false;
                    break;
                }
            }
        }
        if self.expected > 0 && self.received >= self.expected {
            self.running = false;
            self.rx = None;
        }
        any
    }
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

    /// 用户手动隐藏的"最近任务横幅"对应的任务 id。
    /// 只有当最新任务 id 不等于此值时才显示横幅 — 这样新触发下载会自动重新显示。
    pub banner_dismissed_for: Option<u64>,

    /// 当前打开的详情弹窗对应的搜索结果索引。`None` 表示未打开。
    /// 点击搜索结果卡片的书名 → 设为 `Some(idx)`；点弹窗 ✕ 或 ESC 关闭。
    pub detail_popup_for: Option<usize>,

    // ---- 封面（5b 增强） ----
    /// 封面下载完成通道的发送端：保留以便多次 spawn 复用同一通道。
    pub cover_tx: Option<mpsc::UnboundedSender<CoverEvent>>,
    /// 封面下载完成通道的接收端。
    pub cover_rx: Option<mpsc::UnboundedReceiver<CoverEvent>>,
    /// 封面结果缓存：(source_id, cover_url) → CoverEntry。
    pub cover_cache: HashMap<(i32, String), CoverEntry>,
    /// 正在下载中的封面 URL；防止重复 spawn。
    pub cover_in_flight: HashSet<(i32, String)>,
    /// drain_detail 期间收集到的待 prefetch 封面 URL，drain 后由 SoNovelApp 取出统一派发。
    pub pending_cover_prefetch: Vec<(i32, String)>,
}

/// 详情面板加载状态。
#[derive(Debug, Clone)]
pub enum DetailState {
    Pending,
    Loaded(Box<crate::models::Book>),
    Failed(String),
}

impl DetailState {
    /// 仅当 Loaded 状态可取书；Pending/Failed 返回 None。
    pub fn book(&self) -> Option<&crate::models::Book> {
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

/// 封面下载完成事件。后台 HTTP 下载 → UI 构造 `egui_extras::RetainedImage`。
#[derive(Debug)]
pub struct CoverEvent {
    pub source_id: i32,
    pub url: String,
    /// 下载成功：Some(bytes)；失败：None。
    pub bytes: Option<Vec<u8>>,
}

/// 封面缓存条目。`Ready` 持有 `egui::Image<'static>`（懒上传纹理，按 URI 去重）；
/// `Failed` 保留错误文案以便 UI 给出可见反馈而非静默。
pub enum CoverEntry {
    Ready(egui::Image<'static>),
    Failed(String),
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
                                // 短错误文案：取首行，最多 60 字
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
            // 全部源返回后，按 cfg.search_filter 做一次相似度过滤排序。
            // 注意：detail_cache 的 (source_id, url) 仍然有效，filter_sort 只重排不修改字段。
            if self.filter_after_done {
                if let Some(kw) = self.last_keyword.as_deref() {
                    let new_results = crate::parser::filter_sort(&self.results, kw);
                    // 选中行清掉（重排后索引意义变了）
                    self.selected = None;
                    self.results = new_results;
                }
            }
        }

        // 顺便排空详情通道
        any |= self.drain_detail();
        any |= self.drain_cover();
        any
    }

    /// 排空详情后台通道；与 search 主通道独立但合并到 drain() 结果。
    /// 详情就绪后，若该书有 `cover_url`，追加到 `pending_cover_prefetch`，
    /// 由 `SoNovelApp::update` 在 `drain()` 之后统一派发（避免 SearchState 持 Runtime）。
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
                    // 不清空 rx：还可能有后续请求复用
                    self.detail_rx = None;
                    break;
                }
            }
        }
        any
    }

    /// 排空封面下载完成事件通道。
    /// 后台线程只负责 HTTP 取字节；构造 `egui::Image` 在 UI 线程做（无需 Context，
    /// 实际纹理上传由 egui 在 `ui.add(&image)` 时按 URI 懒加载 + 缓存）。
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

    /// 派一个封面下载任务。已有缓存 / 正在下载 / url 为空时直接返回（幂等）。
    /// `cfg` 仅在调用期间借用，函数内部 clone 后 move 进 async block。
    pub fn spawn_cover_download(
        &mut self,
        source_id: i32,
        url: &str,
        cfg: &AppConfig,
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

        // 长生命周期通道：首次创建 sender+receiver，后续 clone sender。
        let tx = match self.cover_tx.as_ref() {
            Some(t) => t.clone(),
            None => {
                let (t, r) = mpsc::unbounded_channel();
                self.cover_tx = Some(t.clone());
                self.cover_rx = Some(r);
                t
            }
        };

        let cfg = cfg.clone();
        let url_owned = url.to_string();
        let source_id_send = source_id;
        runtime.spawn(async move {
            let key_send = (source_id_send, url_owned.clone());
            // 用 async reqwest：共用我们 runtime 的 tokio 上下文，
            // **不会**像 reqwest::blocking 那样建一个嵌套 current_thread runtime
            // （嵌套 runtime 在 spawn_blocking 工作线程上 drop 会触发
            // "Cannot drop a runtime in a context where blocking is not allowed" panic）。
            let opts = crate::http::client::ClientOptions {
                // 封面多为公开 CDN；不走源站 ignore_ssl。CDN 多半不会校验客户端证书。
                unsafe_ssl: false,
            };
            // 大多数书源的封面 CDN 要求 Referer + UA，否则 403/404；
            // 用图片 URL 自身的 origin 当 Referer（与 fetch.rs 的策略一致）。
            let referer = crate::http::origin_or_self(&url_owned);
            let ua = crate::http::ua::random_ua();
            let result: Option<Vec<u8>> = match crate::http::client::build_async_client(&cfg, &opts)
            {
                Ok(client) => match client
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
                            tracing::warn!(
                                "封面下载失败（已忽略）: HTTP {} for {}",
                                status,
                                url_owned
                            );
                            None
                        } else {
                            match r.bytes().await {
                                Ok(b) if !b.is_empty() => Some(b.to_vec()),
                                Ok(_) => {
                                    tracing::warn!(
                                        "封面下载失败（已忽略）: 空 body for {}",
                                        url_owned
                                    );
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
                },
                Err(e) => {
                    tracing::warn!("封面 client 构造失败（已忽略）: {e}");
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

/// 把后台下载的字节构造为 `CoverEntry`。
/// 失败（空 body / 解码错误）时给出中文短文案，UI 仍会显示一行小字提示。
///
/// URI 取自 `(source_id, cover_url)`，确保不同书源/不同封面在 egui 内部纹理缓存里互不污染。
pub(crate) fn cover_entry_from_bytes(
    source_id: i32,
    cover_url: &str,
    bytes: Option<Vec<u8>>,
) -> CoverEntry {
    match bytes {
        None => CoverEntry::Failed("下载为空或失败".to_string()),
        Some(b) => {
            // egui::Image::from_bytes 是懒解码（错误要等 ui.add 时才暴露），
            // 这里用 image::ImageReader 提前验证字节是真的图片，让 Failed 路径可达。
            let probe = image::ImageReader::new(std::io::Cursor::new(&b))
                .with_guessed_format()
                .ok()
                .and_then(|r| r.decode().ok());
            match probe {
                Some(_) => {
                    let uri = format!("cover://{source_id}/{}", hash_short(cover_url));
                    CoverEntry::Ready(egui::Image::from_bytes(uri, b))
                }
                None => CoverEntry::Failed("图片解码失败（非有效图片或格式不支持）".to_string()),
            }
        }
    }
}

/// 短哈希（fnv-like 64-bit → 16 hex），仅用于 URI 去重 key，**不是**密码学用途。
fn hash_short(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// 应用整体状态。任何 UI 访问的字段都集中在这里，便于持久化与测试。
pub struct SoNovelApp {
    pub paths: ConfigPaths,
    pub config: AppConfig,
    pub rules: Vec<Rule>,
    pub rule_load_error: Option<String>,
    pub config_load_error: Option<String>,

    /// 用户对书源的禁用 / 启用覆写。toggle 后立即持久化到
    /// `paths.source_overrides_file`，加载规则时应用到每个 rule.disabled。
    pub source_overrides: crate::rules::SourceOverrides,

    pub current_page: NavPage,

    /// 设置页可编辑的副本；点击"保存"后写回 config.ini，并替换 `config`。
    pub draft_config: AppConfig,

    /// 顶部状态栏的临时消息（保存成功 / 加载失败等）。
    pub toast: Option<(String, Instant)>,

    /// 后台任务运行时。所有 spawn 都走它。
    /// 通过 `Box::leak` 得到 `&'static Runtime`，永不 drop ——
    /// 见 `build_shared_runtime` 注释，规避 Runtime drop panic。
    pub runtime: &'static Runtime,

    /// 是否已对 OS 窗口应用 DWM 圆角 + 沉浸式暗色。
    /// 第一次 update 时拿到 HWND 调用一次，之后不再调用。
    pub window_chrome_applied: bool,

    /// 上一帧的 dark_mode 状态。主题切换后用 `apply_windows11_chrome` 重新设置
    /// DWM 暗色标题栏（沉浸式标题栏不会随 ctx.set_theme 自动跟随）。
    pub last_dark_mode: bool,

    /// 搜索下载页状态。
    pub search: SearchState,

    /// 活动 / 已完成的下载任务。最新加在末尾。
    pub tasks: Vec<DownloadTask>,
    next_task_id: u64,

    /// 本地书库状态（首次进入 Library 页时延迟扫描）。
    pub library: LibraryState,

    /// 书源管理页状态（连通性检测结果）。
    pub sources_state: SourcesState,
}

impl SoNovelApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // 注入中文字体（egui 默认字体不含 CJK，否则会显示豆腐块）。
        theme::install_cjk_fonts(&cc.egui_ctx);
        //theme::install_visuals(&cc.egui_ctx);
        // 安装 egui_extras 的图片 loader（PNG/JPEG/SVG/GIF/...）。
        // 不调用这个，`egui::Image::from_bytes` 会报 "no image loaders installed"。
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let paths = ConfigPaths::discover();

        let (config, config_load_error) = match load_config(&paths.config_file) {
            Ok(c) => (c, None),
            Err(e) => {
                tracing::warn!("config load failed: {e:#}");
                (AppConfig::default(), Some(format!("{e:#}")))
            }
        };

        let (mut rules, rule_load_error) = load_initial_rules(&paths.rules_dir, &config);

        // 应用用户的禁用覆写（独立 sidecar JSON，不污染上游 rules 文件）。
        let source_overrides = crate::rules::SourceOverrides::load(&paths.source_overrides_file);
        source_overrides.apply_to_rules(&mut rules);

        let runtime = build_shared_runtime();

        let draft_config = config.clone();
        Self {
            paths,
            config,
            rules,
            rule_load_error,
            config_load_error,
            source_overrides,
            current_page: NavPage::Search,
            draft_config,
            toast: None,
            runtime,
            window_chrome_applied: false,
            last_dark_mode: false,
            search: SearchState::default(),
            tasks: Vec::new(),
            next_task_id: 1,
            library: LibraryState::default(),
            sources_state: SourcesState::default(),
        }
    }

    pub fn show_toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    /// 派一个新的下载任务到后台。返回新任务的 id。
    pub fn spawn_download(&mut self, target: SearchResult) -> u64 {
        let id = self.next_task_id;
        self.next_task_id += 1;
        let (tx, rx) = mpsc::unbounded_channel::<Progress>();
        let cancel = CancelToken::new();

        // 找到对应规则；找不到则失败提示。
        let rule = self
            .rules
            .iter()
            .find(|r| r.id == target.source_id)
            .cloned();
        let cfg = self.config.clone();
        let book_url = target.url.clone();
        let cancel_for_task = cancel.clone();
        let tx_for_task = tx.clone();

        self.runtime.spawn(async move {
            let Some(rule) = rule else {
                let _ = tx_for_task.send(Progress::Cancelled);
                return;
            };
            let source = crate::rules::Source::from(rule, &cfg);
            let opts = crate::crawler::DownloadOptions {
                progress: tx_for_task,
                cancel: cancel_for_task,
            };
            // 错误也作为最终态：转化为 Cancelled 让 UI 把任务收尾。
            // 失败时仅 log + 任务列表里 finished=Err(...)。
            if let Err(e) = crate::crawler::download_book(&cfg, &source, &book_url, opts).await {
                tracing::warn!("download_book failed: {e}");
                // download_book 内部已经在每条章节失败时推过 Failed；
                // 这里只是补一个最终态，UI 看到通道关闭即可。
            }
        });

        // 保留 tx 一份给 task：当后台任务结束后，drop tx 让通道关闭，UI 端会感知。
        // （上面 spawn 已 move 走 tx_for_task；本地 tx 是早先 clone 的，不再需要持有，drop 即可）
        drop(tx);

        let task = DownloadTask {
            id,
            origin: target,
            rx,
            cancel,
            started_at: Instant::now(),
            book_meta: None,
            total_chapters: 0,
            completed: 0,
            failed: 0,
            last_chapter_title: String::new(),
            finished: None,
            failures: Vec::new(),
        };
        self.tasks.push(task);
        id
    }

    /// 派聚合搜索任务。返回是否成功派发（关键字非空 + 还没有进行中的搜索）。
    pub fn spawn_search(&mut self) -> bool {
        let keyword = self.search.keyword.trim().to_string();
        if keyword.is_empty() {
            self.search.last_error = Some("请输入关键词".to_string());
            return false;
        }
        if self.search.running {
            return false;
        }

        self.search.last_error = None;
        self.search.last_keyword = Some(keyword.clone());
        self.search.results.clear();
        self.search.source_status.clear();
        self.search.received = 0;

        // 决定要搜哪些源。
        let target_sources: Vec<crate::rules::Source> = if let Some(id) = self.search.source_id {
            self.rules
                .iter()
                .filter(|r| r.id == id)
                .cloned()
                .map(|r| crate::rules::Source::from(r, &self.config))
                .collect()
        } else {
            // 聚合搜索：跳过 disabled 与未启用 search 的书源（与 Java
            // SourceUtils.getSearchableSources 等价）。
            self.rules
                .iter()
                .filter(|r| !r.disabled && r.search.as_ref().map(|s| !s.disabled).unwrap_or(false))
                .cloned()
                .map(|r| crate::rules::Source::from(r, &self.config))
                .collect()
        };

        if target_sources.is_empty() {
            self.search.last_error =
                Some("没有可用的书源（请在 [书源管理] 检查规则文件）".to_string());
            return false;
        }

        // 预填 status = Pending，让 UI 显示等待项
        self.search.source_status = target_sources
            .iter()
            .map(|s| (s.rule.id, s.rule.name.clone(), SourceStatus::Pending))
            .collect();
        self.search.expected = target_sources.len();
        self.search.running = true;
        self.search.filter_after_done = self.config.search_filter;

        let (tx, rx) = mpsc::unbounded_channel::<SourceSearchEvent>();
        self.search.rx = Some(rx);

        let cfg = self.config.clone();
        let cf_bypass = if self.config.cf_bypass.trim().is_empty() {
            None
        } else {
            Some(self.config.cf_bypass.clone())
        };
        let limit = self
            .config
            .search_limit
            .map(|v| v.max(0) as usize)
            .filter(|v| *v > 0);

        self.runtime.spawn(async move {
            let outcomes = crate::crawler::search::search_aggregated(
                &cfg,
                target_sources,
                keyword,
                limit,
                cf_bypass,
            )
            .await;
            for o in outcomes {
                let send_result = match o.result {
                    Ok(list) => Ok(list),
                    Err(e) => Err(format!("{e}")),
                };
                let _ = tx.send(SourceSearchEvent {
                    source_id: o.source_id,
                    source_name: o.source_name,
                    result: send_result,
                });
            }
            // tx drop → UI 端 Disconnected
        });

        true
    }

    /// 选中某条搜索结果；如果之前没拉过详情就 spawn 一次。
    pub fn select_search_result(&mut self, idx: usize) {
        if idx >= self.search.results.len() {
            return;
        }
        self.search.selected = Some(idx);

        let r = &self.search.results[idx];
        let key = (r.source_id, r.url.clone());
        if self.search.detail_cache.contains_key(&key) {
            return; // 已加载过，免重复请求
        }

        // 找规则
        let Some(rule) = self.rules.iter().find(|x| x.id == r.source_id).cloned() else {
            self.search.detail_cache.insert(
                key,
                DetailState::Failed(format!("找不到 ID 为 {} 的书源规则", r.source_id)),
            );
            return;
        };

        // 标记 Pending
        self.search
            .detail_cache
            .insert(key.clone(), DetailState::Pending);

        // 复用一个共用 detail_rx；首次创建
        let tx = match &self.search.detail_rx {
            Some(_) => {
                // 已有通道；复用 sender 需要保留 tx 引用 — 但当前 detail_rx 仅持有 receiver。
                // 简单起见：每次 spawn 新建一个 (tx, rx)，drop 旧 rx。
                let (tx, rx) = mpsc::unbounded_channel();
                self.search.detail_rx = Some(rx);
                tx
            }
            None => {
                let (tx, rx) = mpsc::unbounded_channel();
                self.search.detail_rx = Some(rx);
                tx
            }
        };

        let cfg = self.config.clone();
        let url = r.url.clone();
        let source_id = r.source_id;
        let cf_bypass = if self.config.cf_bypass.trim().is_empty() {
            None
        } else {
            Some(self.config.cf_bypass.clone())
        };

        self.runtime.spawn(async move {
            let url_for_event = url.clone();
            let cf = cf_bypass.clone();
            let result: Result<crate::models::Book, String> =
                tokio::task::spawn_blocking(move || {
                    let opts = crate::http::client::ClientOptions {
                        unsafe_ssl: rule.ignore_ssl,
                    };
                    let client = crate::http::client::build_blocking_client(&cfg, &opts)
                        .map_err(|e| format!("client: {e:#}"))?;
                    crate::parser::parse_book_detail(&client, &rule, &url, cf.as_deref())
                        .map_err(|e| format!("{e}"))
                })
                .await
                .unwrap_or_else(|join_err| Err(format!("spawn_blocking: {join_err}")));

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

    /// 切换书源的禁用状态；立即持久化到 sidecar JSON，并同步到 `self.rules`。
    pub fn toggle_source_disabled(&mut self, source_id: i32) {
        let now_disabled = self.source_overrides.toggle(source_id);
        if let Err(e) = self
            .source_overrides
            .save(&self.paths.source_overrides_file)
        {
            tracing::warn!("source-overrides 保存失败: {e:#}");
            self.show_toast(format!("保存失败: {e}"));
        } else if let Some(r) = self.rules.iter_mut().find(|r| r.id == source_id) {
            r.disabled = now_disabled;
        }
    }

    /// 派一个连通性检测任务到后台，对全部 rules（含已禁用）做 HEAD 探测。
    /// 已禁用的源也检测，方便用户判断要不要重新启用。
    pub fn spawn_health_check(&mut self) {
        if self.sources_state.running {
            return;
        }
        if self.rules.is_empty() {
            self.show_toast("没有可检测的书源");
            return;
        }

        // 重置状态（保留旧 health 一帧让 UI 平滑：但这里直接清空，避免误读旧值）
        self.sources_state.health.clear();
        self.sources_state.received = 0;
        self.sources_state.expected = self.rules.len();
        self.sources_state.running = true;

        let (tx, rx) = mpsc::unbounded_channel();
        self.sources_state.rx = Some(rx);

        let cfg = Arc::new(self.config.clone());
        let rules = self.rules.clone();
        self.runtime.spawn(async move {
            crate::crawler::health::check_sources_health(cfg, rules, tx).await;
        });
    }

    /// 扫描 `download_path` 下所有已生成的电子书文件，写入 `self.library.entries`。
    /// 同步操作（download_path 通常 < 数百个文件，IO 量小，不必 spawn）。
    pub fn refresh_library(&mut self) {
        let dir = PathBuf::from(&self.config.download_path);
        let abs = if dir.is_absolute() {
            dir.clone()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(&dir))
                .unwrap_or(dir)
        };
        self.library.scanned_dir = Some(abs.clone());
        self.library.entries.clear();
        self.library.last_error = None;
        self.library.pending_delete = None;

        if !abs.exists() {
            // 目录还没建（用户没下过书）— 不算错误，只是空。
            return;
        }

        match scan_library_dir(&abs) {
            Ok(mut entries) => {
                entries.sort_by_key(|b| std::cmp::Reverse(b.modified_unix_secs));
                self.library.entries = entries;
            }
            Err(e) => {
                self.library.last_error = Some(format!("扫描下载目录失败: {e}"));
            }
        }
    }

    /// 真正删除一个本地文件；删完后立即重扫。
    pub fn delete_library_entry(&mut self, path: &Path) {
        match std::fs::remove_file(path) {
            Ok(_) => {
                self.show_toast(format!(
                    "已删除: {}",
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("（未知）")
                ));
            }
            Err(e) => {
                self.library.last_error = Some(format!("删除失败: {e}"));
            }
        }
        self.library.pending_delete = None;
        self.refresh_library();
    }
}

fn load_initial_rules(rules_dir: &Path, cfg: &AppConfig) -> (Vec<Rule>, Option<String>) {
    let active = std::path::Path::new(&cfg.active_rules);
    let target = if active.is_absolute() {
        active.to_path_buf()
    } else {
        rules_dir.join(active)
    };

    match load_rules_from_path(&target) {
        Ok(rs) => (rs, None),
        Err(e) => {
            tracing::warn!("rules load failed: {e}");
            (Vec::new(), Some(format!("{e}")))
        }
    }
}

/// 扫描下载目录得到 LibraryEntry 列表。
///
/// 行为：
/// - 仅包含**直接子文件**（不递归子目录，避免把章节缓存目录里成百上千的小文件
///   也列进来）。`Crawler` 已经把章节缓存目录单独命名为 `<书名>(<作者>) EXT/`，
///   合并产物则放在 download_path 根下；二者目录层级清晰可分。
/// - 仅保留 `.epub / .txt / .zip / .html / .pdf` 五种扩展名。
fn scan_library_dir(dir: &Path) -> std::io::Result<Vec<LibraryEntry>> {
    const KEEP_EXT: &[&str] = &["epub", "txt", "zip", "html", "pdf"];

    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(ext_raw) = path.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        let ext = ext_raw.to_ascii_lowercase();
        if !KEEP_EXT.contains(&ext.as_str()) {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let meta = entry.metadata()?;
        let size_bytes = meta.len();
        let modified_unix_secs = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        out.push(LibraryEntry {
            path,
            file_name,
            size_bytes,
            modified_unix_secs,
            ext,
        });
    }
    Ok(out)
}

impl eframe::App for SoNovelApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // 0. 首次 update：把 OS 窗口设为 Windows 11 圆角 + 沉浸式暗色标题栏。
        //    后续主题切换时再次调用以同步 OS 标题栏配色（圆角是持久的，重复设置无害）。
        let dark = ui.ctx().global_style().visuals.dark_mode;
        let need_chrome = !self.window_chrome_applied || self.last_dark_mode != dark;
        if need_chrome {
            if let Some(hwnd) = crate::window::platform::extract_hwnd(frame) {
                crate::window::platform::apply_windows11_chrome(hwnd, dark);
            }
            self.window_chrome_applied = true;
            self.last_dark_mode = dark;
        }

        // 1. 排空所有后台通道。任何事件都触发一次 repaint，让 UI 即时反映进度。
        let mut any_progress = self.search.drain();
        // drain_detail 在此期间把"详情已就绪且有 cover_url"的条目塞进 pending_cover_prefetch；
        // 取出后统一派发，避免 SearchState 持 Runtime / cfg。
        let to_fetch = std::mem::take(&mut self.search.pending_cover_prefetch);
        for (sid, url) in to_fetch {
            self.search
                .spawn_cover_download(sid, &url, &self.config, self.runtime);
        }
        for t in self.tasks.iter_mut() {
            any_progress |= t.drain();
        }
        any_progress |= self.sources_state.drain();
        let ctx = ui.ctx().clone();
        if any_progress {
            ctx.request_repaint();
        }
        // 任意活动任务都让 UI 持续刷新，进度文字会动。
        let any_running = self.search.running
            || self.sources_state.running
            || self.tasks.iter().any(|t| t.is_running());
        if any_running {
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        // 顶部窗口控制条（最小化 / 最大化 / 关闭 + 拖拽区） — 必须在 nav 之前 add，
        // egui 的 Panel 按 add 顺序从上到下堆叠。
        crate::ui::title_bar::show(ui, &ctx);

        // 顶部水平导航 — 在 title_bar 下方
        let visuals = ctx.global_style().visuals.clone();
        egui::Panel::top("nav")
            .frame(theme::content_frame(&visuals))
            .show_inside(ui, |ui| {
                crate::ui::nav::show_in_panel(ui, &ctx, self);
            });

        // 内容区 — 中央面板，content_frame 给整页加内外边距
        egui::CentralPanel::default()
            .frame(theme::content_frame(&visuals))
            .show_inside(ui, |ui| {
                crate::ui::pages::show(ui, self);
            });

        // 在所有 panel 添加完之后，处理无装饰窗口的边缘缩放（光标 + BeginResize）。
        crate::ui::title_bar::handle_window_resize(&ctx);

        // toast 自动消失
        if let Some((_, t)) = self.toast {
            if t.elapsed() > Duration::from_secs(4) {
                self.toast = None;
            }
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }
}

// ============================================================
// 单元测试（5b 封面）
// ============================================================

#[cfg(test)]
mod cover_tests {
    use super::*;
    use std::io::Cursor;

    /// 构造一个 2x2 RGBA 红色像素的 PNG 字节流。
    /// 不读磁盘，完全在内存里生成。
    fn make_png_bytes() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
            .expect("write png");
        buf
    }

    #[test]
    fn cover_entry_from_bytes_decodes_valid_png() {
        let png = make_png_bytes();
        assert!(!png.is_empty(), "PNG 字节流不应为空");
        let entry = cover_entry_from_bytes(7, "https://example.com/cover.png", Some(png));
        match entry {
            CoverEntry::Ready(img) => {
                // egui::Image 内部的 source 是 ImageSource::Bytes，
                // 我们只验证它能成功构造、不是 Failed 即可。
                let _ = img;
            }
            CoverEntry::Failed(e) => panic!("期望 Ready，实际 Failed: {e}"),
        }
    }

    #[test]
    fn cover_entry_from_bytes_rejects_garbage() {
        // 任意非图片字节
        let entry = cover_entry_from_bytes(
            1,
            "https://example.com/bad.png",
            Some(b"this is not a valid image".to_vec()),
        );
        match entry {
            CoverEntry::Failed(msg) => assert!(msg.contains("解码失败"), "错误文案: {msg}"),
            CoverEntry::Ready(_) => panic!("垃圾字节不应成功解码"),
        }
    }

    #[test]
    fn cover_entry_from_bytes_handles_none() {
        let entry = cover_entry_from_bytes(1, "https://example.com/x.png", None);
        assert!(matches!(entry, CoverEntry::Failed(_)));
    }

    #[test]
    fn cover_entry_from_bytes_uses_distinct_uris() {
        // 不同 source_id 或 url 应都能正常构造（egui 内部按 uri 去重缓存，三者互不污染）。
        // 这里只能验证"都能 Ready"——uri 实际值由 egui 内部使用，外部不可见。
        let png = make_png_bytes();
        let a = cover_entry_from_bytes(1, "https://a.com/c.png", Some(png.clone()));
        let b = cover_entry_from_bytes(2, "https://a.com/c.png", Some(png.clone()));
        let c = cover_entry_from_bytes(1, "https://b.com/c.png", Some(png));
        assert!(matches!(a, CoverEntry::Ready(_)));
        assert!(matches!(b, CoverEntry::Ready(_)));
        assert!(matches!(c, CoverEntry::Ready(_)));
    }

    #[test]
    fn hash_short_is_deterministic_and_distinct() {
        let h1 = hash_short("https://a.com/c.png");
        let h2 = hash_short("https://a.com/c.png");
        assert_eq!(h1, h2, "相同输入应得到相同哈希");
        assert_eq!(h1.len(), 16, "应为 16 hex chars (64-bit)");
        let h3 = hash_short("https://b.com/c.png");
        assert_ne!(h1, h3, "不同输入应得到不同哈希");
    }

    #[test]
    fn search_state_cover_cache_initially_empty() {
        let s = SearchState::default();
        assert!(s.cover_cache.is_empty());
        assert!(s.cover_in_flight.is_empty());
        assert!(s.cover_rx.is_none());
        assert!(s.cover_tx.is_none());
        assert!(s.pending_cover_prefetch.is_empty());
    }

    #[test]
    fn search_state_spawn_cover_download_is_idempotent() {
        // 不真正触发 HTTP；只验证幂等性（连调两次不会重复入队）
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let cfg = AppConfig::default();
        let mut s = SearchState::default();
        let url = "https://example.com/cover.png";

        s.spawn_cover_download(1, url, &cfg, &rt);
        let in_flight_after_first = s.cover_in_flight.len();
        assert_eq!(in_flight_after_first, 1);

        s.spawn_cover_download(1, url, &cfg, &rt);
        assert_eq!(s.cover_in_flight.len(), 1, "重复调用不应重复入队");

        s.spawn_cover_download(1, "  https://example.com/cover.png  ", &cfg, &rt);
        assert_eq!(
            s.cover_in_flight.len(),
            1,
            "带空格的同一 URL 也不应重复入队"
        );
    }

    #[test]
    fn search_state_spawn_cover_download_skips_empty_url() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let cfg = AppConfig::default();
        let mut s = SearchState::default();

        s.spawn_cover_download(1, "", &cfg, &rt);
        s.spawn_cover_download(1, "   ", &cfg, &rt);
        assert!(s.cover_in_flight.is_empty());
    }

    /// 回归测试：跑完 spawn 后 drop multi_thread runtime 不应触发
    /// "Cannot drop a runtime in a context where blocking is not allowed"。
    /// 之前用 reqwest::blocking + spawn_blocking 在 cover 路径下会触发此 panic
    /// （reqwest 内部的 current_thread runtime 在 tokio blocking 工作线程上 drop）。
    /// 改用 async reqwest 后修复。
    #[test]
    fn search_state_cover_runtime_drop_does_not_panic() {
        let rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .thread_name("so-novel-rt-test")
                .build()
                .unwrap(),
        );
        let cfg = AppConfig::default();
        let mut s = SearchState::default();

        // 用一个本地 mpsc 让 spawn_cover_download 走完，模拟"下载成功收到字节"。
        // 直接调 spawn 即可；cover 任务会用 build_async_client 试着去取
        // https://example.com/cover.png — 这是 RFC 2606 保留域名，连接会被
        // 立刻 reset，返回 None。不触发 panic 才是关键。
        s.spawn_cover_download(1, "https://example.com/cover.png", &cfg, &rt);

        // 给任务一点时间跑完（不 await，只 sleep）。
        std::thread::sleep(std::time::Duration::from_millis(500));

        // 现在 drop runtime。如果有 runtime-in-context panic，这里会触发。
        // 把 drop 单独放在子作用域，前面 sleep 已经让所有任务结束。
        drop(rt);
    }

    /// **真实回归测试**：在 spawn_blocking 里用 reqwest::blocking 发请求，
    /// 然后 drop client。reqwest 内部 current_thread runtime drop 会触发
    /// "Cannot drop a runtime in a context where blocking is not allowed" panic。
    ///
    /// 当前 download 路径仍用这个反模式 — 这个测试现在应 panic（#[ignore] 避免
    /// 影响常规 cargo test）。验证修复方向：等 download 路径迁到 async reqwest
    /// 后，这个 panic 应该消失。
    #[test]
    #[ignore = "真实网络 + 反模式；cargo test -- --ignored 跑"]
    fn download_blocking_client_real_request_in_spawn_blocking_panics() {
        let rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .thread_name("so-novel-rt-test")
                .build()
                .unwrap(),
        );
        let cfg = AppConfig::default();

        rt.block_on(async {
            let _ = tokio::task::spawn_blocking(move || {
                let client = crate::http::client::build_blocking_client(
                    &cfg,
                    &crate::http::client::ClientOptions::default(),
                )
                .unwrap();
                // 实际发请求：触发 reqwest inner thread 完成启动
                let _ = client
                    .get("https://example.com/")
                    .timeout(std::time::Duration::from_secs(5))
                    .send();
                // drop 在 spawn_blocking 工作线程上发生
                drop(client);
            })
            .await;
        });

        // 给 reqwest inner thread 一点时间跑完它的 shutdown
        std::thread::sleep(std::time::Duration::from_millis(200));
        drop(rt);
    }
}
