//! Bubble text that joins the window-wide text selection gpui-base runs
//! under the `Root`: drag across one bubble or many, Cmd+C copies what is
//! lit, Escape clears it. Each run keeps its own participant handle in
//! element state, as gpui-base's `SelectableText` does, and its place in
//! reading order comes from the message date so a drag down the thread
//! copies in thread order. The text itself is whatever `emoji_font::styled_text`
//! built, so mentions, links and colour emoji paint as before.

use std::ops::Range;

use gpui_base::{TextSelection, TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};
use gpui_kit::*;

pub struct Selectable {
    id: ElementId,
    text: SharedString,
    styled: StyledText,
    order: u64,
    color: Hsla,
}

/// `styled` laid out and painted as it is, with `text` as the copy source and
/// `order` its place in the thread.
pub fn selectable(id: impl Into<ElementId>, text: SharedString, styled: StyledText, order: u64, color: Hsla) -> Selectable {
    Selectable { id: id.into(), text, styled, order, color }
}

/// Reading order for the `block`th text of a message sent at `date`, leaving
/// room for the words a mixed-size body is split into.
pub fn order(date: i64, block: usize, word: usize) -> u64 {
    (date.max(0) as u64) * 4096 + (block.min(15) as u64) * 256 + word.min(255) as u64
}

fn paint_selection(layout: &TextLayout, range: Range<usize>, color: Hsla, window: &mut Window) {
    let (Some(start), Some(end)) = (layout.position_for_index(range.start), layout.position_for_index(range.end)) else { return };
    let bounds = layout.bounds();
    let line = layout.line_height();
    let mut quads = Vec::with_capacity(3);
    if start.y == end.y {
        quads.push(Bounds::from_corners(start, point(end.x, end.y + line)));
    } else {
        quads.push(Bounds::from_corners(start, point(bounds.right(), start.y + line)));
        if end.y > start.y + line {
            quads.push(Bounds::from_corners(point(bounds.left(), start.y + line), point(bounds.right(), end.y)));
        }
        quads.push(Bounds::from_corners(point(bounds.left(), end.y), point(end.x, end.y + line)));
    }
    for bounds in quads {
        window.paint_quad(fill(bounds, color));
    }
}

impl IntoElement for Selectable {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Selectable {
    type RequestLayoutState = TextSelectionHandle;
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, global_id: Option<&GlobalElementId>, inspector_id: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        let text = self.text.clone();
        let handle = window.with_element_state(global_id.expect("selectable text has an id"), |retained: Option<TextSelectionHandle>, _| {
            let handle = retained.unwrap_or_else(|| TextSelectionHandle::new(text, cx));
            (handle.clone(), handle)
        });
        let (layout_id, ()) = self.styled.request_layout(global_id, inspector_id, window, cx);
        (layout_id, handle)
    }

    fn prepaint(&mut self, global_id: Option<&GlobalElementId>, inspector_id: Option<&InspectorElementId>, bounds: Bounds<Pixels>, handle: &mut Self::RequestLayoutState, window: &mut Window, cx: &mut App) -> Self::PrepaintState {
        self.styled.prepaint(global_id, inspector_id, bounds, &mut (), window, cx);
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        handle.register(TextSelectionRegistration::new(hitbox.clone(), bounds).with_document_order(self.order).with_text_bounds(vec![bounds]), window, cx);
        hitbox
    }

    fn paint(&mut self, global_id: Option<&GlobalElementId>, inspector_id: Option<&InspectorElementId>, bounds: Bounds<Pixels>, handle: &mut Self::RequestLayoutState, _: &mut Self::PrepaintState, window: &mut Window, cx: &mut App) {
        let layout = self.styled.layout().clone();
        let before = TextSelection::selected_text(window, cx);
        let projection = handle.update_runs(&[TextSelectionRun::new(self.text.clone(), layout.clone(), bounds).with_document_order(self.order)], cx);
        if before != TextSelection::selected_text(window, cx) {
            window.refresh();
        }
        for range in projection.ranges().iter().flatten().cloned() {
            paint_selection(&layout, range, self.color, window);
        }
        self.styled.paint(global_id, inspector_id, bounds, &mut (), &mut (), window, cx);
    }
}
