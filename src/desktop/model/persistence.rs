//! `AppModel` 持久化方法 (`save_sources_config` / `persist_settings` / `persist_tasks`)。
//!
//! 任务落盘 (`tasks.json`) 统一走 [`AppModel::persist_tasks`]：UI 线程只序列化，
//! 写盘丢给 blocking pool，调用方拿到 fire-and-forget 语义。

use super::AppModel;

impl AppModel {
    /// 把书源选择状态写回 `sources_config.json`；失败仅记日志。
    pub fn save_sources_config(&self) {
        if let Err(e) = self.sources_config.save(&self.paths.sources_config) {
            tracing::warn!("保存书源配置失败: {e:#}");
        }
    }

    /// 把当前 config 写回 config.toml。
    ///
    /// **Auto-save 模式**: 每个 setter 改完字段后立即调本方法, 没有"立即保存"按钮、
    /// 没有 dirty 概念。成功静默, 失败弹 error notification。单次写盘只有几 ms,
    /// 暂不做 debounce。
    pub fn persist_settings(&mut self) {
        if let Err(e) = crate::desktop::model::ops::settings::persist_settings(
            &self.config,
            &self.paths.config_file,
        ) {
            let msg = e.message();
            tracing::warn!("自动保存 config.toml 失败: {msg}");
            self.push_error(msg);
            return;
        }
        tracing::debug!("config.toml 自动保存成功");

        // proxy / unsafe_ssl 改了 → 重建共享 HTTP client。`rebuild_proxy` 按
        // **解析后的代理 URL** 比对（见 `HttpClients::rebuild_proxy`），未变即 no-op。
        // 重建失败时 config 已写盘但客户端还是旧配置, 推 error 让用户知道。
        if let Err(e) = self.http.rebuild_proxy(&self.config) {
            let msg = format!("HTTP client 重建失败（配置已保存）: {e}");
            tracing::warn!("{msg}");
            self.push_error(msg);
        }
    }

    /// 把当前任务列表 fire-and-forget 落盘。
    ///
    /// 序列化（含超额完成任务的裁剪）留在调用线程，避免把整张 `DownloadTask` 表
    /// clone 进 blocking pool；写盘本身仍是异步的，失败只记日志。
    pub fn persist_tasks(&self) {
        let path = self.paths.tasks_file.clone();
        let json = match crate::db::tasks_json_with_trim(&self.tasks) {
            Ok(json) => json,
            Err(e) => {
                tracing::warn!("序列化任务列表失败: {e:#}");
                return;
            }
        };
        self.runtime.spawn_blocking(move || {
            if let Err(e) = crate::db::write_json(&path, &json) {
                tracing::warn!("保存任务到文件失败: {e:#}");
            }
        });
    }
}
