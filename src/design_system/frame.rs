//! 面板 / 导航 / 标题栏 frame 工厂。

/// 顶部导航条 frame：底边 1px 分隔线 + 上下 padding。
pub fn nav_frame(style: &egui::Style) -> egui::Frame {
    let visuals = &style.visuals;
    egui::Frame::new()
        .fill(visuals.panel_fill)
        .inner_margin(egui::Margin::symmetric(0, 4))
        .stroke(egui::Stroke::new(
            1.0,
            visuals.widgets.noninteractive.bg_stroke.color,
        ))
}

/// 顶部标题栏（窗口控制按钮所在）frame：无内外边距，紧贴顶部。
pub fn title_bar_frame(style: &egui::Style) -> egui::Frame {
    let visuals = &style.visuals;
    egui::Frame::new()
        .fill(visuals.panel_fill)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .outer_margin(egui::Margin::ZERO)
}

/// 主面板通用 padding frame：8px 内边距 + `visuals.panel_fill` 背景。
pub fn content_frame(visuals: &egui::Visuals) -> egui::Frame {
    egui::Frame::new()
        .fill(visuals.panel_fill)
        .inner_margin(egui::Margin::same(8))
}
