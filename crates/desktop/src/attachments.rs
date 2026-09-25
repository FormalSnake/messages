//! D2 stub. Port of the attachment tiles in `apps/desktop/src/ui/thread.tsx`.
//! Owned by D2; not mounted directly by app.rs.

use gpui_kit::*;

pub struct Attachments;

impl Render for Attachments {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("attachments.rs (D2)")
    }
}
