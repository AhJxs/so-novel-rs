//! Material Symbols Rounded 图标字体（Filled 变体）。
//!
//! 字体文件和 codepoints 来自 Google Material Symbols（vendor 在本目录下）。
//! 编译时 `include_bytes!` 嵌入字体数据，运行时通过 `initialize()` 注入 egui。

use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{Button, FontData, FontFamily, Frame, Response, RichText, Widget};

pub mod icons;

// =============================================================================
// Font data (filled only, no feature gate needed)
// =============================================================================

pub(crate) const FONT_DATA: &[u8] = include_bytes!("MaterialSymbolsRounded_Filled-Regular.ttf");

// =============================================================================
// Font family names
// =============================================================================

/// The font family name used for filled material icons.
pub const FONT_FAMILY: &str = "material-icons";

// =============================================================================
// MaterialIcon
// =============================================================================

/// A material icon that can be rendered as filled.
///
/// Use directly with widgets: `ui.button(ICON_ADD)`.
#[derive(Clone, Copy, Debug)]
pub struct MaterialIcon {
    pub codepoint: &'static str,
}

impl MaterialIcon {
    pub const fn new(codepoint: &'static str) -> Self {
        Self { codepoint }
    }

    /// Returns the [`FontFamily`] for this icon.
    pub fn font_family(&self) -> FontFamily {
        FontFamily::Name(FONT_FAMILY.into())
    }

    /// Returns the icon as a [`RichText`] with the appropriate font family.
    pub fn rich_text(self) -> RichText {
        RichText::new(self.codepoint).family(self.font_family())
    }
}

impl From<MaterialIcon> for RichText {
    fn from(icon: MaterialIcon) -> Self {
        icon.rich_text()
    }
}

impl From<MaterialIcon> for egui::WidgetText {
    fn from(icon: MaterialIcon) -> Self {
        icon.rich_text().into()
    }
}

impl From<MaterialIcon> for &str {
    fn from(icon: MaterialIcon) -> Self {
        icon.codepoint
    }
}

impl From<MaterialIcon> for String {
    fn from(icon: MaterialIcon) -> Self {
        icon.codepoint.to_string()
    }
}

// =============================================================================
// Font registration
// =============================================================================

/// Creates a [`FontInsert`] for the material icons font.
pub fn font_insert() -> FontInsert {
    let mut data = FontData::from_static(FONT_DATA);
    data.tweak.y_offset_factor = 0.05;

    let families = vec![
        InsertFontFamily {
            family: FontFamily::Proportional,
            priority: FontPriority::Lowest,
        },
        InsertFontFamily {
            family: FontFamily::Name(FONT_FAMILY.into()),
            priority: FontPriority::Highest,
        },
    ];

    FontInsert::new(FONT_FAMILY, data, families)
}

/// Initializes the material icons font.
pub fn initialize(ctx: &egui::Context) {
    ctx.add_font(font_insert());
}

// =============================================================================
// Helper functions
// =============================================================================

/// Creates a frameless icon button.
pub fn icon_button(ui: &mut egui::Ui, icon: MaterialIcon) -> Response {
    Frame::new()
        .show(ui, |ui| {
            Button::new(icon.rich_text().size(18.0)).frame(false).ui(ui)
        })
        .inner
}

/// Creates a [`RichText`] from an icon.
pub fn icon_text(icon: MaterialIcon) -> RichText {
    icon.rich_text()
}
