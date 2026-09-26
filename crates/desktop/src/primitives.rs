//! Port of `apps/desktop/src/ui/primitives.tsx`: Avatar, IconButton, Button,
//! Divider, SectionLabel and the password-masking text field wrapper.

use gpui_kit::component::box_shadow;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::{Chat, Handle, chat_title, handle_name};

use crate::icons::{Icon, IconName};
use crate::theme::{Palette, Theme, radius, spacing, type_scale};

/// Always painted, transparent until the row is the keyboard cursor, so
/// taking the cursor never reflows the row. GPUI gives no separate focus
/// event a list row can drive its own ring from, so callers track the cursor
/// themselves and pass it in here.
pub fn ring(active: bool, color: Hsla, transparent: Hsla) -> (Pixels, Hsla) {
    (px(2.), if active { color } else { transparent })
}

pub fn overlay_shadows() -> Vec<BoxShadow> {
    vec![box_shadow(px(0.), px(10.), px(28.), px(0.), hsla(0., 0., 0., 0.65))]
}

#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    label: SharedString,
    size: Pixels,
    hit: Pixels,
    color: Option<Hsla>,
    active: bool,
    disabled: bool,
    strong: bool,
    on_click: Option<std::rc::Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

/// IconButton and tapback tooltips wait a beat longer than the app-wide
/// 500 ms default (primitives.tsx:77, menus.tsx:192).
pub const TOOLTIP_DELAY: std::time::Duration = std::time::Duration::from_millis(600);

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName, label: impl Into<SharedString>) -> Self {
        Self { id: id.into(), icon, label: label.into(), size: px(16.), hit: px(28.), color: None, active: false, disabled: false, strong: false, on_click: None }
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn hit(mut self, hit: Pixels) -> Self {
        self.hit = hit;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn strong(mut self, strong: bool) -> Self {
        self.strong = strong;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(std::rc::Rc::new(handler));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = Theme::get(cx);
        let fg = if self.active { palette.accent } else { self.color.unwrap_or(palette.secondary) };
        let disabled = self.disabled;
        let label = self.label.clone();
        let selector = self.id.to_string();

        div()
            .id(self.id)
            .debug_selector(|| selector)
            .w(self.hit)
            .h(self.hit)
            .occlude()
            .rounded(radius::CONTROL)
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .when(self.disabled, |el| el.opacity(0.4))
            .when(self.active, |el| el.bg(palette.selected_soft))
            .when(!disabled, |el| {
                el.hover(move |style| style.bg(if self.active { palette.selected_soft } else { palette.hover_wash }))
                    .active(move |style| style.bg(if self.active { palette.selected_soft } else { palette.press_wash }))
                    .when_some(self.on_click, |el, handler| {
                        let on_key = handler.clone();
                        el.tab_index(0).on_click(move |event, window, cx| handler(event, window, cx)).on_key_down(move |event: &KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                on_key(&ClickEvent::default(), window, cx);
                            }
                        })
                    })
            })
            .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
            .tooltip_show_delay(TOOLTIP_DELAY)
            .child(Icon::new(self.icon).size(self.size).color(fg).strong(self.strong))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Danger,
}

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    kind: ButtonKind,
    disabled: bool,
    on_click: Option<std::rc::Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self { id: id.into(), label: label.into(), kind: ButtonKind::Secondary, disabled: false, on_click: None }
    }

    pub fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(std::rc::Rc::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = Theme::get(cx);
        let (fill, fg) = match self.kind {
            ButtonKind::Primary => (palette.accent, palette.on_accent),
            ButtonKind::Danger => (palette.danger, palette.on_accent),
            ButtonKind::Secondary => (palette.raised, palette.text),
        };
        let disabled = self.disabled;
        let selector = self.id.to_string();

        div()
            .id(self.id)
            .debug_selector(|| selector)
            .h(px(30.))
            .px(spacing::X3)
            .rounded(radius::CONTROL)
            .bg(fill)
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .when(disabled, |el| el.opacity(0.4))
            .when(!disabled, |el| {
                el.hover(|style| style.opacity(0.88)).active(|style| style.opacity(0.7)).when_some(self.on_click, |el, handler| {
                    let on_key = handler.clone();
                    el.tab_index(0).on_click(move |event, window, cx| handler(event, window, cx)).on_key_down(move |event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            on_key(&ClickEvent::default(), window, cx);
                        }
                    })
                })
            })
            .child(div().text_color(fg).font_weight(FontWeight::SEMIBOLD).text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).child(self.label))
    }
}

/// Below this a monogram is a smudge, so the disc stays plain.
const MONOGRAM_MIN: f32 = 18.;

// None of the elements below register a click or id, so unlike the TS/gpuix
// renderer (see CLAUDE.md's gpuix rules) they never capture a click meant for
// their row: GPUI only hit-tests elements that ask for it.

fn photo_avatar(src: &str, size: Pixels) -> impl IntoElement {
    let inner = size - px(2.);
    div()
        .w(size)
        .h(size)
        .rounded(size / 2.)
        .flex_shrink_0()
        .overflow_hidden()
        .border_1()
        .border_color(hsla(0., 0., 1., 0.1))
        .child(img(crate::attachments::sized_image_source(std::path::Path::new(src), inner, inner, ObjectFit::Cover)).w(inner).h(inner).rounded(inner / 2.).object_fit(ObjectFit::Cover))
}

fn monogram_avatar(label: &str, size: Pixels) -> impl IntoElement {
    let monogram = messages_core::format::initials(label);
    let bg = linear_gradient(180., linear_color_stop(rgb(0xa2a2a8), 0.), linear_color_stop(rgb(0x78787e), 1.));
    let size_f: f32 = size.into();
    div().w(size).h(size).rounded(size / 2.).flex_shrink_0().flex().items_center().justify_center().bg(bg).child(if size_f < MONOGRAM_MIN {
        div().into_any_element()
    } else if monogram == "#" {
        Icon::new(IconName::Person).size(px((size_f * 0.52).round())).color(white()).strong(true).into_any_element()
    } else {
        div().text_color(white()).text_size(px((size_f * 0.38).round())).line_height(px((size_f * 0.46).round())).child(monogram).into_any_element()
    })
}

/// Messages' own group picture: the first few people clustered inside one
/// circle, so it stays round under a selection ring or an unread badge.
fn group_avatar(chat: &Chat, size: Pixels, palette: &Palette) -> impl IntoElement {
    let people: Vec<&Handle> = chat.participants.iter().take(4).collect();
    let size_f: f32 = size.into();
    let small = px((if people.len() <= 2 { size_f * 0.42 } else { size_f * 0.36 }).round());
    let rows: Vec<Vec<&Handle>> = match people.len() {
        0..=2 => vec![people.clone()],
        3 => vec![vec![people[0]], people[1..].to_vec()],
        _ => vec![people[0..2].to_vec(), people[2..4].to_vec()],
    };
    div().w(size).h(size).rounded(size / 2.).flex_shrink_0().overflow_hidden().bg(palette.raised_hover).flex().flex_col().items_center().justify_center().gap(px(1.)).children(
        rows.into_iter().map(|row| div().flex().flex_row().gap(px(1.)).children(row.into_iter().map(|person| avatar_for_handle(Some(person), None, small, palette)))),
    )
}

fn avatar_for_handle(handle: Option<&Handle>, chat: Option<&Chat>, size: Pixels, palette: &Palette) -> gpui_kit::AnyElement {
    if let Some(chat) = chat {
        if chat.is_group {
            return if let Some(icon) = chat.icon.as_deref() { photo_avatar(icon, size).into_any_element() } else { group_avatar(chat, size, palette).into_any_element() };
        }
    }
    let person = handle.or_else(|| chat.and_then(|chat| chat.participants.first()));
    let label = person.map(handle_name).map(str::to_owned).or_else(|| chat.map(chat_title)).unwrap_or_else(|| "?".to_owned());
    if let Some(avatar) = person.and_then(|person| person.avatar.as_deref()) {
        return photo_avatar(avatar, size).into_any_element();
    }
    monogram_avatar(&label, size).into_any_element()
}

/// Messages uses a neutral gradient monogram for anyone without a photo.
/// Colour would imply meaning it does not have.
pub fn avatar(handle: Option<&Handle>, chat: Option<&Chat>, size: Pixels, cx: &App) -> gpui_kit::AnyElement {
    avatar_for_handle(handle, chat, size, &Theme::get(cx))
}

pub fn divider(color: Hsla) -> impl IntoElement {
    div().h(px(1.)).flex_shrink_0().bg(color)
}

/// The small all-caps-weight heading above a group of rows.
pub fn section_label(label: impl Into<SharedString>, color: Hsla, inset: Pixels) -> impl IntoElement {
    div()
        .text_size(type_scale::MICRO.font_size)
        .line_height(type_scale::MICRO.line_height)
        .text_color(color)
        .pl(inset)
        .pr(inset)
        .pb(spacing::X1)
        .child(label.into())
}

/// A single-line text field. `secure` shows a mask-toggle eye button, the
/// Rust equivalent of the TS bullet-masking trick (gpui-component's `Input`
/// has real password masking, so there is no need to fake it with bullets).
pub fn new_input_state(window: &mut Window, cx: &mut App, placeholder: impl Into<SharedString>, secure: bool) -> Entity<InputState> {
    cx.new(|cx| {
        let mut state = InputState::new(window, cx).placeholder(placeholder);
        if secure {
            state = state.masked(true);
        }
        state
    })
}

pub fn text_field(state: &Entity<InputState>) -> impl IntoElement {
    Input::new(state).bordered(true)
}
