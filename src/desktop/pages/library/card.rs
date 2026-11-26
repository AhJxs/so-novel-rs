//! 书架卡片：封面（真实封面或生成封面）、格式徽标、收藏星标、阅读进度角标、书名 / 作者。

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Selectable as _};
use gpui_kit::{
    App, ClickEvent, Context, ElementId, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ObjectFit, ParentElement, RenderOnce, StatefulInteractiveElement as _, Styled,
    StyledImage as _, Window, div, img, linear_color_stop, linear_gradient, rems, rgb,
};
use rust_i18n::t;

use crate::desktop::motion;
use crate::library::{ShelfBook, cover_palette};

/// 封面尺寸（2:3，rem 尺度，默认字号下为 140×210）。
///
/// 用 rem 而不是 px：正文是 `text_2xl` / `text_sm` 这类相对字号，字号放大后
/// 固定的 140px 会把两行书名挤爆，封面与卡片宽度必须跟着一起缩放。
const COVER_W_REMS: f32 = 8.75;
const COVER_H_REMS: f32 = 13.125;

/// 卡片渲染所需的只读快照 —— 每帧只取这些字段，不 clone 整本 `ShelfBook`。
///
/// `Clone` 只服务于书架页缓存：缓存里的卡片每个可见项复制一份交给 `BookCard`（元素必须是
/// `'static`，不能借用），代价是几次字符串拷贝，不再有磁盘 stat。
#[derive(Clone)]
pub(super) struct CardData {
    pub id: String,
    pub title: String,
    pub author: String,
    pub format_label: &'static str,
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
    /// 由 `ShelfBook` 与封面缓存目录构造；没有下载目录或缓存文件已不在磁盘上时退回生成封面。
    pub(super) fn from_book(book: &ShelfBook, covers_dir: Option<&Path>) -> Self {
        let cover_path = covers_dir
            .zip(book.cover_file.as_deref())
            .and_then(|(dir, file)| {
                // 持久化数据可能被手工改过：带路径分隔符的名字一律不读（防目录穿越）。
                if file.contains('/') || file.contains('\\') {
                    return None;
                }
                let path = dir.join(file);
                path.is_file().then_some(path)
            });
        let (_, top, bottom) = cover_palette(&book.id);
        Self {
            id: book.id.clone(),
            title: book.title.clone(),
            author: book.author.clone(),
            format_label: book.format.label(),
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
    /// 键盘导航当前是否落在这张卡上（由书架页算，不进 `CardData` 缓存）。
    focused: bool,
}

impl BookCard {
    pub(super) fn new(data: CardData, on_open: ClickHandler, on_favorite: ClickHandler) -> Self {
        Self {
            data,
            on_open,
            on_favorite,
            menu: None,
            focused: false,
        }
    }

    /// 标记键盘焦点卡片：只影响焦点环，不参与 `CardData` 缓存键。
    pub(super) const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// 挂上右键菜单（继续阅读 / 书籍信息 / 在文件夹中显示 …）。
    pub(super) fn with_context_menu(mut self, menu: MenuBuilder) -> Self {
        self.menu = Some(menu);
        self
    }
}

impl RenderOnce for BookCard {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            data,
            on_open,
            on_favorite,
            menu,
            focused,
        } = self;

        // 悬停态存 keyed state：`BookCard` 是 `RenderOnce`，没有自己的 entity 记这个
        // bool；keyed entity 被当前 view 观察着，`cx.notify()` 会触发书架重绘。
        let hover = window.use_keyed_state(
            ElementId::Name(format!("book-card-hovered-{}", data.id).into()),
            cx,
            |_, _| false,
        );
        let hovered = *hover.read(cx);
        // 背景色过渡：静止态是「同一色、透明度 0」，只在透明通道上动，
        // 渐变到 `list_hover` 时不会先跳到别的色相。
        let hover_bg = cx.theme().list_hover;
        let background = gpui_kit::base::motion::transition(
            ("book-card-bg", data.id.clone()),
            if hovered {
                hover_bg
            } else {
                Hsla { a: 0.0, ..hover_bg }
            },
            motion::fast(cx),
            window,
            cx,
        );

        // ---- 1. 封面区：先铺生成封面（渐变 + 书名首字），有真实封面时再盖上去 ----
        let mut cover = div()
            .relative()
            .w(rems(COVER_W_REMS))
            .h(rems(COVER_H_REMS))
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
                // 首字落在 `cover_palette` 生成的两段深色渐变上，用主色前景保证对比度。
                div()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().primary_foreground)
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
        // 底色 / 文字用主题的背景与前景配对，浅色与深色主题都能读清。
        cover = cover.child(
            div()
                .absolute()
                .top_1()
                .left_1()
                .px_1()
                .rounded_sm()
                .bg(cx.theme().background.opacity(0.85))
                .text_xs()
                .text_color(cx.theme().foreground)
                .child(data.format_label),
        );

        // 右上收藏星标：换成组件化 Button，自带键盘焦点、Enter / Space 激活、
        // 选中语义与 tooltip；外层绝对定位保留在封面上。
        // 点击仍要阻止冒泡，否则会触发外层卡片的「打开阅读器」。
        let favorite_label = t!("Library.card.favorite").to_string();
        let favorite_icon = Icon::new(if data.favorite {
            IconName::StarFill
        } else {
            IconName::Star
        })
        .text_color(if data.favorite {
            cx.theme().warning
        } else {
            // 星标叠在深色封面上，用主色前景而不是 `foreground`，浅色主题下也不会变暗。
            cx.theme().primary_foreground
        });
        cover = cover.child(
            div().absolute().top_1().right_1().child(
                Button::new(ElementId::Name(format!("book-fav-{}", data.id).into()))
                    .ghost()
                    .compact()
                    // 24px 点按区：图标按钮里 `compact` 不改变尺寸，显式给 `size_6`。
                    .size_6()
                    .icon(favorite_icon)
                    .tooltip(favorite_label.clone())
                    .accessibility_label(favorite_label)
                    .selected(data.favorite)
                    .on_click(move |event, window, cx| (on_favorite)(event, window, cx)),
            ),
        );

        // 左下进度角标：只在真读过（四舍五入到 1% 以上）时出现 —— 未读的书整张封面
        // 保持干净，不用「0%」占位。缺失的书不显示进度（进度对它没有意义）。
        let percent = data.percent.clamp(0.0, 100.0);
        if !data.missing && percent.round() >= 1.0 {
            cover = cover.child(
                div()
                    .absolute()
                    .bottom_1()
                    .left_1()
                    .px_1()
                    .rounded_sm()
                    // 与左上格式徽标同一套配对色：叠在深浅不一的封面上都读得清。
                    .bg(cx.theme().background.opacity(0.85))
                    .text_xs()
                    .text_color(cx.theme().foreground)
                    .child(format!("{percent:.0}%")),
            );
        }

        if data.missing {
            // 缺失态是错误语义而不是单纯遮罩：用 danger 配对色，文字本身也说明了原因。
            cover = cover.child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(cx.theme().danger.opacity(0.85))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().danger_foreground)
                            .child(t!("Library.card.missing").to_string()),
                    ),
            );
        }

        // ---- 2. 内容列：封面 + 书名 1 行 + 作者 1 行，宽度锁在封面宽度上 ----
        let content = div()
            .w(rems(COVER_W_REMS))
            .flex()
            .flex_col()
            .gap_1()
            .child(cover)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .truncate()
                    .child(data.title),
            );

        let content = if data.author.is_empty() {
            content
        } else {
            content.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(data.author),
            )
        };

        // ---- 3. 卡片外框：悬停遮罩画在这一层，内边距让它比内容大一圈 ----
        //
        // 遮罩挂在带内边距的外框、而不是内容列上：高亮浮起时四周留出呼吸感，
        // 内容不跟着挪（宽度锁在封面宽度）。网格列数是布局后实测的，加内边距
        // 只会让每列变宽一点，不需要同步改常量。
        let card = div()
            .id(ElementId::Name(format!("book-card-{}", data.id).into()))
            .relative()
            .rounded_md()
            .p_1p5()
            .bg(background)
            .cursor_pointer()
            .on_hover(move |is_hovered, _window, cx| {
                hover.update(cx, |state, cx| {
                    if *state != *is_hovered {
                        *state = *is_hovered;
                        cx.notify();
                    }
                });
            })
            .on_click(move |event, window, cx| (on_open)(event, window, cx))
            .child(content);

        // 键盘焦点环画成覆盖层：绝对定位不参与布局，不会改变网格间距；
        // 只画描边、不填底色，也不会挡住卡片与星标的点击。
        let card = if focused {
            card.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_md()
                    .border_2()
                    .border_color(cx.theme().primary),
            )
        } else {
            card
        };

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
