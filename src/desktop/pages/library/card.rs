//! 书架卡片：封面（真实封面或生成封面）、格式徽标、收藏星标、书名 / 作者、进度条。

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName};
use gpui_kit::{
    App, ClickEvent, Context, ElementId, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ObjectFit, ParentElement, RenderOnce, StatefulInteractiveElement as _, Styled,
    StyledImage as _, Window, div, img, linear_color_stop, linear_gradient, px, relative, rgb,
    rgba,
};
use rust_i18n::t;

use crate::library::{Book, cover_palette};

/// 封面尺寸（2:3）：140×210。
const COVER_W: f32 = 140.0;
const COVER_H: f32 = 210.0;

/// 卡片渲染所需的只读快照 —— 每帧只取这些字段，不 clone 整本 `Book`。
pub(super) struct CardData {
    pub id: String,
    pub title: String,
    pub author: String,
    pub format_label: &'static str,
    /// `Book::status_key()` 的结果，UI 层再翻译。
    pub status_key: &'static str,
    pub percent: f32,
    pub favorite: bool,
    pub missing: bool,
    /// 磁盘路径：右键菜单的「在文件夹中显示 / 用默认程序打开」要用。
    pub path: String,
    /// 格式能否在阅读器里打开（右键菜单据此禁用「继续阅读」）。
    pub readable: bool,
    /// 已读完（右键菜单据此显示「从头开始」）。
    pub finished: bool,
    pub cover_path: Option<PathBuf>,
    /// 生成封面的渐变色（顶部 / 底部）。
    pub cover_top: Hsla,
    pub cover_bottom: Hsla,
}

impl CardData {
    /// 由 `Book` 与封面缓存目录构造；缓存文件已不在磁盘上时退回生成封面。
    pub(super) fn from_book(book: &Book, covers_dir: &Path) -> Self {
        let cover_path = book.cover_file.as_deref().and_then(|file| {
            // `library.json` 可能被手工改过：带路径分隔符的名字一律不读（防目录穿越）。
            if file.contains('/') || file.contains('\\') {
                return None;
            }
            let path = covers_dir.join(file);
            path.is_file().then_some(path)
        });
        let (_, top, bottom) = cover_palette(&book.id);
        Self {
            id: book.id.clone(),
            title: book.title.clone(),
            author: book.author.clone(),
            format_label: book.format.label(),
            status_key: book.status_key(),
            percent: book.progress.percent,
            favorite: book.favorite,
            missing: book.missing,
            path: book.path.clone(),
            readable: book.format.is_readable(),
            finished: book.progress.finished,
            cover_path,
            cover_top: hsla_from(top),
            cover_bottom: hsla_from(bottom),
        }
    }
}

/// 卡片点击回调；`Rc` 让同一个 handler 能被多张卡片共享。
type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// 右键菜单构建器。菜单项要书名 / 路径等快照，所以由书架页按书生成后传进来。
pub(super) type MenuBuilder =
    Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

/// 书架卡片。
#[derive(IntoElement)]
pub(super) struct BookCard {
    data: CardData,
    on_open: ClickHandler,
    on_favorite: ClickHandler,
    menu: Option<MenuBuilder>,
}

impl BookCard {
    pub(super) fn new(data: CardData, on_open: ClickHandler, on_favorite: ClickHandler) -> Self {
        Self {
            data,
            on_open,
            on_favorite,
            menu: None,
        }
    }

    /// 挂上右键菜单（继续阅读 / 书籍信息 / 在文件夹中显示 …）。
    pub(super) fn with_context_menu(mut self, menu: MenuBuilder) -> Self {
        self.menu = Some(menu);
        self
    }
}

impl RenderOnce for BookCard {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            data,
            on_open,
            on_favorite,
            menu,
        } = self;

        // ---- 1. 封面区：先铺生成封面（渐变 + 书名首字），有真实封面时再盖上去 ----
        let mut cover = div()
            .relative()
            .w(px(COVER_W))
            .h(px(COVER_H))
            .rounded_md()
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .bg(linear_gradient(
                135.0,
                linear_color_stop(data.cover_top, 0.0),
                linear_color_stop(data.cover_bottom, 1.0),
            ))
            .child(
                // 白色首字落在两段深色渐变上，对比度够。
                div()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(0x00ff_ffff))
                    .child(first_char(&data.title)),
            );

        if let Some(path) = data.cover_path.as_deref() {
            cover = cover.child(
                div()
                    .absolute()
                    .inset_0()
                    .child(img(path).size_full().object_fit(ObjectFit::Cover)),
            );
        }

        // 左上格式徽标：TXT / EPUB 是技术名词，不翻译。
        cover = cover.child(
            div()
                .absolute()
                .top_1()
                .left_1()
                .px_1()
                .rounded_sm()
                .bg(rgba(0x0000_0099))
                .text_xs()
                .text_color(rgb(0x00ff_ffff))
                .child(data.format_label),
        );

        // 右上收藏星标：点击必须阻止冒泡，否则会触发外层卡片的「打开阅读器」。
        let favorite_icon = Icon::new(if data.favorite {
            IconName::StarFill
        } else {
            IconName::Star
        })
        .text_color(if data.favorite {
            rgb(0x00fb_bf24)
        } else {
            rgb(0x00ff_ffff)
        });
        cover = cover.child(
            div()
                .id(ElementId::Name(format!("book-fav-{}", data.id).into()))
                .absolute()
                .top_1()
                .right_1()
                .w(px(22.))
                .h(px(22.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(0x0000_0066))
                .cursor_pointer()
                .child(favorite_icon)
                .on_click(move |event, window, cx| (on_favorite)(event, window, cx)),
        );

        if data.missing {
            cover = cover.child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x0000_00b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x00ff_ffff))
                            .child(t!("Library.card.missing").to_string()),
                    ),
            );
        }

        // ---- 2. 文本区：书名 2 行 + 作者 1 行 + 进度条 + 状态 ----
        let mut card = div()
            .id(ElementId::Name(format!("book-card-{}", data.id).into()))
            .w(px(COVER_W))
            .flex()
            .flex_col()
            .gap_1()
            .cursor_pointer()
            .on_click(move |event, window, cx| (on_open)(event, window, cx))
            .child(cover)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .line_clamp(2)
                    .child(data.title.clone()),
            );

        if !data.author.is_empty() {
            card = card.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(data.author.clone()),
            );
        }

        let percent = data.percent.clamp(0.0, 100.0);
        let card = card.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .h(px(3.))
                        .w_full()
                        .rounded_full()
                        .bg(cx.theme().muted)
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(cx.theme().primary)
                                .w(relative(percent / 100.0)),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{} · {percent:.0}%", t!(data.status_key))),
                ),
        );

        // 卡片操作走右键菜单：左键仍是打开阅读器，悬停不再出现「…」按钮。
        match menu {
            Some(build) => card
                .context_menu(move |popup, window, cx| build(popup, window, cx))
                .into_any_element(),
            None => card.into_any_element(),
        }
    }
}

/// 书名首字（生成封面用）。
fn first_char(title: &str) -> String {
    title.chars().next().map(String::from).unwrap_or_default()
}

/// `[r, g, b]` → `Hsla`。
fn hsla_from([red, green, blue]: [u8; 3]) -> Hsla {
    rgb(u32::from_be_bytes([0, red, green, blue])).into()
}
