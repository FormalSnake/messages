//! D3 stub. Port of `apps/desktop/src/ui/composer.tsx`. `app.rs` mounts
//! `Composer` at the bottom of the main pane.

use gpui_kit::*;

use crate::theme::Theme;

pub struct Composer;

impl Composer {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for Composer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        div().w_full().h(px(52.)).flex_shrink_0().border_t_1().border_color(palette.sidebar_border).flex().items_center().px(px(16.)).child(
            div().text_color(palette.tertiary).child("composer.rs (D3)"),
        )
    }
}
