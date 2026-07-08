//! `AppModel` 书源管理方法
//!
//! 4 个方法: 切换禁用 / 导入 / 删除 / 切换活跃文件。

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

    /// 从 JSON 文件导入书源。
    ///
    /// 自动复制文件到 `~/.sonovel/rules/`, 重名则覆盖。
    /// 反馈给用户的 toast 显示导入的文件名。
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
                // 如果导入的就是当前活跃文件, rule 集合已被重载: 旧搜索结果的
                // `source_id` 在新 rule 里可能指向错源 (详见 `switch_active_file`
                // 同款注释)。只清这一种情况 — 导入非活跃文件不影响 rule 集合。
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
                // rule 集合整体替换 → 旧搜索结果的 `source_id` 在新 rule 里
                // 可能指向完全不同的源 (数值 ID 在不同文件里不复用)。直接清空
                // 避免用户点了旧结果去下载, 结果跑到错源上。
                self.search.clear_results_and_caches();
                // 源切换会让所有 `*_version` 翻篇; 旧 cache entry 的 `data_version`
                // 必然对不上新值, 但清空更稳 (避免 stale 占用 + 缩小内存)。
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
