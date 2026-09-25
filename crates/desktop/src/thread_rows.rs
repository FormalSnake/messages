//! D2 stub. Grouping, separators and receipts for the thread list (the part
//! of `apps/desktop/src/ui/thread.tsx` that turns messages into rows). Owned
//! by D2; not mounted directly by app.rs.

use gpui_kit::*;

pub struct ThreadRows;

impl Render for ThreadRows {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("thread_rows.rs (D2)")
    }
}
