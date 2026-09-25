//! D1 stub. Port of `apps/desktop/src/ui/sidebar.tsx`. `app.rs` mounts
//! `Sidebar` in the left column; replace this body without changing that
//! call site's shape (`Sidebar::new(window, cx)`).

use gpui_kit::*;

use crate::theme::Theme;

pub struct Sidebar;

impl Sidebar {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        div().size_full().bg(palette.sidebar).flex().items_center().justify_center().child(
            div().text_color(palette.tertiary).child("sidebar.rs (D1)"),
        )
    }
}
