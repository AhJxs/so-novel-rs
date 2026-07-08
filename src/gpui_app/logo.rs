//! Sidebar header 用的小 logo (assets/logo.png 编译期嵌入) + RGBA→BGRA swap。
//!
//! 用 `.png` (位图) 而非 `.svg`: gpui 的 `img()` 不直接吃 SVG 字节 —— SVG 需要装到
//! asset loader 走 `AssetSource` + 内置 SVG 光栅化。本项目 assets loader 是
//! `gpui_component_assets::Assets`, 不包含我们的 logo。最简、零运行时依赖路径就是
//! 嵌 PNG 字节 + `image` crate 解码成 `RenderImage` (流程同 `decode_cover_image`)。
//!
//! 主流程 [`render_logo`] 在 [`super::root::RootView::render_sidebar`]。

use std::io::Cursor;
use std::sync::{Arc, LazyLock};

use gpui::{
    AnyElement, ImageSource, IntoElement, ObjectFit, RenderImage, Styled as _, StyledImage as _,
    div, img, px,
};

/// Sidebar header 用的小 logo (assets/logo.png 编译期嵌入)。
const LOGO_PNG: &[u8] = include_bytes!("../../assets/logo.png");

/// 解码好的 logo (RGBA→BGRA swap 后的 `RenderImage`)。`Lazy` 启动首帧用一次, 之后复用。
pub(super) static LOGO_IMAGE: LazyLock<Option<Arc<RenderImage>>> =
    LazyLock::new(|| decode_logo_image(LOGO_PNG));

/// 解码 PNG 字节 → `RenderImage`。流程同 `decode_cover_image`, 但 logo 是静态资源 → `Lazy` 缓存。
fn decode_logo_image(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let dynamic = reader.decode().ok()?;
    let mut rgba = dynamic.into_rgba8();
    // RGBA → BGRA: GPUI 纹理期望 BGRA 字节序 (见 gpui img.rs L671-674 swap(0,2))。
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let frame = image::Frame::new(rgba);
    Some(Arc::new(RenderImage::new(vec![frame])))
}

/// 渲染 logo 图片元素 (正方形, object-fit contain)。
///
/// 解码失败 → 返回空 div 占位, 不让 UI 崩。`size` 走 `px()` 显式像素而非 rem:
/// logo 是图标资源, 不跟字号缩放。
pub(super) fn render_logo(size: gpui::Pixels) -> AnyElement {
    match LOGO_IMAGE.as_ref() {
        Some(rendered) => img(ImageSource::Render(rendered.clone()))
            .object_fit(ObjectFit::Contain)
            .size(size)
            .flex_shrink_0()
            .into_any_element(),
        None => div().size(size).flex_shrink_0().into_any_element(),
    }
}