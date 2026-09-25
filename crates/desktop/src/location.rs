//! D3 stub. Port of `apps/desktop/src/ui/location.tsx`. Owned by D3; not
//! mounted directly by app.rs (it sits inside the details panel).

use gpui_kit::*;

pub struct LocationCard;

impl Render for LocationCard {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("location.rs (D3)")
    }
}
