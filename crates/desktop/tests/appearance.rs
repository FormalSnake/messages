//! Tier 2: the appearance audit over every window in light, dark and matugen, on both backend
//! kinds, on the demo conversations with a thread open. Fails on a colour that is not a theme
//! token and on text under WCAG contrast.

fn main() {
    nd::audit::Audit::new("messages", messages::app).window("main", |cx| messages::open_demo(cx).id()).run()
}
