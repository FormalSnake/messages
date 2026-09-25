//! D1 stub. Port of `apps/desktop/src/ui/new-chat.tsx`. `app.rs` mounts
//! `NewChat` in the main pane when composing a new conversation.

use gpui_kit::*;

use crate::theme::Theme;

pub struct NewChat;

impl NewChat {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for NewChat {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        div().size_full().flex().items_center().justify_center().child(div().text_color(palette.tertiary).child("new_chat.rs (D1)"))
    }
}
