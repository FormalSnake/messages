//! The search-results list: the "Messages" section header and one result row.
//! `sidebar.rs` builds these into its own virtualized list; results are not
//! persistent entities like `SidebarRow`, since the whole set is rebuilt
//! together whenever the search query or its results change.

use gpui_kit::*;
use messages_core::{Chat, Message, chat_title, format::format_list_date};

use crate::sidebar_row::{ROW_INSET, preview_text};
use crate::theme::{Palette, radius, spacing, type_scale};

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// The "Messages" label above the result rows.
pub fn section_header(palette: Palette) -> impl IntoElement {
    div()
        .text_size(type_scale::MICRO.font_size)
        .line_height(type_scale::MICRO.line_height)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette.tertiary)
        .pl(ROW_INSET)
        .pt(spacing::X3)
        .pb(spacing::X1)
        .child("Messages")
}

/// One matched message, jumping into its conversation on click.
pub fn search_result_row(id: ElementId, message: &Message, chat: &Chat, palette: Palette, on_open: impl Fn(&mut Window, &mut App) + 'static) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(px(1.))
        .px(ROW_INSET)
        .py(spacing::X2)
        .rounded(radius::ROW)
        .cursor_pointer()
        .flex_shrink_0()
        .hover(move |style| style.bg(palette.raised))
        .active(move |style| style.bg(palette.raised_hover))
        .on_click(move |_, window, cx| on_open(window, cx))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(spacing::X1)
                .child(
                    div()
                        .flex_grow(1.)
                        .min_w(px(0.))
                        .text_size(type_scale::BODY.font_size)
                        .line_height(type_scale::BODY.line_height)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text)
                        .text_ellipsis()
                        .child(chat_title(chat)),
                )
                .child(
                    div()
                        .text_size(type_scale::MICRO.font_size)
                        .line_height(type_scale::MICRO.line_height)
                        .text_color(palette.tertiary)
                        .flex_shrink_0()
                        .child(format_list_date(message.date, now_ms())),
                ),
        )
        .child(
            div()
                .text_size(type_scale::PREVIEW.font_size)
                .line_height(type_scale::PREVIEW.line_height)
                .text_color(palette.secondary)
                .line_clamp(2)
                .w_full()
                .min_w(px(0.))
                .child(preview_text(Some(message), chat)),
        )
}
