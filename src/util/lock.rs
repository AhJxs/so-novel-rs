//! 通用锁 poison 防护。
//!
//! 设计：把"锁被 poison 时不 panic，返 Result"的模式抽出来，让所有需要长寿命
//! daemon / 服务的模块（web / http / gpui）统一用一套。
//!
//! 两种调用形态：
//! - `lock_or_log!`（宏版）：用于 fire-and-forget 场景（拿不到就 warn + 走默认）
//! - `lock_or_err!`（宏版）：用于业务关键路径（拿不到就返错让上层处理）
//!
//! 两个 web handler 已经在用 `src/web/handlers/lock.rs` —— 那是 axum 专用的
//! `(StatusCode, String)` 返错形态，本模块的 helper 是"通用"版本，被 http/clients
//! 这类**不**直接走 axum 但仍要防 poison panic 的模块使用。

use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// 拿 `Mutex` 锁。poisoned 时记录 `tracing::error!` 并返 `Err(message)`。
///
/// 调用方决定 `Err` 怎么处理 —— 上层模块（web / http）通常把 `Err` 包装成
/// HTTP 5xx 或业务 error enum。
pub fn mutex_or<'a, T>(label: &str, mtx: &'a Mutex<T>) -> Result<MutexGuard<'a, T>, String> {
    mtx.lock().map_err(|e| {
        tracing::error!("{label}: Mutex poisoned: {e}");
        format!("{label} lock poisoned")
    })
}

/// `RwLock` 读锁版本。
pub fn rw_read_or<'a, T>(label: &str, lk: &'a RwLock<T>) -> Result<RwLockReadGuard<'a, T>, String> {
    lk.read().map_err(|e| {
        tracing::error!("{label}: RwLock read poisoned: {e}");
        format!("{label} read lock poisoned")
    })
}

/// `RwLock` 写锁版本。
pub fn rw_write_or<'a, T>(
    label: &str,
    lk: &'a RwLock<T>,
) -> Result<RwLockWriteGuard<'a, T>, String> {
    lk.write().map_err(|e| {
        tracing::error!("{label}: RwLock write poisoned: {e}");
        format!("{label} write lock poisoned")
    })
}
