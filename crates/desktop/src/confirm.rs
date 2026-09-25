//! Port of `apps/desktop/src/ui/confirm.tsx`: one question, two buttons.
//! Enter confirms, Escape or a click outside cancels.

use gpui_kit::component::box_shadow;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::primitives::{Button, ButtonKind};
use crate::theme::{Theme, radius, spacing, type_scale};

pub struct ConfirmRequest {
    pub title: SharedString,
    pub body: Option<SharedString>,
    /// Label of the button that goes through with it.
    pub action: SharedString,
    pub danger: bool,
    pub on_confirm: std::rc::Rc<dyn Fn(&mut Window, &mut App)>,
}

impl ConfirmRequest {
    pub fn new(title: impl Into<SharedString>, action: impl Into<SharedString>, on_confirm: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        Self { title: title.into(), body: None, action: action.into(), danger: false, on_confirm: std::rc::Rc::new(on_confirm) }
    }

    pub fn body(mut self, body: impl Into<SharedString>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

/// `open=false` while it fades out; the caller unmounts it once that finishes
/// (see `motion::Presence` and `app.rs`'s overlay stack).
pub fn confirm_dialog(
    request: &ConfirmRequest,
    bounds: Size<Pixels>,
    focus_handle: &FocusHandle,
    on_close: impl Fn(&mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let palette = Theme::get(cx);
    let confirm_action = request.on_confirm.clone();
    let close_for_confirm = on_close.clone();
    let close_for_escape = on_close.clone();
    let close_for_outside = on_close.clone();

    anchored().position(point(px(0.), px(0.))).child(deferred(
        div()
            .id("confirm-scrim")
            .track_focus(focus_handle)
            .w(bounds.width)
            .h(bounds.height)
            .bg(hsla(0., 0., 0., 0.5))
            .flex()
            .items_center()
            .justify_center()
            .on_key_down(move |event, window, cx| {
                if event.keystroke.key == "escape" {
                    close_for_escape(window, cx);
                } else if event.keystroke.key == "enter" {
                    close_for_confirm(window, cx);
                    confirm_action(window, cx);
                }
            })
            .child(
                div()
                    .id("confirm-card")
                    .flex()
                    .flex_col()
                    .gap(spacing::X2)
                    .w(px(340.))
                    .p(spacing::X5)
                    .rounded(radius::CARD)
                    .bg(palette.overlay)
                    .border_1()
                    .border_color(palette.overlay_border)
                    .shadow(vec![box_shadow(px(0.), px(10.), px(28.), px(0.), hsla(0., 0., 0., 0.65))])
                    .on_mouse_down_out(move |_, window, cx| close_for_outside(window, cx))
                    .child(div().text_size(type_scale::TITLE.font_size).line_height(type_scale::TITLE.line_height).text_color(palette.text).child(request.title.clone()))
                    .when_some(request.body.clone(), |el, body| {
                        el.child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).child(body))
                    })
                    .child(
                        div().flex().flex_row().justify_end().gap(spacing::X2).pt(spacing::X2).child(Button::new("confirm-cancel", "Cancel").on_click({
                            let on_close = on_close.clone();
                            move |_, window, cx| on_close(window, cx)
                        })).child(Button::new("confirm-action", request.action.clone()).kind(if request.danger { ButtonKind::Danger } else { ButtonKind::Primary }).on_click({
                            let on_confirm = request.on_confirm.clone();
                            move |_, window, cx| {
                                on_close(window, cx);
                                on_confirm(window, cx);
                            }
                        })),
                    ),
            ),
    ))
}
