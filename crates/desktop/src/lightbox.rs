//! D2 stub. Port of `apps/desktop/src/ui/lightbox.tsx`. `app.rs` mounts
//! `Lightbox` over the whole window when an attachment is opened full-screen.

use gpui_kit::*;

use crate::theme::Theme;

pub struct Lightbox;

impl Lightbox {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for Lightbox {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        div().size_full().bg(hsla(0., 0., 0., 0.9)).flex().items_center().justify_center().child(div().text_color(palette.text).child("lightbox.rs (D2)"))
    }
}
