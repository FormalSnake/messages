//! The incoming-call banner. `app.rs` mounts `FaceTimeBanner` absolutely
//! positioned over the thread.
//! Answering asks the Mac to pick up and hand us a FaceTime Link to open in a
//! browser; the Mac itself leaves the call about 15 s after we join
//! (bluebubbles-helper#38).

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::store::FaceTimeCall;

use crate::bridge::{Bridge, StoreHandle, Topic};
use crate::icons::{Icon, IconName};
use crate::primitives::{Button, ButtonKind, IconButton};
use crate::theme::{Theme, radius, spacing, type_scale};

/// Sits `offset` in from the right edge: 12 px, or clear of the details
/// panel while it is open.
pub struct FaceTimeBanner {
    offset: Pixels,
}

impl FaceTimeBanner {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade().into();
        Bridge::watch(cx, Topic::FaceTime, weak);
        Self { offset: px(12.) }
    }

    pub fn set_offset(&mut self, offset: Pixels, cx: &mut Context<Self>) {
        if self.offset != offset {
            self.offset = offset;
            cx.notify();
        }
    }

    fn answer(&self, cx: &mut Context<Self>) {
        let Some(store) = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone()) else { return };
        let task_store = store.clone();
        store.spawn(async move { task_store.answer_facetime().await });
    }

    fn decline(&self, cx: &mut Context<Self>) {
        let Some(store) = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone()) else { return };
        let task_store = store.clone();
        store.spawn(async move { task_store.decline_facetime().await });
    }

    fn dismiss(&self, cx: &mut Context<Self>) {
        if let Some(store) = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone()) {
            store.dismiss_facetime();
        }
    }
}

fn status_line(call: &FaceTimeCall) -> String {
    use messages_core::store::FaceTimeCallStatus::*;
    match call.status {
        Incoming if call.can_answer => "Incoming FaceTime".to_owned(),
        Incoming => "Incoming FaceTime, answer on your Mac".to_owned(),
        Answering => "Answering on the Mac and creating a link…".to_owned(),
        Ready => "Link ready. The Mac leaves the call about 15 seconds after you join.".to_owned(),
        Ended => "Call ended".to_owned(),
        Failed => call.error.clone().unwrap_or_else(|| "Could not answer".to_owned()),
    }
}

impl Render for FaceTimeBanner {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        let Some(store) = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone()) else {
            return div().into_any_element();
        };
        let Some(call) = store.state().facetime.clone() else {
            return div().into_any_element();
        };
        use messages_core::store::FaceTimeCallStatus::*;
        let who = call.from.clone().unwrap_or_else(|| "Unknown caller".to_owned());
        let line = status_line(&call);
        let failed = matches!(call.status, Failed);
        let can_answer = matches!(call.status, Incoming) && call.can_answer;
        let ready_link = matches!(call.status, Ready).then(|| call.link.clone()).flatten();


        div()
            .id("facetime-banner")
            .absolute()
            .top(spacing::X3)
            .right(self.offset)
            .flex()
            .flex_row()
            .items_center()
            .gap(spacing::X3)
            .pl(spacing::X4)
            .pr(spacing::X2)
            .py(spacing::X3)
            .rounded(radius::CARD)
            .bg(palette.overlay)
            .border_1()
            .border_color(palette.overlay_border)
            .shadow(crate::primitives::overlay_shadows(&palette))
            .child(Icon::new(IconName::Video).size(px(20.)).color(if failed { palette.danger } else { palette.online }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .max_w(px(320.))
                    .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).font_weight(FontWeight::SEMIBOLD).text_color(palette.text).text_ellipsis().child(who))
                    .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).child(line)),
            )
            .when(can_answer, |el| {
                el.child(Button::new("facetime-answer", "Answer in browser").kind(ButtonKind::Primary).on_click(cx.listener(|this, _, _window, cx| this.answer(cx))))
            })
            .when_some(ready_link, |el, link| {
                el.child(Button::new("facetime-join", "Join").kind(ButtonKind::Primary).on_click(move |_, _window, _cx| messages_core::open::open_external(&link)))
            })
            .when(can_answer, |el| el.child(Button::new("facetime-decline", "Decline").kind(ButtonKind::Danger).on_click(cx.listener(|this, _, _window, cx| this.decline(cx)))))
            .child(IconButton::new("facetime-dismiss", IconName::Close, "Dismiss").on_click(cx.listener(|this, _, _window, cx| this.dismiss(cx)))).into_any_element()
    }
}
