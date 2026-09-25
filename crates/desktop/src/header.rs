//! D3 stub. Port of the conversation header in `apps/desktop/src/ui/header.tsx`.
//! `app.rs` mounts `ConversationHeader` above `Thread`.

use gpui_kit::*;

use crate::theme::{Theme, TITLEBAR_HEIGHT};

pub struct ConversationHeader;

impl ConversationHeader {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for ConversationHeader {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        div().w_full().h(TITLEBAR_HEIGHT).flex_shrink_0().border_b_1().border_color(palette.sidebar_border).flex().items_center().px(px(16.)).child(
            div().text_color(palette.tertiary).child("header.rs (D3)"),
        )
    }
}
