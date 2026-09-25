//! Port of `apps/desktop/src/ui/menus.tsx`: the `MenuItem` data model other
//! screens build, and the context menu that renders it. `MenuItem` is the
//! "MenuItem data screens build" piece D0 owns; D1/D2/D3 construct
//! `Vec<MenuItem>` for their own rows and call `open_menu`.

use gpui_kit::component::box_shadow;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::TapbackKind;

use crate::icons::{Icon, IconName};
use crate::theme::{Theme, radius, spacing, type_scale};

/// Also the Ctrl/Cmd+1..6 keyboard tapback order in app.rs.
pub const TAPBACK_ORDER: [TapbackKind; 6] =
    [TapbackKind::Love, TapbackKind::Like, TapbackKind::Dislike, TapbackKind::Laugh, TapbackKind::Emphasize, TapbackKind::Question];

fn tapback_label(kind: TapbackKind) -> &'static str {
    match kind {
        TapbackKind::Love => "Love",
        TapbackKind::Like => "Like",
        TapbackKind::Dislike => "Dislike",
        TapbackKind::Laugh => "Laugh",
        TapbackKind::Emphasize => "Emphasize",
        TapbackKind::Question => "Question",
        TapbackKind::Emoji => "Emoji",
    }
}

pub enum MenuItem {
    Item {
        label: SharedString,
        icon: Option<IconName>,
        /// `Rc`, not `Box`: `AppRoot` keeps the open `MenuRequest` in its own
        /// state and re-renders it (by reference) on every notify, so the
        /// handler has to be cloneable into each render's `'static` click
        /// closure rather than moved out once.
        on_select: std::rc::Rc<dyn Fn(&mut Window, &mut App)>,
        danger: bool,
        disabled: bool,
        /// Right-aligned hint, already formatted for the platform by `shortcut()`.
        shortcut: Option<SharedString>,
    },
    Separator,
    Header(SharedString),
    /// A line of information the pointer does nothing with.
    Note(SharedString),
    Tapbacks {
        chat_guid: String,
        message_guid: String,
        /// The kind I already reacted with, if any, so the row can highlight it.
        mine: Option<TapbackKind>,
        on_select: std::rc::Rc<dyn Fn(TapbackKind, &mut Window, &mut App)>,
    },
}

impl MenuItem {
    pub fn item(label: impl Into<SharedString>, on_select: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        MenuItem::Item { label: label.into(), icon: None, on_select: std::rc::Rc::new(on_select), danger: false, disabled: false, shortcut: None }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        if let MenuItem::Item { icon: slot, .. } = &mut self {
            *slot = Some(icon);
        }
        self
    }

    pub fn danger(mut self) -> Self {
        if let MenuItem::Item { danger, .. } = &mut self {
            *danger = true;
        }
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        if let MenuItem::Item { disabled: slot, .. } = &mut self {
            *slot = disabled;
        }
        self
    }

    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        if let MenuItem::Item { shortcut: slot, .. } = &mut self {
            *slot = Some(shortcut.into());
        }
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Hangs the menu off the point, like every context menu.
    Below,
    /// Rests its bottom edge on the point, which is how the tapback picker
    /// sits on top of the bubble it belongs to.
    Above,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Start,
    /// Centre the menu on `x` instead of starting there. Used by the picker.
    Center,
}

pub struct MenuRequest {
    pub position: Point<Pixels>,
    pub items: Vec<MenuItem>,
    pub placement: Placement,
    pub align: Align,
    pub min_width: Option<Pixels>,
}

impl MenuRequest {
    pub fn at(position: Point<Pixels>, items: Vec<MenuItem>) -> Self {
        Self { position, items, placement: Placement::Below, align: Align::Start, min_width: None }
    }

    pub fn above(mut self) -> Self {
        self.placement = Placement::Above;
        self
    }

    pub fn centered(mut self) -> Self {
        self.align = Align::Center;
        self
    }

    pub fn min_width(mut self, width: Pixels) -> Self {
        self.min_width = Some(width);
        self
    }

    fn anchor(&self) -> Anchor {
        match (self.placement, self.align) {
            (Placement::Below, Align::Start) => Anchor::TopLeft,
            (Placement::Below, Align::Center) => Anchor::TopCenter,
            (Placement::Above, Align::Start) => Anchor::BottomLeft,
            (Placement::Above, Align::Center) => Anchor::BottomCenter,
        }
    }
}

/// Renders a `MenuRequest`. The caller keeps `request` in its own state and
/// re-renders this (by reference) on every notify; `on_close` runs on
/// Escape, a click outside, or after an item activates.
pub fn context_menu(request: &MenuRequest, cx: &App, on_close: impl Fn(&mut Window, &mut App) + Clone + 'static) -> impl IntoElement {
    let pointer_only = request.items.len() == 1 && matches!(request.items[0], MenuItem::Tapbacks { .. });
    let palette = Theme::get(cx);
    let close_for_outside = on_close.clone();

    anchored()
        .position(request.position)
        .anchor(request.anchor())
        .snap_to_window_with_margin(spacing::X2)
        .child(deferred(
            div()
                .id("context-menu")
                .flex()
                .flex_col()
                .when(!pointer_only, |el| el.min_w(request.min_width.unwrap_or(px(196.))))
                .p(spacing::X1)
                .rounded(if pointer_only { radius::PILL } else { radius::MENU })
                .bg(palette.overlay)
                .border_1()
                .border_color(palette.overlay_border)
                .shadow(vec![box_shadow(px(0.), px(10.), px(28.), px(0.), hsla(0., 0., 0., 0.65))])
                .on_mouse_down_out(move |_, window, cx| close_for_outside(window, cx))
                .children(request.items.iter().map(|item| render_item(item, palette, on_close.clone()))),
        ))
}

fn render_item(item: &MenuItem, palette: crate::theme::Palette, on_close: impl Fn(&mut Window, &mut App) + Clone + 'static) -> AnyElement {
    match item {
        MenuItem::Separator => div().h(px(1.)).bg(palette.separator).mt(spacing::X1).mb(spacing::X1).mx(spacing::X2).into_any_element(),
        MenuItem::Header(label) => div()
            .text_size(type_scale::MICRO.font_size)
            .line_height(type_scale::MICRO.line_height)
            .text_color(palette.tertiary)
            .px(spacing::X2)
            .py(spacing::X1)
            .child(label.clone())
            .into_any_element(),
        MenuItem::Note(label) => div()
            .text_size(type_scale::BODY.font_size)
            .line_height(px(28.))
            .text_color(palette.text)
            .px(spacing::X2)
            .h(px(28.))
            .child(label.clone())
            .into_any_element(),
        MenuItem::Tapbacks { chat_guid, message_guid, mine, on_select } => {
            tapback_row(chat_guid.clone(), message_guid.clone(), *mine, on_select.clone(), palette, false, on_close).into_any_element()
        }
        MenuItem::Item { label, icon, on_select, danger, disabled, shortcut } => {
            let (label, icon, danger, disabled, shortcut) = (label.clone(), *icon, *danger, *disabled, shortcut.clone());
            let on_select = on_select.clone();
            let fg = if disabled { palette.secondary } else if danger { palette.danger } else { palette.text };
            let id = ElementId::Name(format!("menu-{label}").into());
            div()
                .id(id)
                .flex()
                .flex_row()
                .items_center()
                .gap(spacing::X2)
                .h(px(28.))
                .px(spacing::X2)
                .rounded(radius::MENU_ITEM)
                .when(disabled, |el| el.opacity(0.4))
                .when(!disabled, |el| {
                    el.hover(|style| style.bg(palette.accent)).on_click(move |_, window, cx| {
                        on_select(window, cx);
                        on_close(window, cx);
                    })
                })
                .child(div().w(px(14.)).flex().items_center().justify_center().flex_shrink_0().when_some(icon, |el, icon| el.child(Icon::new(icon).size(px(14.)).color(fg))))
                .child(div().flex_grow(1.).min_w(px(0.)).text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).text_color(fg).child(label))
                .when_some(shortcut, |el, shortcut| {
                    el.child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).text_color(palette.tertiary).pl(spacing::X3).child(shortcut))
                })
                .into_any_element()
        }
    }
}

fn tapback_row(
    chat_guid: String,
    message_guid: String,
    mine: Option<TapbackKind>,
    on_select: std::rc::Rc<dyn Fn(TapbackKind, &mut Window, &mut App)>,
    palette: crate::theme::Palette,
    bare: bool,
    on_close: impl Fn(&mut Window, &mut App) + Clone + 'static,
) -> impl IntoElement {
    div()
        .id("tapback-row")
        .flex()
        .flex_row()
        .items_center()
        .gap(spacing::X1)
        .px(spacing::X1)
        .when(!bare, |el| el.pb(spacing::X1).mb(spacing::X1).border_b_1().border_color(palette.separator))
        .children(TAPBACK_ORDER.into_iter().enumerate().map(|(index, kind)| {
            let selected = mine == Some(kind);
            let on_select = on_select.clone();
            let on_close = on_close.clone();
            let chat_guid = chat_guid.clone();
            let message_guid = message_guid.clone();
            div()
                .id(ElementId::Name(format!("tapback-{}", tapback_label(kind)).into()))
                .w(px(30.))
                .h(px(30.))
                .rounded(px(15.))
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .bg(if selected { palette.accent } else { palette.overlay })
                .hover(move |style| style.bg(if selected { palette.accent } else { palette.hover_wash }))
                .active(move |style| style.bg(if selected { palette.accent } else { palette.press_wash }))
                .on_click(move |_, window, cx| {
                    let _ = (&chat_guid, &message_guid);
                    on_select(kind, window, cx);
                    on_close(window, cx);
                })
                .tooltip(move |window, cx| Tooltip::new(format!("{}  {}", tapback_label(kind), index + 1)).build(window, cx))
                .child(div().text_size(px(16.)).line_height(px(20.)).text_color(palette.text).child(messages_core::tapback_glyph(kind, None).to_owned()))
        }))
}
