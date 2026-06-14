//! 书源管理页状态：连通性检测的结果与运行标记。

use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::crawler::health::SourceHealth;

#[derive(Default)]
pub struct SourcesState {
    /// source_id → 探测结果（按到达顺序覆盖；不要求全部都到齐）。
    pub health: HashMap<i32, SourceHealth>,
    /// 是否正在跑探测（true 时禁用按钮 + 显示 spinner）。
    pub running: bool,
    /// 总共要等多少源；用于 UI 显示 "M/N 已返回"。
    pub expected: usize,
    pub received: usize,
    /// 后台推送的接收端，update 循环 drain。
    pub rx: Option<mpsc::UnboundedReceiver<SourceHealth>>,
    /// 二次删除确认：UI 点了一次「删除」后存它的 id；再点「确认删除」才真正删。
    /// 与 library 卡片的 `pending_delete: Option<PathBuf>` 同模式。
    pub pending_delete: Option<i32>,
}

impl SourcesState {
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
}
