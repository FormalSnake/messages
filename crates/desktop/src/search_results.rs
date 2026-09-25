//! D1 stub. Port of the search-results list in `apps/desktop/src/ui/sidebar.tsx`
//! (lines ~252-284, 447-456). Owned by D1; not mounted directly by app.rs.

use gpui_kit::*;

pub struct SearchResults;

impl Render for SearchResults {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("search_results.rs (D1)")
    }
}
