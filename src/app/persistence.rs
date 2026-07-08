//! `AppModel` 持久化方法
//!
//! 3 个方法: `save_tasks_to_file` / `save_sources_config` / `persist_settings`.
//! 不依赖 `ops`, 直接调 `crate::db` / `crate::config::save_config`.

use super::AppModel;

impl AppModel {
    /// 保存所有任务到文件, 并自动清理超额的已完成任务。
    ///
    /// 性能改造: 原实现把所有 `std::fs::*` + `fsync` 同步跑在调用线程上
    /// (通常是 UI 线程 / `drain_loop`), 下载密集时一次 fsync 可让 UI 顿
    /// 几十毫秒。现改为:
    /// 1. UI 线程: 把 `tasks` 转为 `DownloadTaskRecord` Vec, clone 出来
    ///    (仅 `origin` 里有 `String` / 路径, 深拷贝代价不大);
    /// 2. `runtime.spawn(spawn_blocking(...))` 把 trim + write + fsync + rename
    ///    派到 tokio blocking pool;
    /// 3. UI 线程**不**等待结果 — 失败只 warn; 下一次保存会覆盖同一文件,
    ///    数据丢失最坏情况是一次 (tasks.json 偶尔丢一次不致命, tasks
    ///    持久化本来就是 best-effort)。
    ///
    /// **不等待结果**的副作用: 本次 trim 删除的记录**下一次** `drain` 才同步
    /// 清理 `self.tasks`。换言之, UI 上可能多显示若干被 trim 的旧任务一帧。
    /// 这个权衡是值得的 — 顿 UI 比显示一帧 stale 数据更糟。
    pub fn save_tasks_to_file(&mut self) {
        // 1. UI 线程: 构造 record Vec (clone) 并 spawn 异步保存。
        let records: Vec<crate::models::DownloadTaskRecord> = self
            .tasks
            .iter()
            .map(super::download_task::DownloadTask::to_record)
            .collect();
        let path = self.paths.tasks_file.clone();

        self.runtime.spawn(async move {
            // 2. blocking pool: trim + write + fsync + rename。
            //    失败仅 warn — tasks.json 的丢失可由下一次保存覆盖。
            let mut records = records;
            if let Err(e) = crate::db::save_with_trim(&path, &mut records) {
                tracing::warn!("保存任务到文件失败: {e:#}");
                return;
            }
            // 保存成功: trim 已就地修改 records (删除了超额项)。下一次 drain
            // 不会重做这一步 — 我们**不再**回写 record_ids 到 self.tasks (已
            // 在 spawn 里跑过, AppModel 拿不到引用)。这是"best-effort 持久化"
            // 语义: 丢失的旧任务在内存里再多留一会儿, 下次手动操作 (删除任务等)
            // 会自然清理。
            let trimmed = records.len();
            tracing::debug!("tasks.json 持久化成功, 保留 {trimmed} 条记录");
        });
        // 立即返回, UI 线程不阻塞。
    }

    /// 保存书源配置到文件。
    pub fn save_sources_config(&self) {
        if let Err(e) = self.sources_config.save(&self.paths.sources_config) {
            tracing::warn!("保存书源配置失败: {e:#}");
        }
    }

    /// 把当前 config 写回 config.toml。
    ///
    /// **Auto-save 模式**: 每个 setter 改完字段后立即调本方法写盘 — 没有"立即保存"
    /// 按钮, 没有 dirty 概念。成功时静默 (`tracing::debug` 留痕), 失败时弹 error
    /// notification 让用户知道 (极少见: 磁盘满 / 权限问题 / 路径不存在等)。
    ///
    /// 每个 setter 改完字段后立即调本方法 (auto-save), 无需「立即保存」按钮。
    /// 如果以后想加 debounce 写入 (比如连续拖动 number input), 可以在 setter 里加
    /// cx.spawn(timer 500ms) 合并多次 `persist_settings` 调用 — 单次写盘本来就很快
    /// (小 TOML 几 ms), 目前不做 debounce。
    pub fn persist_settings(&mut self) {
        if let Err(e) =
            crate::app::ops::settings::persist_settings(&self.config, &self.paths.config_file)
        {
            let msg = e.message();
            tracing::warn!("自动保存 config.toml 失败: {msg}");
            self.push_error(msg);
            return;
        }
        tracing::debug!("config.toml 自动保存成功");

        // proxy / unsafe_ssl 改了 → 重建共享 HTTP client。
        // `rebuild_proxy` 内部按 `(proxy_enabled, proxy_host, proxy_port)` 三元组
        // 比对, 未变则 no-op; 其它字段 (theme / language / timeout 等) 不触发。
        // 重建失败: config 已写盘但客户端拿的是旧配置 → 推 error 让用户知道;
        // 下次重启 / 再次触发 persist_settings 会重试。
        if let Err(e) = self.http.rebuild_proxy(&self.config) {
            let msg = format!("HTTP client 重建失败（配置已保存）: {e}");
            tracing::warn!("{msg}");
            self.push_error(msg);
        }
    }
}
