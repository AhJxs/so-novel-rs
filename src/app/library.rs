//! `AppModel` 本地书库方法
//!
//! 3 个方法: `refresh_library` / `refresh_library_async` / `delete_library_entry`.

use std::path::{Path, PathBuf};

use super::{AppModel, ops};

impl AppModel {
    /// 同步扫描下载目录。
    pub fn refresh_library(&mut self) {
        ops::refresh_library(&mut self.library, &self.config.download.download_path);
    }

    /// 异步扫描下载目录。
    ///
    /// 阻塞的 `read_dir` / `metadata` 跑在 `tokio::task::spawn_blocking` (共享 tokio
    /// runtime), 结果通过 smol channel 回到主线程, 由 `events::drain` 排空。
    /// 重复触发会被 `scan_in_flight` 拦截。`scanned_dir` 路径解析在主线程做 (轻量)。
    pub fn refresh_library_async(&mut self) {
        if self.library.scan_in_flight {
            return;
        }
        let dir_raw = PathBuf::from(&self.config.download.download_path);
        let abs = if dir_raw.is_absolute() {
            dir_raw
        } else {
            match std::env::current_dir() {
                Ok(cwd) => cwd.join(&dir_raw),
                Err(e) => {
                    tracing::warn!("获取当前目录失败: {e:#}, 使用原始路径");
                    dir_raw
                }
            }
        };

        // 先在主线程重置 entries / scanned_dir / pending_delete (轻量、即时反馈)。
        self.library.scanned_dir = Some(abs.clone());
        self.library.entries.clear();
        self.library.last_error = None;
        self.library.pending_delete = None;

        if !abs.exists() {
            return;
        }

        let (tx, rx) = smol::channel::unbounded::<crate::app::library_state::LibraryScanEvent>();
        self.library.scan_rx = Some(rx);
        self.library.scan_in_flight = true;

        // 借用 self.runtime 启动 spawn_blocking, 调用阻塞的 std::fs — 共享 tokio
        // runtime (已 leaked), 进程结束才 drop。
        let runtime = self.runtime;
        runtime.spawn(async move {
            // tokio 的 spawn_blocking 隔离阻塞 IO, 不阻塞 reactor。
            let result = tokio::task::spawn_blocking(move || {
                crate::app::library_state::scan_library_dir(&abs)
            })
            .await;
            let event = match result {
                Ok(Ok(entries)) => Ok(entries),
                Ok(Err(io_err)) => {
                    let i18n_msg = crate::i18n::ts_fmt(
                        "Toasts.library_scan_failed",
                        &[("err", &io_err.to_string())],
                    )
                    .to_string();
                    Err(crate::error::AppError::internal(i18n_msg))
                }
                Err(join_err) => Err(crate::error::AppError::internal(format!(
                    "scan task join failed: {join_err}"
                ))),
            };
            // receiver 可能已被 drop (AppModel 销毁) — send 在 channel 关闭时
            // 静默失败, 符合"没人听就不发"原则。
            let _ = tx.send(event).await;
        });
    }

    /// 真正删除一个本地文件。
    pub fn delete_library_entry(&mut self, path: &Path) {
        match ops::delete_library_entry(
            &mut self.library,
            &self.config.download.download_path,
            path,
        ) {
            Ok(msg) if !msg.is_empty() => self.push_success(msg),
            Ok(_) => {}
            Err(e) => self.push_error(e.message()),
        }
    }
}
