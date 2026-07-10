//! 全局追踪 ID：一次搜索/下载/详情拉取/TOC 预取 = 一个 `TraceId`。
//!
//! 设计目标：
//! - **最小侵入**：通过 `tracing` span 跨 `.await` 传播，函数签名零改动。
//! - **可 grep**：每个顶层操作 mint 一个 `u64`；日志文件里 `trace_id=42` 即可
//!   还原一次完整调用的全部阶段。
//! - **细粒度子事件**用 `sub=` 字段表达（`sub=chapter:142` / `sub=source:5`），
//!   不另起 `trace_id，保持父子关系简单`。
//!
//! 调用入口在 `app/ops/search.rs` / `app/ops/download.rs` 的 4 个 `spawn_*` 处。
//! `#[tracing::instrument]` 在 crawler / parser 入口处接管，把 `trace_id` 透传
//! 给所有 `tracing::info!/warn!/error!` 调用 —— 无需把 `TraceId` 加到任何函数签名里。

use std::sync::atomic::{AtomicU64, Ordering};

/// 单调递增的全局 ID 源。从 1 开始（0 保留为"未分配"哨兵，理论不会被用到）。
static NEXT: AtomicU64 = AtomicU64::new(1);

/// 单次顶层操作的追踪 ID。
///
/// 复制成本 = 8 字节（`Copy`），可在 `.instrument(span)` 之间随意 clone。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraceId(u64);

impl TraceId {
    /// 分配一个新的全局唯一 ID。线程安全，单调递增。
    pub(crate) fn mint() -> Self {
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    #[cfg(test)]
    pub(crate) const fn raw(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for TraceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<u64> for TraceId {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

/// `sub=` 字段的常用字面量。集中放这里方便改、避免散落的字符串。
///
/// 搜索：每源完成时 `sub = format!("source:{id}", id = source_id)`。
/// 下载：章节失败时 `sub = format!("chapter:{order}")`。
pub mod sub {
    pub const SEARCH: &str = "search";
    pub const DETAIL: &str = "detail";
    pub const TOC: &str = "toc";
    pub const DOWNLOAD: &str = "download";
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::sync::Barrier;
    use std::thread;

    #[test]
    fn mint_is_monotonic() {
        let a = TraceId::mint();
        let b = TraceId::mint();
        let c = TraceId::mint();
        assert!(a.raw() < b.raw());
        assert!(b.raw() < c.raw());
    }

    #[test]
    fn mint_is_unique_across_threads() {
        // 16 个线程各 mint 1000 个 id，合计 16000 个应全部不重复。
        let barrier = Arc::new(Barrier::new(16));
        let mut handles = Vec::new();
        for _ in 0..16 {
            let b = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                b.wait();
                let mut ids = Vec::with_capacity(1000);
                for _ in 0..1000 {
                    ids.push(TraceId::mint().raw());
                }
                ids
            }));
        }
        let mut all = HashSet::new();
        for h in handles {
            for id in h.join().unwrap() {
                assert!(all.insert(id), "duplicate id {id}");
            }
        }
        assert_eq!(all.len(), 16 * 1000);
    }

    #[test]
    fn display_and_eq() {
        let id = TraceId::from(42u64);
        assert_eq!(id.to_string(), "42");
        assert_eq!(id, TraceId::from(42u64));
        assert_ne!(id, TraceId::from(43u64));
    }

    /// 端到端验证：mint 一个 `trace_id，挂到` span 上，
    /// 在 span 内部用 `tracing::info!` 触发事件，
    /// 校验 `trace_id=N` 出现在事件字段里 —— 这是 grep 流程的核心假设。
    #[test]
    fn trace_id_appears_in_event_fields() {
        use std::sync::{Arc, Mutex};
        use tracing_subscriber::fmt::MakeWriter;

        // 简单的 in-memory writer，把每行 capture 起来。
        #[derive(Clone, Default)]
        struct Capture(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Capture {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl<'a> MakeWriter<'a> for Capture {
            type Writer = Self;
            fn make_writer(&'a self) -> Self::Writer {
                self.clone()
            }
        }
        let cap = Capture::default();

        // 每个测试运行独立的 subscriber — 用 `set_default` 让它只对当前线程生效。
        let subscriber = tracing_subscriber::fmt()
            .with_writer(cap.clone())
            .with_ansi(false)
            .with_target(false)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let id = TraceId::mint();
        let span = tracing::info_span!("smoke", trace_id = %id, keyword = "test");
        let _enter = span.enter();
        tracing::info!("hello from inside span");

        let buf = cap.0.lock().unwrap();
        let s = String::from_utf8_lossy(&buf);
        // 校验 trace_id 出现在 capture 的输出里 —— 完整断言字段格式比较脆，
        // 这里只检查"trace_id=<数字>"这个 token 在日志里。
        let needle = format!("trace_id={}", id.raw());
        let contains_needle = s.contains(&needle);
        let msg = format!("expected log to contain {needle:?}, got:\n{s}");
        drop(buf);
        assert!(contains_needle, "{msg}");
    }
}
