//! 设置项通用布局。

/// 设置项通用布局：左 vertical（标题 + 副标题），右 [附加控件]。
///
/// `add_right` 闭包由调用方塞入具体控件（TextEdit / DragValue / ComboBox…）。
pub fn settings_row(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: Option<&str>,
    add_right: impl FnOnce(&mut egui::Ui),
) {
    const ROW_MIN_HEIGHT: f32 = 40.0;
    const ROW_H_PAD: f32 = 16.0;

    ui.horizontal(|ui| {
        ui.set_min_height(ROW_MIN_HEIGHT);
        ui.add_space(ROW_H_PAD);

        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width() - ROW_H_PAD, 0.0).max(egui::vec2(0.0, 0.0)),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_height(ROW_MIN_HEIGHT);
                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(title).strong());
                        if let Some(sub) = subtitle {
                            ui.label(egui::RichText::new(sub).small().weak());
                        }
                    });
                });
            },
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(ROW_H_PAD);
            add_right(ui);
        });
    });
}
