//! `AppModel` 持久化方法 (`save_sources_config` / `persist_settings`)。
//!
//! 任务落盘 (`tasks.json`) 不在这里 —— 调用方直接 `spawn_blocking` + `db::save_with_trim`
//! 拿 fire-and-forget 语义, 不需要绕一圈 impl method。

use super::AppModel;

impl AppModel {
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
}
