//! 共享的 tokio runtime：leak 后得到 `&'static Runtime`，永不 drop，
//! 彻底规避 "Cannot drop a runtime in a context where blocking is not allowed"
//! panic（即便 eframe 退出 / 某些边界场景下 runtime 在 worker 线程上 drop）。
//!
//! 进程退出时 OS 自动回收所有线程与内存，所以 leak 不影响清理。

use anyhow::{Context, Result};
use tokio::runtime::Runtime;

/// 构造共享的多线程 tokio runtime 并 leak 成 `&'static`。
///
/// `tokio::runtime::Builder::build()` 失败的唯一现实场景是系统资源耗尽
/// （内存 / fd / thread 上限），此时 app 必然跑不起来 — 错误沿 `Result` 冒到
/// `AppModel::new` 的初始化失败分支，让 UI 入口弹致命错误对话框，而不是 panic。
pub fn build_shared_runtime() -> Result<&'static Runtime> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("so-novel-rt")
        .build()
        .context("构造 tokio runtime 失败（可能是内存或 fd 耗尽）")?;
    Ok(Box::leak(Box::new(rt)))
}
