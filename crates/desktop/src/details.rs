//! D3 stub. Port of `InfoPanel` in `apps/desktop/src/ui/header.tsx` (the
//! details panel, ~lines 428-449 of docs/rust-parity.md). `app.rs` mounts
//! `InfoPanel` in the sliding info column.

use gpui_kit::*;

use crate::theme::Theme;

pub struct InfoPanel;

impl InfoPanel {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for InfoPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        div().size_full().bg(palette.sidebar).flex().items_center().justify_center().child(div().text_color(palette.tertiary).child("details.rs (D3)"))
    }
}
