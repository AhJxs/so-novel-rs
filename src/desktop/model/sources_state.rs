//! 书源管理页状态：连通性检测的结果与运行标记。

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::core::{DrainOutcome, sources as core_sources, try_drain_all};
use crate::crawler::health::SourceHealth;
use crate::models::Rule;

/// 书源过滤状态（持久化在 `SourcesState` 里）。
#[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
pub enum SourcesFilterStatus {
    /// 不过滤（默认）。
    #[default]
    All,
    /// 只看 `disabled == false` 的源。
    Enabled,
    /// 只看 `disabled == true` 的源。
    Disabled,
}

/// 书源管理页状态：探测结果与过滤条件。
#[derive(Default)]
pub struct SourcesState {
    /// `source_id` → 探测结果（按到达顺序覆盖, 不要求全部到齐）。
    /// `Arc` 快照：渲染时整体 clone 只是 refcount bump，drain 时 `make_mut` 写回。
    pub health: Arc<HashMap<i32, SourceHealth>>,
    /// 是否正在跑探测（true 时禁用按钮 + 显示 spinner）。
    pub running: bool,
    /// 本轮预计探测的书源数。
    pub expected: usize,
    /// 已收到探测结果的书源数。
    pub received: usize,
    /// 后台推送的接收端, 由 `drain` 排空。
    pub rx: Option<mpsc::UnboundedReceiver<SourceHealth>>,
    /// 书源名过滤关键词。
    pub filter_text: String,
    /// 按健康状态过滤的选项。
    pub filter_status: SourcesFilterStatus,
}

impl SourcesState {
    /// 清空探测结果与进度计数，回到未探测状态。
    pub fn clear_health(&mut self) {
        Arc::make_mut(&mut self.health).clear();
        self.received = 0;
        self.expected = 0;
        self.running = false;
        self.rx = None;
    }

    /// 排空通道；返回是否产生过事件（触发 repaint）。
    pub fn drain(&mut self) -> bool {
        let mut any = false;
        let outcome = try_drain_all(&mut self.rx, |h: SourceHealth| {
            any = true;
            self.received += 1;
            Arc::make_mut(&mut self.health).insert(h.source_id, h);
        });
        if matches!(outcome, DrainOutcome::Disconnected) {
            self.running = false;
        }
        if self.expected > 0 && self.received >= self.expected {
            self.running = false;
            self.rx = None;
        }
        any
    }

    /// 应用 `filter_text` + `filter_status` 过滤 rules, 返回命中项在 `rules` 中的下标
    /// （按 id 升序）。
    ///
    /// 只返回下标、不复制 `Rule`：调用方按需对当前页的项做 `Arc::clone`。
    pub fn filtered_rule_indices(&self, rules: &[Arc<Rule>]) -> Vec<usize> {
        let kw = self.filter_text.trim().to_lowercase();
        let mut out: Vec<usize> = rules
            .iter()
            .enumerate()
            .filter(|(_, r)| match self.filter_status {
                SourcesFilterStatus::All => true,
                SourcesFilterStatus::Enabled => !r.disabled,
                SourcesFilterStatus::Disabled => r.disabled,
            })
            .filter(|(_, r)| {
                if kw.is_empty() {
                    return true;
                }
                r.name.to_lowercase().contains(&kw) || core_sources::rule_key(r).contains(&kw)
            })
            .map(|(i, _)| i)
            .collect();
        // 按 id 升序（id 是加载时分配的自增主键）。
        out.sort_by_key(|&i| rules[i].id);
        out
    }
}
