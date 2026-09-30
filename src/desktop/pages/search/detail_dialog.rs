//! 搜索结果详情 Dialog body 渲染：左侧封面 + 右侧字段列表。
//!
//! 详情字段与封面都是反应式的：drain loop 每 100ms notify 触发重 render，builder 重调本
//! 函数重读 live cache，从占位自动切到真实内容。详情字段只有详情请求才完整，所以优先
//! `detail_cache` 的 Book，未回来时用 `SearchResult` 兜底。

use std::io::Cursor;
use std::sync::Arc;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Sizable, h_flex, link::Link, spinner::Spinner, v_flex,
};
use gpui_kit::{
    App, Entity, ImageSource, IntoElement, ObjectFit, ParentElement, RenderImage, SharedString,
    Styled, StyledImage, div, img, px,
};

use crate::desktop::model::{CoverEntry, DetailState};
use crate::i18n::ts;
use crate::models::SearchResult;

use super::SearchPage;

/// 详情 Dialog 封面区固定尺寸（宽 × 高）。封面比例不一，统一容器 + `ObjectFit::Fill`
/// 保证布局稳定。
const COVER_W: f32 = 120.0;
const COVER_H: f32 = 170.0;

/// 渲染详情 Dialog 的 body：左侧封面 + 右侧字段列表。
pub(super) fn content(
    r: &SearchResult,
    page: &Entity<SearchPage>,
    source_id: i32,
    url: &str,
    cx: &mut App,
) -> impl IntoElement {
    // 详情字段（intro / category / status / latest / last_update / author）搜索结果里是空的，
    // 优先取 detail_cache 的 Book。
    let book = page
        .read(cx)
        .model
        .read(cx)
        .search
        .detail_cache
        .get(&(source_id, url.to_string()))
        .and_then(|s| s.book().cloned());
    let b = book.as_ref();

    // source_name / word_count 只有 SearchResult 有（Book 不带），永远用 r。
    let source_val = if r.source_name.is_empty() {
        ts("Search.detail.unknown").to_string()
    } else {
        r.source_name.clone()
    };

    // detail-only 字段优先 Book，Book 为空时回退 SearchResult。
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

    // 链接行：label + 可点击 Link（自带 link 色 / 下划线 / hover）。
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
            // URL 无空格不会自动换行，overflow_x_hidden 截断超长部分。
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
            // 长简介（数千字）会把 Dialog body 顶出视口 → 内滚上限 ~10 行（200px）。
            Some(200.0),
            cx,
        ))
        .child(url_link);

    h_flex()
        .gap_4()
        .items_start()
        .child(render_detail_cover(page, source_id, url, cx))
        .child(fields.flex_1().min_w_0())
}

/// `Option<&str>` → 显示值；`None` / 纯空白 → `Search.detail.unknown`。
fn detail_opt(v: Option<&str>) -> SharedString {
    match v {
        Some(s) if !s.trim().is_empty() => SharedString::from(s.to_string()),
        _ => ts("Search.detail.unknown"),
    }
}

/// 「label + value」行：label 固定 84px、muted、xs；value `flex_1` 可换行。
///
/// `max_h` 给 value 区设最大高度 + 内滚，避免长字段把 Dialog 撑超高。
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
        // min_w_0 让 flex 子项可收缩到内容以下，长 value 不会撑爆行宽。
        .min_w_0()
        .text_sm()
        .text_color(cx.theme().foreground)
        .child(value);

    // `overflow_y_scrollbar` 是 terminal builder（返回 `Scrollable<Div>`，类型不是 `Div`），
    // 所以只能按 max_h 分支构造两种 element，不能链在 `when_some` 里。
    let value_el: gpui_kit::AnyElement = if let Some(h) = max_h {
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
/// `CoverEntry` 只存原图字节（UI 中立，见 `app/cover.rs`），所以解码必须放 UI 层。
/// 失败返回 `None` 而非 panic —— 调用方缓存负面结果，避免每帧重试解码。
fn decode_cover_image(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    // with_guessed_format 让 image crate 按 magic bytes 推断格式（PNG/JPEG/WebP/…）。
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    // 二次保险：CoverEntry::Ready 下载时已 probe 过，但缓存可能跨进程，这里再 probe 一次。
    let dynamic = match reader.decode() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(size = bytes.len(), error = %format!("{e}"), "UI 封面解码失败");
            return None;
        }
    };
    let mut rgba = dynamic.into_rgba8();

    // RGBA → BGRA：GPUI 纹理期望 BGRA 字节序。
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }

    // `Frame` 指 `image::Frame`。**别**写成 `gpui_kit::Frame` —— 那是 window 模块的
    // dispatch tree Frame，pub(crate) 且类型对不上。
    let frame = image::Frame::new(rgba);
    Some(Arc::new(RenderImage::new(vec![frame])))
}

/// 渲染详情 Dialog 的封面区。
///
/// 封面不在 `SearchResult` 里，要两级查找：`detail_cache` 拿 `book.cover_url` →
/// `cover_cache` 拿 `CoverEntry::Ready` 的字节 → 解码。任一级没到都显示「加载中」
/// （drain loop 100ms 后重 render 自动补上）；无 `cover_url` 显示「无封面」。
///
/// `page` 必须传 entity：`cover_images` 是 `&mut self` 字段，只能靠 `page.update` 拿
/// 可变借用写缓存。
fn render_detail_cover(
    page: &Entity<SearchPage>,
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
                        // 命中本页解码缓存直接复用，否则解码后写缓存。
                        if let Some(cached) = p.cover_images.get(uri.as_str()).cloned() {
                            cached.map_or(CoverView::Failed, CoverView::Image)
                        } else if let Some(img) = decode_cover_image(bytes) {
                            p.cover_images.put(uri.clone(), Some(img.clone()));
                            CoverView::Image(img)
                        } else {
                            p.cover_images.put(uri.clone(), None);
                            CoverView::Failed
                        }
                    }
                    Some(CoverEntry::Failed(_)) => CoverView::Failed,
                    None => CoverView::Loading,
                }
            }
        }
    });

    // 固定容器：封面 / 各种占位都进同一个框，保证布局不抖。
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
            // 改名 `rendered` —— `img` 是自由函数，避免遮蔽。
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
