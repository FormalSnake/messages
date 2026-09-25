//! D2 stub. Port of the "Thread" reply view (docs/rust-parity.md's app.test.tsx
//! note: "Reply quote opens the Thread view with 1 reply and Back"). Owned by
//! D2; not mounted directly by app.rs.

use gpui_kit::*;

pub struct ReplyThread;

impl Render for ReplyThread {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("reply_thread.rs (D2)")
    }
}
