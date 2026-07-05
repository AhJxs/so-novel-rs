//! Shared lock helpers for web handlers.
//!
//! Web handlers hit `Arc<WebState>` fields (`config`, `rules`, `tasks`, ...)
//! behind `Mutex`/`RwLock`. Poisoned locks are a real risk: if any task panics
//! while holding one, every subsequent `lock()` returns `Err(PoisonError)`.
//! Raw `.unwrap()` there would panic the request worker and propagate as
//! connection drop on the client side -- unhelpful and noisy.
//!
//! These helpers convert that into a stable 500 response with `tracing::error!`
//! so on-call sees the panic root cause without the request just dying.
//!
//! Borrow lifetime tracking is preserved (the returned guard is bound to `&mtx`,
//! not the helper stack), so callers can `?`-propagate inside async handlers
//! without `let` chains.

use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Error returned to the HTTP layer when a lock is poisoned. Stable string so
/// clients / frontend can match without locale surprise.
pub(crate) const LOCK_ERR_MSG: &str = "内部状态不可用";

pub(crate) fn mutex<'a, T>(
    label: &str,
    mtx: &'a Mutex<T>,
) -> Result<MutexGuard<'a, T>, (axum::http::StatusCode, String)> {
    mtx.lock().map_err(|e| {
        tracing::error!("{label}: Mutex poisoned: {e}");
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            LOCK_ERR_MSG.to_string(),
        )
    })
}

pub(crate) fn rw_read<'a, T>(
    label: &str,
    lk: &'a RwLock<T>,
) -> Result<RwLockReadGuard<'a, T>, (axum::http::StatusCode, String)> {
    lk.read().map_err(|e| {
        tracing::error!("{label}: RwLock read poisoned: {e}");
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            LOCK_ERR_MSG.to_string(),
        )
    })
}

pub(crate) fn rw_write<'a, T>(
    label: &str,
    lk: &'a RwLock<T>,
) -> Result<RwLockWriteGuard<'a, T>, (axum::http::StatusCode, String)> {
    lk.write().map_err(|e| {
        tracing::error!("{label}: RwLock write poisoned: {e}");
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            LOCK_ERR_MSG.to_string(),
        )
    })
}
