//! 共享的 tokio runtime：leak 后得到 `&'static Runtime`，永不 drop，
//! 彻底规避 "Cannot drop a runtime in a context where blocking is not allowed"
//! panic（即便 eframe 退出 / 某些边界场景下 runtime 在 worker 线程上 drop）。
//!
//! 进程退出时 OS 自动回收所有线程与内存，所以 leak 不影响清理。

use tokio::runtime::Runtime;

pub fn build_shared_runtime() -> &'static Runtime {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("so-novel-rt")
        .build()
        .expect("build tokio runtime");
    Box::leak(Box::new(rt))
}
