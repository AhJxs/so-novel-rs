//! `AppModel` 持久化方法
//!
//! 2 个方法: `save_sources_config` / `persist_settings`.
//! 不依赖 `ops`, 直接调 `crate::db` / `crate::config::save_config`.
//!
//! 任务落盘 (`tasks.json`) 不在这里 —— 调用方直接走
//! `self.runtime.spawn_blocking(move || crate::db::save_with_trim(&path, &tasks))`
//! 拿到 fire-and-forget 语义, 不需要绕一圈 impl method。

use super::AppModel;

impl AppModel {
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
