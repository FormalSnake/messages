//! D1 stub. Port of `apps/desktop/src/ui/switcher.tsx`. `app.rs` mounts
//! `Switcher` when Cmd/Ctrl+K opens it.

use gpui_kit::*;

use crate::theme::Theme;

pub struct Switcher;

impl Switcher {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for Switcher {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        div().w(px(480.)).p(px(16.)).bg(palette.overlay).border_1().border_color(palette.overlay_border).rounded(px(10.)).child(
            div().text_color(palette.tertiary).child("switcher.rs (D1)"),
        )
    }
}
