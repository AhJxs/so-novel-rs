//! 书源管理页状态：连通性检测的结果与运行标记。

use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::crawler::health::SourceHealth;
use crate::models::Rule;

/// 书源过滤状态（持久化在 `SourcesState` 里，跟 `LibraryState.filter_text` 同模式）。
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
    /// `source_id` → 探测结果（按到达顺序覆盖；不要求全部都到齐）。
    pub health: HashMap<i32, SourceHealth>,
    /// 是否正在跑探测（true 时禁用按钮 + 显示 spinner）。
    pub running: bool,
    /// 总共要等多少源；用于 UI 显示 "M/N 已返回"。
    pub expected: usize,
    pub received: usize,
    /// 后台推送的接收端，update 循环 drain。
    pub rx: Option<mpsc::UnboundedReceiver<SourceHealth>>,
    /// 名称 / URL 关键字过滤（不区分大小写，子串匹配）。
    pub filter_text: String,
    /// 状态过滤（全部 / 启用 / 禁用）。
    pub filter_status: SourcesFilterStatus,
}

impl SourcesState {
    /// 清除所有测速状态（书源更新后调用）。
    pub fn clear_health(&mut self) {
        self.health.clear();
        self.received = 0;
        self.expected = 0;
        self.running = false;
        self.rx = None;
    }

    /// 排空通道；返回是否产生过事件（触发 repaint）。
    pub fn drain(&mut self) -> bool {
        let Some(rx) = self.rx.as_mut() else {
            return false;
        };
        let mut any = false;
        loop {
            match rx.try_recv() {
                Ok(h) => {
                    any = true;
                    self.received += 1;
                    self.health.insert(h.source_id, h);
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.rx = None;
                    self.running = false;
                    break;
                }
            }
        }
        if self.expected > 0 && self.received >= self.expected {
            self.running = false;
            self.rx = None;
        }
        any
    }

    /// 应用当前过滤（`filter_text` + `filter_status`）到传入的 rules 列表，返回克隆后的 Vec。
    ///
    /// 跟 `LibraryState::filtered_entries`（`app/library_state.rs`）同模式：
    ///   - 不修改 self，不修改传入的 rules
    ///   - 返回 owned Vec 方便 caller 排序 / 分页
    ///   - 不过滤 status：直接读 `self.filter_status`
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
                r.name.to_lowercase().contains(&kw) || r.url.to_lowercase().contains(&kw)
            })
            .cloned()
            .collect();
        // 按 id 升序，跟原版一致（id 是加载时分配的自增主键）。
        out.sort_by_key(|r| r.id);
        out
    }
}
