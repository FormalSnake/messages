//! D3 stub. Port of `apps/desktop/src/ui/scheduled.tsx`. Owned by D3; not
//! mounted directly by app.rs (the composer area shows it).

use gpui_kit::*;

pub struct ScheduledList;

impl Render for ScheduledList {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("scheduled.rs (D3)")
    }
}
