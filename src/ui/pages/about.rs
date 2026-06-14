use crate::app::SoNovelApp;

pub fn show(ui: &mut egui::Ui, _app: &mut SoNovelApp) {
    ui.heading("关于");
    ui.add_space(8.0);
    ui.label(format!(
        "So Novel — Rust + egui 客户端 v{}",
        env!("CARGO_PKG_VERSION")
    ));
    ui.add_space(8.0);
    ui.label("从 Java 项目（v1.10.3）分阶段迁移至 Rust。当前阶段 1：");
    ui.label("• 工程骨架、数据模型、config.toml 兼容、规则加载已完成。");
    ui.label("• 搜索 / 解析 / 下载 / 导出尚未实现，详见审计文档。");
    ui.add_space(12.0);
    ui.hyperlink_to(
        "迁移审计文档（docs/rust-egui-migration-audit.md）",
        "https://github.com/freeok/so-novel",
    );
    ui.add_space(8.0);
    ui.hyperlink_to("原 Java 项目主页", "https://github.com/freeok/so-novel");
    ui.add_space(16.0);
    ui.label(
        egui::RichText::new(
            "本客户端默认不进行任何客户端遥测或上报。\n\
             （Java 版的 ClientReport 上报功能在 Rust 版中不会被迁移。）",
        )
        .small()
        .weak(),
    );
}
