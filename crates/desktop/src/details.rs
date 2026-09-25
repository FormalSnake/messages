//! Port of `InfoPanel` in `apps/desktop/src/ui/header.tsx` (the details
//! panel). `app.rs` mounts `InfoPanel` in the sliding info column.

use std::collections::HashMap;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::conversations::{conversation_focus, conversation_guid, conversation_handles};
use messages_core::findmy::{FriendLocation, normalize_address};
use messages_core::{Handle, chat_title, handle_name};

use crate::bridge::{Bridge, StoreHandle, Topic};
use crate::confirm::{ConfirmRequest, confirm_dialog};
use crate::icons::{Icon, IconName};
use crate::location::LocationCard;
use crate::primitives::{IconButton, avatar, divider, section_label};
use crate::theme::{INFO_WIDTH, Theme, spacing, type_scale};

pub struct InfoPanel {
    current_chat: Option<String>,
    locations: HashMap<String, Entity<LocationCard>>,
    confirm: Option<ConfirmRequest>,
    confirm_focus: FocusHandle,
}

impl InfoPanel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade();
        Bridge::watch(cx, Topic::Selection, weak.clone().into());
        Bridge::watch(cx, Topic::Locations, weak.into());
        let confirm_focus = cx.focus_handle();
        Self { current_chat: None, locations: HashMap::new(), confirm: None, confirm_focus }
    }

    fn store(cx: &App) -> Option<messages_core::MessagesStore> {
        cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone())
    }

    /// The details panel is always mounted; it drives the store's Find My
    /// stream and poll only while `open`, per `set_details_open`
    /// (header.tsx:516-519).
    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if let Some(store) = Self::store(cx) {
            store.set_details_open(open);
        }
    }

    fn sync_selection(&mut self, cx: &mut Context<Self>) {
        let Some(store) = Self::store(cx) else { return };
        let state = store.state();
        let primary = state.selected_chat.as_deref().map(|guid| conversation_guid(&state.grouping, guid).to_owned());
        if primary != self.current_chat {
            self.current_chat = primary;
        }
    }

    fn confirm_leave(&mut self, chat_guid: String, title: String, cx: &mut Context<Self>) {
        self.confirm = Some(ConfirmRequest::new(format!("Leave \"{title}\"?"), "Leave", move |_window, cx| {
            if let Some(store) = Self::store(cx) {
                let chat_guid = chat_guid.clone();
                let task_store = store.clone();
                store.spawn(async move { task_store.leave_group(&chat_guid).await });
            }
        }));
        cx.notify();
    }

    fn confirm_delete(&mut self, chat_guid: String, title: String, cx: &mut Context<Self>) {
        self.confirm = Some(ConfirmRequest::new(format!("Delete \"{title}\"?"), "Delete", move |_window, cx| {
            if let Some(store) = Self::store(cx) {
                let chat_guid = chat_guid.clone();
                let task_store = store.clone();
                store.spawn(async move { task_store.delete_chat(&chat_guid).await });
            }
        })
        .danger()
        .body("This removes the conversation everywhere it syncs."));
        cx.notify();
    }
}

fn action_row(palette: &crate::theme::Palette, icon: IconName, label: impl Into<SharedString>, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> impl IntoElement {
    let label = label.into();
    div()
        .id(ElementId::Name(format!("row-{label}").into()))
        .flex()
        .flex_row()
        .items_center()
        .gap(spacing::X2)
        .h(px(32.))
        .px(spacing::X4)
        .hover(|style| style.bg(palette.hover_wash))
        .on_click(on_click)
        .child(Icon::new(icon).size(px(14.)).color(palette.secondary))
        .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).text_color(palette.text).child(label))
}

impl Render for InfoPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_selection(cx);
        let palette = Theme::get(cx);
        let close_confirm = {
            let weak = cx.entity().downgrade();
            move |_window: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| {
                    this.confirm = None;
                    cx.notify();
                });
            }
        };
        let Some(store) = Self::store(cx) else {
            return div().w(INFO_WIDTH).h_full().bg(palette.sidebar).border_l_1().border_color(palette.sidebar_border).into_any_element();
        };
        let state = store.state();
        let Some(chat) = self.current_chat.as_deref().and_then(|guid| state.chat(guid)).cloned() else {
            drop(state);
            return div().w(INFO_WIDTH).h_full().bg(palette.sidebar).border_l_1().border_color(palette.sidebar_border).into_any_element();
        };
        let guid = chat.guid.clone();
        let title = chat_title(&chat);
        let handles: Vec<Handle> = self.current_chat.as_deref().map(|guid| conversation_handles(&state, guid)).unwrap_or_default();
        let locations: HashMap<String, FriendLocation> = state.locations.clone();
        let capabilities = state.capabilities;
        drop(state);

        // Ensure a `LocationCard` exists for every sharing participant and
        // hand each its latest snapshot; unmatched ones are dropped.
        let mut sharing: Vec<(Handle, FriendLocation)> = Vec::new();
        for handle in &handles {
            if let Some(friend) = locations.get(&normalize_address(&handle.address)) {
                sharing.push((handle.clone(), friend.clone()));
            }
        }
        for (handle, friend) in &sharing {
            if let Some(existing) = self.locations.get(&handle.address) {
                existing.update(cx, |card, cx| card.set_location(friend.clone(), cx));
            } else {
                let address = handle.address.clone();
                let friend = friend.clone();
                let card = cx.new(|cx| LocationCard::new(address.clone(), friend, window, cx));
                self.locations.insert(address, card);
            }
        }

        div()
            .id("details-panel")
            .w(INFO_WIDTH)
            .h_full()
            .flex()
            .flex_col()
            .bg(palette.sidebar)
            .border_l_1()
            .border_color(palette.sidebar_border)
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px(spacing::X4)
                    .py(spacing::X3)
                    .child(div().text_size(type_scale::TITLE.font_size).line_height(type_scale::TITLE.line_height).text_color(palette.text).child("Details"))
                    .child(IconButton::new("close-details", IconName::Close, "Close").on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(crate::app::ToggleInfo), cx);
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(spacing::X1)
                    .px(spacing::X4)
                    .pb(spacing::X3)
                    .child(avatar(chat.participants.first(), Some(&chat), px(72.), cx))
                    .child(div().text_size(type_scale::TITLE.font_size).line_height(type_scale::TITLE.line_height).text_color(palette.text).child(title.clone()))
                    .child(
                        div()
                            .text_size(type_scale::CAPTION.font_size)
                            .line_height(type_scale::CAPTION.line_height)
                            .text_color(palette.secondary)
                            .child(if chat.is_group { format!("{} people", chat.participants.len()) } else { "iMessage".to_owned() }),
                    ),
            )
            .child(divider(palette.separator))
            .child(action_row(&palette, if chat.pinned { IconName::PinOff } else { IconName::Pin }, if chat.pinned { "Unpin" } else { "Pin" }, {
                let guid = guid.clone();
                move |_, _, cx| {
                    if let Some(store) = Self::store(cx) {
                        store.toggle_pin(&guid);
                    }
                }
            }))
            .child(action_row(&palette, if chat.muted { IconName::Unmute } else { IconName::Mute }, if chat.muted { "Show alerts" } else { "Hide alerts" }, {
                let guid = guid.clone();
                move |_, _, cx| {
                    if let Some(store) = Self::store(cx) {
                        store.toggle_mute(&guid);
                    }
                }
            }))
            .when(capabilities.read_receipts, |el| {
                let receipts_on = chat.read_receipts.unwrap_or(true);
                el.child(action_row(
                    &palette,
                    if receipts_on { IconName::EyeOff } else { IconName::Eye },
                    if receipts_on { "Read without receipts" } else { "Send read receipts" },
                    {
                        let guid = guid.clone();
                        move |_, _, cx| {
                            if let Some(store) = Self::store(cx) {
                                store.toggle_read_receipts(&guid);
                            }
                        }
                    },
                ))
            })
            .when(capabilities.mark_unread, |el| {
                el.child(action_row(&palette, IconName::MarkUnread, "Mark as unread", {
                    let guid = guid.clone();
                    move |_, _, cx| {
                        if let Some(store) = Self::store(cx) {
                            let guid = guid.clone();
                            let task_store = store.clone();
                            store.spawn(async move { task_store.mark_unread(&guid).await });
                        }
                    }
                }))
            })
            .child(divider(palette.separator))
            .child(section_label(if chat.is_group { "People" } else { "Contact" }, palette.tertiary, spacing::X4))
            .children(handles.iter().map(|handle| {
                let focus = conversation_focus_for(&self.current_chat, cx);
                let _ = focus;
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(spacing::X2)
                    .px(spacing::X4)
                    .py(spacing::X1)
                    .child(avatar(Some(handle), None, px(28.), cx))
                    .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).text_color(palette.text).child(handle_name(handle).to_owned()))
            }))
            .when(!sharing.is_empty(), |el| {
                el.child(divider(palette.separator)).child(section_label("Location", palette.tertiary, spacing::X4)).children(sharing.iter().map(|(handle, _)| {
                    self.locations.get(&handle.address).cloned().into_iter().map(|card| div().px(spacing::X4).pb(spacing::X2).child(card))
                }).flatten())
            })
            .child(divider(palette.separator))
            .child(action_row(&palette, IconName::Download, "Export conversation…", {
                let guid = guid.clone();
                move |_, _, cx| {
                    if let Some(store) = Self::store(cx) {
                        let guid = guid.clone();
                        let task_store = store.clone();
                        store.spawn(async move { task_store.export_conversation(&guid).await });
                    }
                }
            }))
            .when(chat.is_group && capabilities.group_management, |el| {
                el.child({
                    let guid = guid.clone();
                    let title = title.clone();
                    action_row(&palette, IconName::Leave, "Leave conversation", cx.listener(move |this, _, _, cx| this.confirm_leave(guid.clone(), title.clone(), cx)))
                })
            })
            .child({
                let guid = guid.clone();
                let title = title.clone();
                action_row(&palette, IconName::Trash, "Delete conversation", cx.listener(move |this, _, _, cx| this.confirm_delete(guid.clone(), title.clone(), cx)))
            })
            .when_some(self.confirm.as_ref(), |el, request| {
                el.child(confirm_dialog(request, window.viewport_size(), &self.confirm_focus, close_confirm.clone(), cx))
            })
            .into_any_element()
    }
}

fn conversation_focus_for(chat: &Option<String>, cx: &App) -> messages_core::FocusStatus {
    let Some(store) = cx.try_global::<crate::bridge::StoreHandle>().and_then(|handle| handle.0.clone()) else { return messages_core::FocusStatus::Unknown };
    let Some(guid) = chat else { return messages_core::FocusStatus::Unknown };
    conversation_focus(&store.state(), guid)
}
