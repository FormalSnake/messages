//! The "Thread" view: a message and every reply to it, opened from
//! "N replies", with a Back button. The main thread computes its rows and
//! hands them over on every change.

use gpui_kit::*;
use messages_core::MessagesStore;

use crate::icons::{Icon, IconName};
use crate::theme::{Theme, radius, spacing, type_scale};
use crate::thread::{RowList, Slot, Thread};

pub struct ReplyThread {
    thread: WeakEntity<Thread>,
    rows: RowList,
    label: SharedString,
}

impl ReplyThread {
    pub fn new(store: MessagesStore, thread: WeakEntity<Thread>) -> Self {
        ReplyThread { rows: RowList::new(store, thread.clone(), ListAlignment::Top), thread, label: SharedString::default() }
    }

    pub(crate) fn set(&mut self, slots: Vec<Slot>, replies: usize, is_group: bool, cx: &mut Context<Self>) {
        self.label = if replies == 1 { "1 reply".into() } else { format!("{replies} replies").into() };
        self.rows.apply(slots, is_group, None, false, cx);
        cx.notify();
    }
}

impl Render for ReplyThread {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        let thread = self.thread.clone();
        div()
            .flex_grow(1.)
            .min_h(px(0.))
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(spacing::X2)
                    .h(px(40.))
                    .px(spacing::X3)
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(palette.sidebar_border)
                    .child(
                        div()
                            .id("thread-back")
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(spacing::X1)
                            .h(px(28.))
                            .pl(spacing::X1)
                            .pr(spacing::X2)
                            .rounded(radius::CONTROL)
                            .cursor_pointer()
                            .hover(move |style| style.bg(palette.raised))
                            .on_click(move |_, _, cx| {
                                let _ = thread.update(cx, |thread, cx| thread.close_reply_thread(cx));
                            })
                            .child(Icon::new(IconName::ChevronLeft).size(px(14.)).color(palette.accent))
                            .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).text_color(palette.accent).child("Back")),
                    )
                    .child(div().text_size(type_scale::TITLE.font_size).line_height(type_scale::TITLE.line_height).font_weight(FontWeight::SEMIBOLD).text_color(palette.text).child("Thread"))
                    .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).child(self.label.clone())),
            )
            .child(self.rows.width_probe())
            .child(self.rows.element().flex_grow(1.).w_full().pb(spacing::X2))
    }
}
