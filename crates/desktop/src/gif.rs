//! D2 stub. Owns the per-file GIF clock (docs/rust-architecture.md's "GIF
//! animation" performance note): decode once per shared path, step frames on
//! one clock per file, notify only the bubbles showing it. Owned by D2; not
//! mounted directly by app.rs.

use gpui_kit::*;

pub struct GifView;

impl Render for GifView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("gif.rs (D2)")
    }
}
