//! `AppModel` 持久化方法 (`save_sources_config` / `persist_settings` / `persist_tasks`)。
//!
//! 书源选择与下载任务写进运行数据存储（`~/.sonovel/state`），`config.toml` 仍是文件。
//! 存储常驻打开、单次提交约亚毫秒级，所以两者都在 UI 线程同步写，返回即已 fsync。

use super::AppModel;

impl AppModel {
    /// 把书源选择状态写回运行数据；失败仅记日志。
    ///
    /// 加载时 blob 损坏（原文已备份）则本次运行拒写，只告警一次。
    pub fn save_sources_config(&mut self) {
        if self.sources_readonly {
            if !self.sources_readonly_warned {
                self.sources_readonly_warned = true;
                tracing::warn!("书源选择数据加载时已损坏（原文已备份），本次运行不再覆盖写入");
            }
            return;
        }
        let bytes = match self.sources_config.to_json() {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::warn!("序列化书源配置失败: {error:#}");
                return;
            }
        };
        if let Err(error) = self.state.put(crate::db::KEY_SOURCES, &bytes) {
            tracing::warn!("保存书源配置失败: {error:#}");
        }
    }

    /// 把当前 config 写回 config.toml。
    ///
    /// **Auto-save 模式**: 没有"立即保存"按钮、没有 dirty 概念。成功静默, 失败弹
    /// error notification。
    ///
    /// 调用时机: 设置页的下载目录 / Cookie 输入与字号滑块由页面按 400ms 合并写
    /// （`SettingsPage::schedule_persist`, 回车 / 松开滑块 / 退出时立即 flush）;
    /// 其余 setter（开关、主题选择等）低频且语义明确, 改完立即调本方法。
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

    /// 把当前任务列表写进运行数据。
    ///
    /// 序列化（含超额完成任务的裁剪）与写入都在调用线程完成：提交前已 fsync，
    /// 调用返回后数据一定在磁盘上，退出时不需要额外的 flush。
    ///
    /// 加载时 blob 损坏（原文已备份）则本次运行拒写，只告警一次。
    pub fn persist_tasks(&mut self) {
        if self.tasks_readonly {
            if !self.tasks_readonly_warned {
                self.tasks_readonly_warned = true;
                tracing::warn!("下载任务数据加载时已损坏（原文已备份），本次运行不再覆盖写入");
            }
            return;
        }
        let json = match crate::db::tasks_json_with_trim(&self.tasks) {
            Ok(json) => json,
            Err(e) => {
                tracing::warn!("序列化任务列表失败: {e:#}");
                return;
            }
        };
        if let Err(error) = self.state.put(crate::db::KEY_TASKS, json.as_bytes()) {
            tracing::warn!("保存下载任务失败: {error:#}");
        }
    }
}
