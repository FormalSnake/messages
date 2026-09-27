//! The `MenuItem` data model other screens build, and the `ContextMenu`
//! entity that renders it. `MenuItem` is the piece D0 owns; D1/D2/D3
//! construct `Vec<MenuItem>` for their own rows and open a menu through `AppRoot`.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::TapbackKind;

use crate::icons::{Icon, IconName};
use crate::theme::{Palette, Theme, radius, spacing, type_scale};

/// A menu hint reads the way the platform writes it, "⇧⌘U" on macOS,
/// "Ctrl+Shift+U" elsewhere.
pub fn shortcut(key: &str, shift: bool, alt: bool) -> String {
    if cfg!(target_os = "macos") {
        format!("{}{}⌘{}", if shift { "⇧" } else { "" }, if alt { "⌥" } else { "" }, key.to_uppercase())
    } else {
        let key = if key.chars().count() == 1 { key.to_uppercase() } else { key.to_owned() };
        format!("Ctrl+{}{}{}", if shift { "Shift+" } else { "" }, if alt { "Alt+" } else { "" }, key)
    }
}

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
        /// `Rc`, not `Box`: `ContextMenu` keeps the request in its own state
        /// and re-renders it on every notify, so the handler has to be
        /// cloneable into each render's click/keydown closures.
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

    fn is_selectable(&self) -> bool {
        matches!(self, MenuItem::Item { .. })
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

/// The rendered menu: focused on open, Escape closes, Down/Up wrap through
/// the selectable items, Home/End jump to the ends, Enter/Space activates.
/// No typeahead is implemented.
pub struct ContextMenu {
    request: MenuRequest,
    highlighted: Option<usize>,
    focus_handle: FocusHandle,
    on_close: std::rc::Rc<dyn Fn(&mut Window, &mut App)>,
}

impl ContextMenu {
    pub fn open(request: MenuRequest, on_close: impl Fn(&mut Window, &mut App) + 'static, window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self::new(request, on_close, window, cx))
    }

    fn new(request: MenuRequest, on_close: impl Fn(&mut Window, &mut App) + 'static, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        Self { request, highlighted: None, focus_handle, on_close: std::rc::Rc::new(on_close) }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.request.items.len()
    }

    fn selectable(&self) -> Vec<usize> {
        self.request.items.iter().enumerate().filter(|(_, item)| item.is_selectable()).map(|(index, _)| index).collect()
    }

    fn step(&mut self, delta: i32) {
        let selectable = self.selectable();
        if selectable.is_empty() {
            return;
        }
        let len = selectable.len() as i32;
        let current = match self.highlighted.and_then(|h| selectable.iter().position(|&index| index == h)) {
            Some(position) => position as i32,
            None => if delta > 0 { -1 } else { 0 },
        };
        let next = (current + delta).rem_euclid(len);
        self.highlighted = Some(selectable[next as usize]);
    }

    fn jump_start(&mut self) {
        self.highlighted = self.selectable().first().copied();
    }

    fn jump_end(&mut self) {
        self.highlighted = self.selectable().last().copied();
    }

    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(MenuItem::Item { on_select, disabled, .. }) = self.request.items.get(index) else { return };
        if *disabled {
            return;
        }
        let on_select = on_select.clone();
        let on_close = self.on_close.clone();
        on_select(window, cx);
        on_close(window, cx);
    }

    fn activate_highlighted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.highlighted {
            self.activate(index, window, cx);
        }
    }
}

impl Render for ContextMenu {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pointer_only = self.request.items.len() == 1 && matches!(self.request.items[0], MenuItem::Tapbacks { .. });
        let palette = Theme::get(cx);
        let highlighted = self.highlighted;
        let on_close = self.on_close.clone();
        let close_for_outside = on_close.clone();
        let close_for_escape = on_close.clone();

        anchored()
            .position(self.request.position)
            .anchor(self.request.anchor())
            .snap_to_window_with_margin(spacing::X2)
            .child(deferred(
                div()
                    .id("context-menu")
                    .track_focus(&self.focus_handle)
                    .flex()
                    .flex_col()
                    .when(!pointer_only, |el| el.min_w(self.request.min_width.unwrap_or(px(196.))))
                    .p(spacing::X1)
                    .rounded(if pointer_only { radius::PILL } else { radius::MENU })
                    .bg(palette.overlay)
                    .border_1()
                    .border_color(palette.overlay_border)
                    .shadow(crate::primitives::overlay_shadows(&palette))
                    .on_mouse_down_out(move |_, window, cx| close_for_outside(window, cx))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| match event.keystroke.key.as_str() {
                        "escape" => close_for_escape(window, cx),
                        "down" => {
                            this.step(1);
                            cx.notify();
                        }
                        "up" => {
                            this.step(-1);
                            cx.notify();
                        }
                        "home" => {
                            this.jump_start();
                            cx.notify();
                        }
                        "end" => {
                            this.jump_end();
                            cx.notify();
                        }
                        "enter" | "space" => this.activate_highlighted(window, cx),
                        _ => {}
                    }))
                    .children((0..self.request.items.len()).map(|index| render_item(index, &self.request.items[index], palette, highlighted, cx))),
            ))
    }
}

fn render_item(index: usize, item: &MenuItem, palette: Palette, highlighted: Option<usize>, cx: &Context<ContextMenu>) -> AnyElement {
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
            let on_close = cx.entity().downgrade();
            tapback_row(chat_guid.clone(), message_guid.clone(), *mine, on_select.clone(), palette, false, move |window, cx| {
                let _ = on_close.update(cx, |this: &mut ContextMenu, cx| (this.on_close.clone())(window, cx));
            })
            .into_any_element()
        }
        MenuItem::Item { label, icon, danger, disabled, shortcut, .. } => {
            let (label, icon, danger, disabled, shortcut) = (label.clone(), *icon, *danger, *disabled, shortcut.clone());
            let active = highlighted == Some(index) && !disabled;
            let fg = if disabled { palette.secondary } else if danger && !active { palette.danger } else if active { palette.on_accent } else { palette.text };
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
                .when(active, |el| el.bg(palette.accent))
                .when(disabled, |el| el.opacity(0.4))
                .when(!disabled, |el| {
                    el.on_click(cx.listener(move |this, _, window, cx| this.activate(index, window, cx))).on_hover(cx.listener(move |this, hovered, _window, cx| {
                        if *hovered {
                            this.highlighted = Some(index);
                        } else if this.highlighted == Some(index) {
                            this.highlighted = None;
                        }
                        cx.notify();
                    }))
                })
                .child(div().w(px(14.)).flex().items_center().justify_center().flex_shrink_0().when_some(icon, |el, icon| el.child(Icon::new(icon).size(px(14.)).color(fg))))
                .child(div().flex_grow(1.).min_w(px(0.)).text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).text_color(fg).child(label))
                .when_some(shortcut, |el, shortcut| {
                    el.child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).text_color(if active { palette.on_accent_soft } else { palette.tertiary }).pl(spacing::X3).child(shortcut))
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
    palette: Palette,
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
                .tooltip(move |window, cx| Tooltip::new(format!("{}  {}", tapback_label(kind), shortcut(&(index + 1).to_string(), false, false))).build(window, cx))
                .tooltip_show_delay(crate::primitives::TOOLTIP_DELAY)
                .child(div().text_size(px(16.)).line_height(px(20.)).text_color(palette.text).child(messages_core::tapback_glyph(kind, None).to_owned()))
        }))
}
