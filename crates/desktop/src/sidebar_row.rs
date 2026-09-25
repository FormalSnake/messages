//! D1 stub. Port of the row component in `apps/desktop/src/ui/sidebar.tsx`
//! (lines ~112-235). Owned by D1; not mounted directly by app.rs.

use gpui_kit::*;

pub struct SidebarRow;

impl Render for SidebarRow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("sidebar_row.rs (D1)")
    }
}
