//! 搜索结果详情 Dialog body 渲染：左侧封面 + 右侧字段列表。
//!
//! 布局 `h_flex`：左封面固定 `COVER_W × COVER_H`，右字段 `flex_1`。Dialog body 自带
//! `overflow_y_scrollbar`（见 gpui-component Dialog::render），字段多 / 简介长可滚动查看。
//!
//! 封面是反应式的：`render_detail_cover` 每帧重读 live `cover_cache`，封面到达后自动刷新
//! （drain loop 100ms notify → RootView 重 render → Dialog builder 重调本函数）。

use std::io::Cursor;
use std::sync::Arc;

use gpui::{
    App, Entity, ImageSource, IntoElement, ObjectFit, ParentElement, RenderImage, SharedString,
    Styled, StyledImage, div, img, px,
};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{ActiveTheme as _, Sizable, h_flex, link::Link, spinner::Spinner, v_flex};

use crate::app::{CoverEntry, DetailState};
use crate::i18n::ts;
use crate::models::SearchResult;

use super::SearchPage;

/// 详情 Dialog 封面区固定尺寸（宽 × 高）。封面比例不一，统一容器 + `ObjectFit::Contain`
/// 居中显示，留白用 muted 背景，跟空态 / 失败占位共用同一个框。
const COVER_W: f32 = 120.0;
const COVER_H: f32 = 170.0;

/// 渲染详情 Dialog 的 body：左侧封面 + 右侧字段列表。
pub(super) fn content(
    r: SearchResult,
    page: Entity<SearchPage>,
    source_id: i32,
    url: String,
    cx: &mut App,
) -> impl IntoElement {
    // 详情是再次请求拿的完整字段（intro / category / status / latest / last_update / author），
    // 搜索结果里这些是空的。优先用 detail_cache 的 Book；detail 还没回来时用 SearchResult
    // 兜底（intro 会显 unknown），drain loop 把 Book 拉回来后自动切到完整数据。
    let book = page
        .read(cx)
        .model
        .read(cx)
        .search
        .detail_cache
        .get(&(source_id, url.clone()))
        .and_then(|s| s.book().cloned());
    let b = book.as_ref();

    // source_name / word_count 只有 SearchResult 有（Book 不带），永远用 r。
    let source_val = if r.source_name.is_empty() {
        ts("Search.detail.unknown").to_string()
    } else {
        r.source_name.clone()
    };

    // 合并：detail-only 字段优先 Book，Book 为空时回退 SearchResult。
    let book_name = b
        .map(|x| x.book_name.clone())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| r.book_name.clone());
    let author = match b {
        Some(x) if !x.author.trim().is_empty() => SharedString::from(x.author.clone()),
        _ => detail_opt(r.author.as_deref()),
    };
    let category = b
        .and_then(|x| x.category.as_deref())
        .or(r.category.as_deref());
    let status = b.and_then(|x| x.status.as_deref()).or(r.status.as_deref());
    let latest = b
        .and_then(|x| x.latest_chapter.as_deref())
        .or(r.latest_chapter.as_deref());
    let last_update = b
        .and_then(|x| x.last_update_time.as_deref())
        .or(r.last_update_time.as_deref());
    let intro = b.and_then(|x| x.intro.as_deref()).or(r.intro.as_deref());

    // 链接行：label + 可点击 Link（自带 link 色 / 下划线 / hover，点击 cx.open_url 打开）。
    let url_display = if r.url.trim().is_empty() {
        ts("Search.detail.unknown")
    } else {
        SharedString::from(r.url.clone())
    };
    let url_link = h_flex()
        .gap_3()
        .items_start()
        .child(
            div()
                .w(px(84.0))
                .flex_shrink_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(ts("Search.detail.field.url")),
        )
        .child(
            // gpui 0.2.2 无 break_all —— URL 无空格不会自动换行，overflow_x_hidden 截断
            // 超长部分（用户点开链接即可看完整 URL）。
            div()
                .flex_1()
                .min_w_0()
                .overflow_x_hidden()
                .text_sm()
                .child(
                    Link::new("detail-url")
                        .href(r.url.clone())
                        .child(url_display),
                ),
        );

    let fields = v_flex()
        .gap_2()
        .child(detail_row(
            ts("Search.detail.field.book_name"),
            SharedString::from(book_name),
            None,
            cx,
        ))
        .child(detail_row(
            ts("Search.detail.field.author"),
            author,
            None,
            cx,
        ))
        .child(detail_row(
            ts("Search.detail.field.source"),
            SharedString::from(source_val),
            None,
            cx,
        ))
        .child(detail_row(
            ts("Search.detail.field.category"),
            detail_opt(category),
            None,
            cx,
        ))
        .child(detail_row(
            ts("Search.detail.field.status"),
            detail_opt(status),
            None,
            cx,
        ))
        .child(detail_row(
            ts("Search.detail.field.latest_chapter"),
            detail_opt(latest),
            None,
            cx,
        ))
        .child(detail_row(
            ts("Search.detail.field.last_update"),
            detail_opt(last_update),
            None,
            cx,
        ))
        .child(detail_row(
            ts("Search.detail.field.intro"),
            detail_opt(intro),
            // 长简介（数千字）容易把 Dialog body 顶出视口 → 内滚上限 ~10 行（200px）。
            Some(200.0),
            cx,
        ))
        .child(url_link);

    h_flex()
        .gap_4()
        .items_start()
        .child(render_detail_cover(page, source_id, &url, cx))
        .child(fields.flex_1().min_w_0())
}

/// `Option<&str>` → 显示值；`None` / 纯空白 → `Search.detail.unknown` fallback。
fn detail_opt(v: Option<&str>) -> SharedString {
    match v {
        Some(s) if !s.trim().is_empty() => SharedString::from(s.to_string()),
        _ => ts("Search.detail.unknown"),
    }
}

/// 详情 Dialog 的「label + value」行：label 固定 84px、muted、xs；value flex_1、可换行。
///
/// `max_h`：长内容字段（如简介）传 `Some(px)` 给 value 区设最大高度 + 内部滚动条，
/// 避免单个字段把整个 Dialog 撑得超高。
fn detail_row(
    label: SharedString,
    value: SharedString,
    max_h: Option<f32>,
    cx: &App,
) -> impl IntoElement {
    let label_el = div()
        .w(px(84.0))
        .flex_shrink_0()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(label);

    let value_inner = div()
        .flex_1()
        // min_w_0 让 flex 子项能收缩到内容以下，长 value 不会把行宽撑爆。
        .min_w_0()
        .text_sm()
        .text_color(cx.theme().foreground)
        .child(value);

    // `overflow_y_scrollbar` 是 terminal builder（返回 `Scrollable<Div>`），类型与
    // `Div` 不同 → 不能用 `when_some` 链在内部，按 max_h 分支构造两种 element。
    let value_el: gpui::AnyElement = if let Some(h) = max_h {
        value_inner
            .max_h(px(h))
            .overflow_y_scrollbar()
            .into_any_element()
    } else {
        value_inner.into_any_element()
    };

    h_flex()
        .gap_3()
        .items_start()
        .child(label_el)
        .child(value_el)
}

/// 解码封面原始字节 → `Arc<RenderImage>`。
///
/// `CoverEntry` 是 UI 中立的（只存原图字节，见 `app/cover.rs`），解码必须放 UI 层。
/// 流程跟 gpui 自己的 `AssetLoader::<ImageDecoder>` 内部一致（`img.rs` L669-692）：
/// `image::ImageReader` → `into_rgba8()` → RGBA↔BGRA swap（GPUI 纹理是 BGRA）→ `Frame` → `RenderImage`。
///
/// 失败返回 `None`（不是 panic）—— 调用方缓存负面结果，避免每帧重试解码。
fn decode_cover_image(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    // with_guessed_format 让 image crate 按 magic bytes 推断格式（PNG/JPEG/WebP/…）。
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    // 二次保险：解码前再校验是有效图片 —— CoverEntry::Ready 已在下载时 probe 过一次，
    // 但缓存可能跨进程/异常，这里 probe 一次更稳，且只是几 µs 的开销。
    let dynamic = match reader.decode() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(size = bytes.len(), error = %format!("{e}"), "UI 封面解码失败");
            return None;
        }
    };
    let mut rgba = dynamic.into_rgba8();

    // RGBA → BGRA：GPUI 纹理期望 BGRA 字节序（见 gpui img.rs L671-674 swap(0,2)）。
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    // `Frame` 是 `image::Frame`（跟 gpui 的 `RenderImage::new` 内部一致，见 gpui img.rs
    // L669-692 用 `image::Frame::new`）。**别**写成 `gpui::Frame` —— 那是 window 模块的
    // dispatch tree Frame（pub(crate)，外部不可构造，类型也对不上）。
    let frame = image::Frame::new(rgba);
    Some(Arc::new(RenderImage::new(vec![frame])))
}

/// 渲染详情 Dialog 的封面区。
///
/// 封面两级查找（封面不在 `SearchResult` 里，得先拉详情拿 `cover_url` 再下载字节）：
/// 1. `detail_cache[(source_id, url)]` → `DetailState::Loaded(book)` → `book.cover_url`
/// 2. `cover_cache[(source_id, cover_url)]` → `CoverEntry::Ready { bytes, uri }` → 解码
///
/// 状态分支：
/// - `DetailState::Pending` / detail 未拉 → 显示「封面加载中…」（drain loop 100ms 后刷新）
/// - `Loaded` 但无 `cover_url` → 「无封面」
/// - 有 `cover_url` 但 `cover_cache` 还没到 / `Failed` → 「封面加载中…」/「封面获取失败」
/// - `CoverEntry::Ready` → 命中本页解码缓存就渲染，未命中就解码 + 写缓存再渲染
///
/// `page: Entity<SearchPage>`：本页 `cover_images` 缓存是 `&mut self` 字段，必须通过
/// `page.update` 拿可变借用写缓存。读 model 也走 `page.model`，避免在已借 `model` 时再借。
fn render_detail_cover(
    page: Entity<SearchPage>,
    source_id: i32,
    url: &str,
    cx: &mut App,
) -> impl IntoElement {
    enum CoverView {
        Loading,
        Failed,
        None,
        Image(Arc<RenderImage>),
    }

    let view = page.update(cx, |p, cx| {
        let detail = p
            .model
            .read(cx)
            .search
            .detail_cache
            .get(&(source_id, url.to_string()));
        match detail {
            None | Some(DetailState::Pending) => CoverView::Loading,
            Some(DetailState::Failed(_)) => CoverView::Failed,
            Some(DetailState::Loaded(book)) => {
                let Some(cover_url) = book
                    .cover_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                else {
                    return CoverView::None;
                };

                let cover = p
                    .model
                    .read(cx)
                    .search
                    .cover_cache
                    .peek(&(source_id, cover_url.to_string()));
                match cover {
                    Some(CoverEntry::Ready { bytes, uri }) => {
                        // 命中本页解码缓存就复用；否则解码 + 写缓存。
                        if let Some(cached) = p.cover_images.get(uri.as_str()).cloned() {
                            match cached {
                                Some(img) => CoverView::Image(img),
                                None => CoverView::Failed,
                            }
                        } else {
                            match decode_cover_image(bytes) {
                                Some(img) => {
                                    p.cover_images.put(uri.clone(), Some(img.clone()));
                                    CoverView::Image(img)
                                }
                                None => {
                                    p.cover_images.put(uri.clone(), None);
                                    CoverView::Failed
                                }
                            }
                        }
                    }
                    Some(CoverEntry::Failed(_)) => CoverView::Failed,
                    None => CoverView::Loading,
                }
            }
        }
    });

    // 固定容器：muted 底 + 圆角 + 居中内容。封面 / 占位文案都进同一个框，保证布局稳定。
    let container = div()
        .w(px(COVER_W))
        .h(px(COVER_H))
        .flex_shrink_0()
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden();

    match view {
        CoverView::Image(rendered) => container.child(
            // 变量改名 `rendered` —— `img` 是 gpui 自由函数（`gpui::img(source)`），避免遮蔽。
            img(ImageSource::Render(rendered))
                .rounded(cx.theme().radius)
                .object_fit(ObjectFit::Fill)
                .size_full(),
        ),
        CoverView::Loading => container.child(
            v_flex()
                .gap_1()
                .items_center()
                .child(Spinner::new().small())
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(ts("Search.detail.cover.loading")),
                ),
        ),
        CoverView::Failed => container.child(
            div()
                .p_2()
                .text_center()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(ts("Search.detail.cover.failed")),
        ),
        CoverView::None => container.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(ts("Search.detail.cover.none")),
        ),
    }
}
