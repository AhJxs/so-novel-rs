//! 各页面的 UI。
//!
//! 阶段 4b 之后：搜索 / 任务 / 书库 / 书源 / 设置 全部已为真实功能页。
//!
//! 所有页面在 dispatcher 里**统一包一层 8px inner_margin**，避免每个页面自己
//! 加 padding 导致不一致；CentralPanel 的 outer_margin 已经在 `theme::content_frame`
//! 里给了 8px 外边距，再加这个 inner 8 就形成"窗口边缘 → 内容"共 16px 的呼吸感。

mod library;
mod search;
mod settings;
mod sources;
mod tasks;

use crate::app::SoNovelApp;
use crate::ui::nav::NavPage;

pub fn show(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    egui::Frame::new()
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| match app.current_page {
            NavPage::Search => search::show(ui, app),
            NavPage::Tasks => tasks::show(ui, app),
            NavPage::Library => library::show(ui, app),
            NavPage::Sources => sources::show(ui, app),
            NavPage::Settings => settings::show(ui, app),
        });
}
