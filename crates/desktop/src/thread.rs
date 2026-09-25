//! D2 stub. Port of `apps/desktop/src/ui/thread.tsx`. `app.rs` mounts
//! `Thread` in the main pane between the header and the composer.

use gpui_kit::*;

use crate::theme::Theme;

pub struct Thread;

impl Thread {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for Thread {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        div().size_full().flex().items_center().justify_center().child(div().text_color(palette.tertiary).child("thread.rs (D2)"))
    }
}
