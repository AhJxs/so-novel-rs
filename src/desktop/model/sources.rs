//! `AppModel` 书源管理方法（切换禁用 / 导入 / 删除 / 切换活跃文件）。

use std::path::Path;

use crate::i18n::{ts, ts_fmt};

use super::{AppModel, ops};

impl AppModel {
    /// 切换书源禁用状态。
    pub fn toggle_source_disabled(&mut self, source_url: &str) {
        ops::toggle_source_disabled(&mut self.sources_config, &mut self.rules, source_url);
        self.sources_state.clear_health();
        self.save_sources_config();
    }

    /// 从 JSON 文件导入书源。自动复制文件到 `~/.sonovel/rules/`, 重名覆盖;
    /// toast 显示导入的文件名。
    pub fn add_sources_from_file(&mut self, path: &Path) {
        match ops::add_sources_from_file(
            &self.paths.rules_dir,
            &self.sources_config,
            &mut self.rules,
            &mut self.rule_load_error,
            path,
        ) {
            Ok(result) => {
                let msg =
                    crate::i18n::ts_fmt("Sources.import.result", &[("filename", &result.filename)])
                        .to_string();
                self.sources_state.clear_health();
                // 导入的就是当前活跃文件 → rule 集合已被重载, 旧结果的 `source_id`
                // 可能指向错源（同 `switch_active_file` 的清理逻辑）。
                if result.reloaded_active {
                    self.search.clear_results_and_caches();
                    self.list_cache.clear();
                }
                self.push_success(msg);
                self.save_sources_config();
            }
            Err(e) => {
                let msg = e.message();
                if msg.starts_with("文件内容为空") || msg.starts_with("文件中未找到有效")
                {
                    self.push_warning(msg);
                } else {
                    self.push_error(msg);
                }
            }
        }
    }

    /// 删除一条书源。
    pub fn delete_source(&mut self, source_url: &str) {
        match ops::delete_source(
            &self.paths.rules_dir,
            &self.sources_config,
            &mut self.rules,
            &mut self.sources_state,
            source_url,
        ) {
            Ok(true) => {
                self.push_success(ts_fmt("Toasts.delete_source_ok", &[("url", source_url)]));
            }
            Ok(false) => self.push_warning(ts("Toasts.delete_source_missing")),
            Err(e) => self.push_error(e.message()),
        }
    }

    /// 切换活跃书源文件。
    pub fn switch_active_file(&mut self, filename: &str) {
        match ops::switch_active_file(
            &self.paths.rules_dir,
            &mut self.sources_config,
            &mut self.rules,
            &mut self.rule_load_error,
            filename,
        ) {
            Ok(()) => {
                self.sources_state.clear_health();
                // rule 集合整体替换: 旧的 `source_id` 数值在新文件里可能指向完全
                // 不同的源, 清空避免用户点旧结果下载到错源。
                self.search.clear_results_and_caches();
                // 同理清 list_cache（也顺带避免 stale 占用）。
                self.list_cache.clear();
                self.push_success(ts_fmt(
                    "Toasts.switch_source_file_ok",
                    &[("filename", filename)],
                ));
                self.save_sources_config();
            }
            Err(e) => self.push_error(e.message()),
        }
    }
}
