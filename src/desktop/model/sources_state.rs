//! 书源管理页状态：连通性检测的结果与运行标记。

use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::core::sources as core_sources;
use crate::core::{DrainOutcome, try_drain_all};
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

#[derive(Default)]
pub struct SourcesState {
    /// `source_id` → 探测结果（按到达顺序覆盖, 不要求全部到齐）。
    pub health: HashMap<i32, SourceHealth>,
    /// 是否正在跑探测（true 时禁用按钮 + 显示 spinner）。
    pub running: bool,
    pub expected: usize,
    pub received: usize,
    /// 后台推送的接收端, 由 `drain` 排空。
    pub rx: Option<mpsc::UnboundedReceiver<SourceHealth>>,
    pub filter_text: String,
    pub filter_status: SourcesFilterStatus,
}

impl SourcesState {
    pub fn clear_health(&mut self) {
        self.health.clear();
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
            self.health.insert(h.source_id, h);
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

    /// 应用 `filter_text` + `filter_status` 过滤 rules, 返回克隆后的 Vec。
    ///
    /// 不改 self、不改传入的 rules, 返回 owned Vec 方便 caller 排序 / 分页。
    pub fn filtered_rules(&self, rules: &[Rule]) -> Vec<Rule> {
        let kw = self.filter_text.trim().to_lowercase();
        let mut out: Vec<Rule> = rules
            .iter()
            .filter(|r| match self.filter_status {
                SourcesFilterStatus::All => true,
                SourcesFilterStatus::Enabled => !r.disabled,
                SourcesFilterStatus::Disabled => r.disabled,
            })
            .filter(|r| {
                if kw.is_empty() {
                    return true;
                }
                r.name.to_lowercase().contains(&kw) || core_sources::rule_key(r).contains(&kw)
            })
            .cloned()
            .collect();
        // 按 id 升序（id 是加载时分配的自增主键）。
        out.sort_by_key(|r| r.id);
        out
    }
}
