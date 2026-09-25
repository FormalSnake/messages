//! D2 stub. Port of the bubble component in `apps/desktop/src/ui/thread.tsx`
//! (lines ~523-534 in docs/rust-parity.md). One entity per message, watching
//! `Topic::Message(guid)`. Owned by D2; not mounted directly by app.rs.

use gpui_kit::*;

pub struct Bubble;

impl Render for Bubble {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("bubble.rs (D2)")
    }
}
