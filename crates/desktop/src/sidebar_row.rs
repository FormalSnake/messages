//! The sidebar row: a plain chat row and a pinned-strip cell, plus the row
//! context menu. One entity renders either shape, chosen by `pinned_cell`,
//! since a chat is never shown as both at once.
//!
//! Watches only `Chat(guid)`, `Typing(guid)` and `Focus` (docs/rust-architecture.md's
//! per-row invalidation contract): a message elsewhere, a tapback, or someone
//! else's typing indicator never touches this row. `Sidebar` owns selection,
//! keyboard cursor and section placement, and pushes those in through
//! `set_state` on the handful of rows an event actually changed.

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::conversations::{conversation_focus, conversation_typing, conversation_unread};
use messages_core::{Chat, FocusStatus, GroupEvent, Message, chat_title, handle_name};

use crate::app::AppRoot;
use crate::bridge::{Bridge, StoreHandle, Topic};
use crate::confirm::ConfirmRequest;
use crate::icons::IconName;
use crate::menus::{MenuItem, MenuRequest};
use crate::primitives::{avatar, ring};
use crate::theme::{AVATAR_ROW, ROW_HEIGHT, Theme, radius, spacing, type_scale};

/// The dot column and the row's right inset, so every row lines up on two edges.
pub(crate) const DOT_COLUMN: Pixels = px(16.);
pub(crate) const ROW_INSET: Pixels = px(10.);
pub(crate) const PINNED_CELL: Pixels = px(92.);
/// The unread badge on a pin, centred on the picture's edge the way Messages puts it.
pub(crate) const BADGE: Pixels = px(16.);

fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

/// Platform-formatted shortcut hint: "⇧⌘U" on macOS, "Ctrl+Shift+U" elsewhere.
pub fn shortcut(key: &str, shift: bool) -> SharedString {
    if cfg!(target_os = "macos") {
        format!("{}⌘{key}", if shift { "⇧" } else { "" }).into()
    } else if shift {
        format!("Ctrl+Shift+{key}").into()
    } else {
        format!("Ctrl+{key}").into()
    }
}

pub fn preview_text(message: Option<&Message>, chat: &Chat) -> String {
    let Some(message) = message else {
        return if chat.is_group { "New group".to_owned() } else { "New conversation".to_owned() };
    };
    let who = if message.from_me {
        "You".to_owned()
    } else if let Some(sender) = &message.sender {
        first_name(handle_name(sender)).to_owned()
    } else {
        String::new()
    };
    if message.date_retracted.is_some() {
        return format!("{who} unsent a message");
    }
    if let Some(event) = &message.group_event {
        return match event {
            GroupEvent::Rename { title } => format!("{who} named the conversation \u{201c}{title}\u{201d}"),
            GroupEvent::Join { who: joined } => format!("{who} added {}", joined.as_ref().map(|h| handle_name(h)).unwrap_or("someone")),
            GroupEvent::Leave { who: left } => format!("{} left the conversation", left.as_ref().map(|h| handle_name(h)).unwrap_or(who.as_str())),
            GroupEvent::Photo => format!("{who} changed the group photo"),
        };
    }
    let mut body = message.text.clone();
    if body.is_empty() {
        if let Some(first) = message.attachments.first() {
            body = if message.is_audio {
                "Audio message".to_owned()
            } else if first.mime.starts_with("image/") {
                "Photo".to_owned()
            } else if first.mime.starts_with("video/") {
                "Video".to_owned()
            } else {
                first.name.clone()
            };
        }
    }
    if chat.is_group && !message.from_me && !who.is_empty() {
        return format!("{who}: {body}");
    }
    body
}

/// `open` adds "Open conversation", used by the pinned strip, where the row
/// itself is not the list item.
pub fn chat_menu(chat: &Chat, store: &messages_core::MessagesStore, app: WeakEntity<AppRoot>, open: bool) -> Vec<MenuItem> {
    let guid = chat.guid.clone();
    let mut items = Vec::new();
    if open {
        let store = store.clone();
        let guid = guid.clone();
        items.push(MenuItem::item("Open conversation", move |_, _| {
            let guid = guid.clone();
            let for_task = store.clone();
            store.spawn(async move { for_task.select_chat(Some(&guid)).await });
        }).icon(IconName::Conversation));
    }
    {
        let store = store.clone();
        let guid = guid.clone();
        let pinned = chat.pinned;
        items.push(MenuItem::item(if pinned { "Unpin" } else { "Pin" }, move |_, _| store.toggle_pin(&guid)).icon(if pinned { IconName::PinOff } else { IconName::Pin }));
    }
    {
        let store = store.clone();
        let guid = guid.clone();
        let muted = chat.muted;
        items.push(MenuItem::item(if muted { "Show alerts" } else { "Hide alerts" }, move |_, _| store.toggle_mute(&guid)).icon(if muted { IconName::Unmute } else { IconName::Mute }));
    }
    {
        let store = store.clone();
        let guid = guid.clone();
        let no_receipts = chat.read_receipts == Some(false);
        items.push(
            MenuItem::item(if no_receipts { "Send read receipts" } else { "Read without receipts" }, move |_, _| store.toggle_read_receipts(&guid))
                .icon(if no_receipts { IconName::Eye } else { IconName::EyeOff }),
        );
    }
    {
        let store = store.clone();
        let guid = guid.clone();
        let unread = chat.unread;
        if unread {
            items.push(MenuItem::item("Mark as read", move |_, _| {
                let guid = guid.clone();
                let for_task = store.clone();
                store.spawn(async move { for_task.mark_read(&guid).await });
            }).icon(IconName::MarkRead));
        } else {
            items.push(
                MenuItem::item("Mark as unread", move |_, _| {
                    let guid = guid.clone();
                    let for_task = store.clone();
                    store.spawn(async move { for_task.mark_unread(&guid).await });
                })
                .icon(IconName::MarkUnread)
                .shortcut(shortcut("U", true)),
            );
        }
    }
    {
        let store = store.clone();
        let guid = guid.clone();
        let app = app.clone();
        items.push(
            MenuItem::item("Show details", move |_, cx| {
                let guid = guid.clone();
                let for_task = store.clone();
                store.spawn(async move { for_task.select_chat(Some(&guid)).await });
                if let Some(app) = app.upgrade() {
                    AppRoot::open_info(&app, cx);
                }
            })
            .icon(IconName::Info)
            .shortcut(shortcut("I", false)),
        );
    }
    {
        let store = store.clone();
        let guid = guid.clone();
        items.push(MenuItem::item("Export conversation…", move |_, _| {
            let guid = guid.clone();
            let for_task = store.clone();
            store.spawn(async move { for_task.export_conversation(&guid).await });
        }).icon(IconName::Download));
    }
    items.push(MenuItem::Separator);
    {
        let chat = chat.clone();
        let store = store.clone();
        let app = app.clone();
        items.push(MenuItem::item("Delete conversation", move |_, cx| confirm_delete(&chat, &store, app.clone(), cx)).icon(IconName::Trash).danger());
    }
    items
}

pub fn confirm_delete(chat: &Chat, store: &messages_core::MessagesStore, app: WeakEntity<AppRoot>, cx: &mut App) {
    let Some(app_entity) = app.upgrade() else { return };
    let store = store.clone();
    let guid = chat.guid.clone();
    let request = ConfirmRequest::new(format!("Delete \u{201c}{}\u{201d}?", chat_title(chat)), "Delete", move |_, _| {
        let guid = guid.clone();
        let for_task = store.clone();
        store.spawn(async move { for_task.delete_chat(&guid).await });
    })
    .body("The conversation is removed on your Mac and on every device that syncs with it.")
    .danger();
    AppRoot::open_confirm(&app_entity, request, cx);
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub struct SidebarRow {
    guid: String,
    app: WeakEntity<AppRoot>,
    focus_handle: FocusHandle,
    pinned_cell: bool,
    selected: bool,
    cursored: bool,
    on_select: Rc<dyn Fn(&str, &mut Window, &mut App)>,
    on_arrow: Rc<dyn Fn(&str, i32, &mut Window, &mut App)>,
}

impl SidebarRow {
    pub fn new(
        guid: String,
        app: WeakEntity<AppRoot>,
        on_select: Rc<dyn Fn(&str, &mut Window, &mut App)>,
        on_arrow: Rc<dyn Fn(&str, i32, &mut Window, &mut App)>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak = cx.entity().downgrade();
        Bridge::watch(cx, Topic::Chat(guid.clone()), weak.clone().into());
        Bridge::watch(cx, Topic::Typing(guid.clone()), weak.clone().into());
        Bridge::watch(cx, Topic::Focus, weak.into());
        Self { guid, app, focus_handle: cx.focus_handle(), pinned_cell: false, selected: false, cursored: false, on_select, on_arrow }
    }

    pub fn guid(&self) -> &str {
        &self.guid
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    /// Called by `Sidebar` for every row a render pass places, and only
    /// notifies when something this row actually paints changed.
    pub fn set_state(&mut self, pinned_cell: bool, selected: bool, cursored: bool, cx: &mut Context<Self>) {
        if self.pinned_cell != pinned_cell || self.selected != selected || self.cursored != cursored {
            self.pinned_cell = pinned_cell;
            self.selected = selected;
            self.cursored = cursored;
            cx.notify();
        }
    }

    fn open_menu(&self, chat: &Chat, position: Point<Pixels>, window: &mut Window, cx: &mut App) {
        let Some(store) = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone()) else { return };
        let Some(app) = self.app.upgrade() else { return };
        let items = chat_menu(chat, &store, self.app.clone(), !self.selected);
        AppRoot::open_menu(&app, MenuRequest::at(position, items), window, cx);
    }
}

impl Render for SidebarRow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        let Some(store) = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone()) else {
            return div().into_any_element();
        };
        let state = store.state();
        let Some(chat) = state.chat(&self.guid).cloned() else {
            return div().into_any_element();
        };
        let title = chat_title(&chat);
        let unread = conversation_unread(&state, &self.guid);
        let typing = conversation_typing(&state, &self.guid);
        let silenced = conversation_focus(&state, &self.guid) == FocusStatus::Silenced;
        drop(state);
        let preview = if typing { "Typing…".to_owned() } else { preview_text(chat.last_message.as_ref(), &chat) };
        let guid = self.guid.clone();
        let on_select = self.on_select.clone();
        let on_arrow = self.on_arrow.clone();
        let chat_for_menu = chat.clone();

        if self.pinned_cell {
            let selected = self.selected;
            let badge_on = unread;
            let label = if chat.is_group { title.clone() } else { first_name(&title).to_owned() };
            let (_, ring_color) = ring(self.cursored, palette.focus_ring, palette.transparent);
            div()
                .id(ElementId::Name(format!("pinned-{}", chat.identifier).into()))
                .track_focus(&self.focus_handle)
                .w(PINNED_CELL)
                .flex()
                .flex_col()
                .items_center()
                .gap(spacing::X1)
                .py(spacing::X2)
                .rounded(radius::ROW)
                .cursor_pointer()
                .border_2()
                .border_color(ring_color)
                .when(!selected, |el| el.hover(move |style| style.bg(palette.raised)).active(move |style| style.bg(palette.raised_hover)))
                .on_click({
                    let guid = guid.clone();
                    let on_select = on_select.clone();
                    move |_, window, cx| on_select(&guid, window, cx)
                })
                .on_mouse_up(MouseButton::Right, {
                    let chat = chat_for_menu.clone();
                    cx.listener(move |this, event: &MouseUpEvent, window, cx| this.open_menu(&chat, event.position, window, cx))
                })
                .on_key_down({
                    let guid = guid.clone();
                    let on_select = on_select.clone();
                    let on_arrow = on_arrow.clone();
                    move |event, window, cx| match event.keystroke.key.as_str() {
                        "enter" | "space" => on_select(&guid, window, cx),
                        "right" | "down" => on_arrow(&guid, 1, window, cx),
                        "left" | "up" => on_arrow(&guid, -1, window, cx),
                        _ => {}
                    }
                })
                .child(
                    div().relative().w(px(60.)).h(px(60.)).flex().items_center().justify_center().flex_shrink_0().child(
                        div()
                            .w(px(60.))
                            .h(px(60.))
                            .rounded(px(30.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .border_2()
                            .border_color(if selected { palette.accent } else { palette.transparent })
                            .child(avatar(None, Some(&chat), px(52.), cx)),
                    )
                    .when(badge_on, |el| {
                        el.child(
                            div()
                                .absolute()
                                .top(px(3.))
                                .right(px(3.))
                                .w(BADGE)
                                .h(BADGE)
                                .rounded(BADGE / 2.)
                                .bg(palette.unread)
                                .border_2()
                                .border_color(palette.sidebar),
                        )
                    }),
                )
                .child(
                    div()
                        .text_size(type_scale::MICRO.font_size)
                        .line_height(type_scale::MICRO.line_height)
                        .text_color(if selected { palette.text } else { palette.secondary })
                        .text_ellipsis()
                        .max_w(PINNED_CELL - spacing::X2)
                        .child(label),
                )
                .into_any_element()
        } else {
            let selected = self.selected;
            let fg = if selected { palette.on_accent } else { palette.text };
            let muted_color = if selected { palette.on_accent_soft } else { palette.secondary };
            let (_, ring_c) = ring(self.cursored, if selected { palette.text } else { palette.focus_ring }, palette.transparent);
            div()
                .id(ElementId::Name(format!("chat-{}", chat.identifier).into()))
                .track_focus(&self.focus_handle)
                .flex()
                .flex_row()
                .items_center()
                .gap(spacing::X2)
                .h(ROW_HEIGHT)
                .pr(ROW_INSET)
                .rounded(radius::ROW)
                .cursor_pointer()
                .border_2()
                .border_color(ring_c)
                .when(selected, |el| el.bg(palette.selected))
                .when(!selected, |el| el.hover(move |style| style.bg(palette.raised)).active(move |style| style.bg(palette.raised_hover)))
                .on_click({
                    let guid = guid.clone();
                    let on_select = on_select.clone();
                    move |_, window, cx| on_select(&guid, window, cx)
                })
                .on_mouse_up(MouseButton::Right, {
                    let chat = chat_for_menu.clone();
                    cx.listener(move |this, event: &MouseUpEvent, window, cx| this.open_menu(&chat, event.position, window, cx))
                })
                .on_key_down({
                    let guid = guid.clone();
                    let on_select = on_select.clone();
                    let on_arrow = on_arrow.clone();
                    move |event, window, cx| match event.keystroke.key.as_str() {
                        "enter" | "space" => on_select(&guid, window, cx),
                        "down" => on_arrow(&guid, 1, window, cx),
                        "up" => on_arrow(&guid, -1, window, cx),
                        _ => {}
                    }
                })
                .child(
                    div().w(DOT_COLUMN).flex().items_center().justify_center().flex_shrink_0().when(unread && !selected, |el| {
                        el.child(div().w(px(8.)).h(px(8.)).rounded(px(4.)).bg(palette.unread))
                    }),
                )
                .child(avatar(None, Some(&chat), AVATAR_ROW, cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_grow(1.)
                        .flex_shrink(1.)
                        .min_w(px(0.))
                        .gap(px(1.))
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
                                        .text_color(fg)
                                        .text_ellipsis()
                                        .child(title),
                                )
                                .when(silenced, |el| el.child(crate::icons::Icon::new(IconName::Silenced).size(px(11.)).color(muted_color)))
                                .when(chat.muted, |el| el.child(crate::icons::Icon::new(IconName::Mute).size(px(11.)).color(muted_color)))
                                .child(
                                    div()
                                        .text_size(type_scale::MICRO.font_size)
                                        .line_height(type_scale::MICRO.line_height)
                                        .text_color(muted_color)
                                        .flex_shrink_0()
                                        .child(if chat.last_activity > 0 { messages_core::format::format_list_date(chat.last_activity, now_ms()) } else { String::new() }),
                                ),
                        )
                        .child({
                            let color = if typing && !selected { palette.accent } else { muted_color };
                            div()
                                .text_size(type_scale::PREVIEW.font_size)
                                .line_height(type_scale::PREVIEW.line_height)
                                .text_color(color)
                                .line_clamp(2)
                                .w_full()
                                .min_w(px(0.))
                                .child(crate::emoji_font::styled_text(preview.into(), Vec::new(), color))
                        }),
                )
                .into_any_element()
        }
    }
}
