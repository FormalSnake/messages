//! Port of the toast in `apps/desktop/src/ui/app.tsx` (lines ~217-225, 388-417):
//! a non-interactive pill, centred at the bottom, that rises and fades in.

use gpui_kit::component::box_shadow;
use gpui_kit::*;

use crate::icons::{Icon, IconName};
use crate::theme::{Theme, radius, spacing, type_scale};

/// The toast rises this far as it fades in.
pub const RISE: Pixels = px(8.);
pub const BOTTOM: Pixels = px(72.);
pub const MAX_WIDTH: Pixels = px(480.);

/// `open=false` is the exit frame; `motion::Presence` in the caller keeps the
/// text mounted through the fade so it does not blank before it is gone.
pub fn toast(message: &str, open: bool, cx: &App) -> impl IntoElement {
    let palette = Theme::get(cx);
    div()
        .absolute()
        .inset_0()
        .flex()
        .flex_row()
        .justify_center()
        .items_end()
        .pb(if open { BOTTOM } else { BOTTOM - RISE })
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(spacing::X2)
                .px(spacing::X3)
                .py(spacing::X2)
                .rounded(radius::MENU)
                .bg(palette.overlay)
                .border_1()
                .border_color(palette.overlay_border)
                .shadow(vec![box_shadow(px(0.), px(10.), px(28.), px(0.), hsla(0., 0., 0., 0.65))])
                .max_w(MAX_WIDTH)
                .opacity(if open { 1. } else { 0. })
                .child(Icon::new(IconName::Alert).size(px(14.)).color(palette.danger))
                .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.text).child(message.to_owned())),
        )
}
