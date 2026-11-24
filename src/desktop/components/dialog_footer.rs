//! 表单对话框的页脚：左取消、右确认。
//!
//! gpui-kit 0.11 起，普通 `Dialog` 不再根据 `button_props` 自己渲染按钮
//! （`Dialog::footer` 文档："A plain `Dialog` never renders buttons from `button_props`"），
//! 只有 `AlertDialog` 会补默认页脚。表单弹窗改用 `Dialog` 后必须自己给页脚，
//! 否则会得到一个没有按钮的空窗体。
//!
//! 按钮不直接调回调：`DialogClose` / `DialogAction` 会把点击派发成对话框作用域里的
//! `Cancel` / `Confirm` action，从而复用 `open_dialog` 上挂的 `on_ok` / `on_cancel`。

use gpui_kit::IntoElement;
use gpui_kit::ParentElement as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};

/// 表单对话框页脚：`cancel` 在左（默认样式），`confirm` 在右（主按钮）。
///
/// `on_ok` 返回 `false` 时对话框保持打开，校验失败的场景直接沿用该语义。
pub fn form_dialog_footer(
    confirm: impl Into<gpui_kit::SharedString>,
    cancel: impl Into<gpui_kit::SharedString>,
) -> impl IntoElement {
    DialogFooter::new()
        .child(DialogClose::new().trigger(|button| button.label(cancel)))
        .child(DialogAction::new().child(Button::new("dialog-confirm").primary().label(confirm)))
}

#[cfg(test)]
mod gui_tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui_kit::component::WindowExt as _;
    use gpui_kit::component::button::Button;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AppContext as _, Context, IntoElement, ParentElement, Render, Styled, TestAppContext,
        Window, div,
    };

    use super::form_dialog_footer;

    /// 宿主：一个「打开」按钮，点击后用 `window.open_dialog` 弹表单对话框。
    ///
    /// 必须走 `open_dialog` 而不是 `Dialog::trigger`：trigger 分支只搬运
    /// style / `button_props` / content，会把 `footer`、`title`、children 丢掉。
    struct Harness {
        confirmed: Rc<Cell<usize>>,
    }

    impl Harness {
        fn open_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
            let confirmed = Rc::clone(&self.confirmed);
            window.open_dialog(cx, move |dialog, _window, _cx| {
                let confirmed = Rc::clone(&confirmed);
                dialog
                    .title("Form")
                    // 关掉右上角 X，让 "close" 只可能是页脚里的取消按钮。
                    .close_button(false)
                    .child(div().child("body"))
                    .footer(form_dialog_footer("OK", "Cancel"))
                    .on_ok(move |_ev, _window, _cx| {
                        confirmed.set(confirmed.get() + 1);
                        true
                    })
            });
        }
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                Button::new("open")
                    .label("Open")
                    .on_click(cx.listener(|this, _, window, cx| this.open_dialog(window, cx))),
            )
        }
    }

    fn open(cx: &mut TestAppContext) -> (Rc<Cell<usize>>, &mut gpui_kit::VisualTestContext) {
        cx.update(gpui_kit::component::init);
        let confirmed = Rc::new(Cell::new(0));
        let handle = Rc::clone(&confirmed);
        // 对话框状态挂在 `gpui_base::Root` 的插件上：窗口根必须是 `Root`，
        // 直接 `add_window_view(Harness)` 会在 `WindowState::read` 处 panic。
        let (_, vcx) = cx.add_window_view(move |window, cx| {
            let content = cx.new(|_cx| Harness { confirmed: handle });
            gpui_kit::base::Root::new(content, window, cx)
        });
        (confirmed, vcx)
    }

    #[gpui_kit::test]
    fn form_dialog_renders_cancel_and_confirm_buttons(cx: &mut TestAppContext) {
        let (_confirmed, vcx) = open(cx);
        vcx.update(|window, cx| {
            window.click("open", cx);
            window.render_frame(cx);
            // 回归点：普通 `Dialog` 不会自己渲染按钮，页脚是唯一的按钮来源。
            assert!(
                window.try_find("dialog-confirm").is_some(),
                "普通 Dialog 必须靠 footer 提供确认按钮"
            );
            // `DialogClose` 内部固定用 "close" 这个 id。
            assert!(window.try_find("close").is_some(), "取消按钮缺失");
        });
    }

    #[gpui_kit::test]
    fn confirm_button_dispatches_dialog_on_ok(cx: &mut TestAppContext) {
        let (confirmed, vcx) = open(cx);
        vcx.update(|window, cx| {
            window.click("open", cx);
            window.render_frame(cx);
            window.click("dialog-confirm", cx);
        });
        assert_eq!(confirmed.get(), 1, "确认按钮必须走 Dialog 的 on_ok");
    }
}
