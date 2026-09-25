//! D3 stub. Port of `apps/desktop/src/ui/gif-picker.tsx`. Owned by D3; not
//! mounted directly by app.rs (the composer opens it).

use gpui_kit::*;

pub struct GifPicker;

impl Render for GifPicker {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("gif_picker.rs (D3)")
    }
}
