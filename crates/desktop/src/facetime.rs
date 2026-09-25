//! D3 stub. Port of `apps/desktop/src/ui/facetime.tsx`. `app.rs` mounts
//! `FaceTimeBanner` absolutely positioned over the thread.

use gpui_kit::*;

pub struct FaceTimeBanner;

impl FaceTimeBanner {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for FaceTimeBanner {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // Nothing to show until a FaceTime call exists; D3 replaces this with
        // the real banner watching `Topic::FaceTime`.
        div()
    }
}
