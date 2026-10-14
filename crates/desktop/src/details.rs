//! The details inspector: who is in the conversation, where Find My puts them, the photos and
//! files sent in it, and the conversation's switches and actions. Opening it drives the store's
//! Find My stream; it scrolls on its own, never the thread's.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use messages_core::conversations::{
    conversation_guid, conversation_handles, conversation_has_older, conversation_loading, conversation_messages,
};
use messages_core::findmy::{FriendLocation, MapSnapshotRequest, contact_addresses, match_friend};
use messages_core::format::{format_address, format_bytes, relative_time};
use messages_core::store::FindMyState;
use messages_core::{Attachment, Chat, Handle, Message, chat_title, handle_name};
use nd::prelude::*;

use crate::actions::*;
use crate::bridge::{self, Bridge, Topic};
use crate::header::service_label;
use crate::ui;

const GALLERY_THUMB: f32 = 76.;
const GALLERY_SLOTS: usize = 3;
const MAP_SIZE: (u32, u32) = (248, 150);
/// Metres across the map's width.
const MAP_SPAN: u32 = 1500;

#[derive(Default)]
struct Thumb {
    fetching: bool,
    failed: bool,
}

pub struct Details {
    chat: Option<String>,
    open: bool,
    thumbs: HashMap<String, Thumb>,
    slots: Arc<tokio::sync::Semaphore>,
    add: SharedString,
    rename: SharedString,
    /// Map snapshots by address: the request they answer and the file, when the agent made one.
    maps: HashMap<String, (MapSnapshotRequest, Option<PathBuf>)>,
}

fn people(chat: &Chat, handles: &[Handle]) -> Vec<(Handle, Vec<String>)> {
    if chat.is_group {
        return chat.participants.iter().map(|h| (h.clone(), vec![h.address.clone()])).collect();
    }
    handles
        .first()
        .map(|first| vec![(first.clone(), handles.iter().map(|h| h.address.clone()).collect())])
        .unwrap_or_default()
}

/// Pictures first, newest first, then everything else that is not a sticker.
fn gallery_items(messages: &[Arc<Message>]) -> (Vec<(Arc<Message>, Attachment)>, Vec<(Arc<Message>, Attachment)>) {
    let mut images = Vec::new();
    let mut files = Vec::new();
    for message in messages.iter().rev() {
        for attachment in &message.attachments {
            if attachment.hidden || attachment.is_sticker {
                continue;
            }
            if attachment.mime.starts_with("image/") {
                images.push((message.clone(), attachment.clone()));
            } else {
                files.push((message.clone(), attachment.clone()));
            }
        }
    }
    (images, files)
}

fn reference(message: &Message, attachment: &Attachment) -> AttachmentRef {
    AttachmentRef {
        at: MessageRef { chat: message.chat_guid.clone(), message: message.guid.clone() },
        attachment: attachment.guid.clone(),
    }
}

impl Details {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let weak = cx.weak_entity();
        for topic in
            [Topic::Selection, Topic::Locations, Topic::Contacts, Topic::Exporting, Topic::Connection, Topic::ChatList]
        {
            Bridge::watch(cx, topic, &weak);
        }
        Self {
            chat: None,
            open: false,
            thumbs: HashMap::new(),
            slots: Arc::new(tokio::sync::Semaphore::new(GALLERY_SLOTS)),
            add: SharedString::default(),
            rename: SharedString::default(),
            maps: HashMap::new(),
        }
    }

    /// The panel is the only Find My consumer, so while it shows it drives the stream and poll.
    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.open = open;
        if let Some(store) = bridge::store(cx) {
            store.set_details_open(open);
        }
        cx.notify();
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        let Some(store) = bridge::store(cx) else { return };
        let (primary, name) = {
            let state = store.state();
            let primary = state.selected_chat.as_deref().map(|g| conversation_guid(&state.grouping, g).to_owned());
            let name =
                primary.as_deref().and_then(|g| state.chat(g)).and_then(|c| c.display_name.clone()).unwrap_or_default();
            (primary, name)
        };
        if primary == self.chat {
            return;
        }
        self.chat = primary.clone();
        self.thumbs.clear();
        self.maps.clear();
        self.add = SharedString::default();
        self.rename = name.into();
        if let Some(guid) = primary {
            {
                let weak = cx.weak_entity();
                Bridge::watch(cx, Topic::Chat(guid.clone()), &weak);
            }
            {
                let weak = cx.weak_entity();
                Bridge::watch(cx, Topic::Thread(guid), &weak);
            }
        }
    }

    /// Downloads gallery photos three at a time; anything over 25 MB waits for a click.
    fn want_thumb(
        &mut self,
        store: &messages_core::MessagesStore,
        message: &Message,
        attachment: &Attachment,
        cx: &mut Context<Self>,
    ) {
        if attachment.local_path.is_some() {
            return;
        }
        let thumb = self.thumbs.entry(attachment.guid.clone()).or_default();
        if thumb.fetching || thumb.failed || attachment.bytes > crate::message::AUTO_DOWNLOAD_BYTES {
            return;
        }
        thumb.fetching = true;
        let (store, slots) = (store.clone(), self.slots.clone());
        let (chat, guid, attachment_guid, name, mime) = (
            message.chat_guid.clone(),
            message.guid.clone(),
            attachment.guid.clone(),
            attachment.name.clone(),
            attachment.mime.clone(),
        );
        let key = attachment_guid.clone();
        crate::rt::spawn_for(
            cx.weak_entity(),
            async move {
                let _slot = slots.acquire_owned().await;
                store.attachment_src(&chat, &guid, &attachment_guid, &name, Some(&mime)).await.is_ok()
            },
            move |this, ok, cx| {
                let thumb = this.thumbs.entry(key).or_default();
                thumb.fetching = false;
                thumb.failed = !ok;
                cx.notify();
            },
        );
    }

    /// Asks the Mac agent for a map snapshot of `location`, again only when the rounded
    /// request changes (a move, the theme flipping).
    fn want_map(&mut self, address: &str, location: &FriendLocation, dark: bool, cx: &mut Context<Self>) {
        let request = MapSnapshotRequest {
            latitude: location.latitude,
            longitude: location.longitude,
            width: MAP_SIZE.0,
            height: MAP_SIZE.1,
            scale: 2,
            dark,
            span: MAP_SPAN,
        };
        if self.maps.get(address).is_some_and(|(asked, _)| asked.file_name() == request.file_name()) {
            return;
        }
        self.maps.insert(address.to_owned(), (request.clone(), None));
        let Some(agent) = bridge::store(cx).and_then(|s| s.agent().cloned()) else { return };
        let key = address.to_owned();
        crate::rt::spawn_for(
            cx.weak_entity(),
            async move { agent.find_my_snapshot(&request, &messages_core::config::cache_dir().join("maps")).await.ok() },
            move |this, path, cx| {
                if let Some((_, slot)) = this.maps.get_mut(&key) {
                    *slot = path;
                    cx.notify();
                }
            },
        );
    }

    fn location_card(&self, handle: &Handle, location: &FriendLocation, theme: &Theme) -> AnyElement {
        let name = location.name.clone().or_else(|| handle.name.clone()).unwrap_or_else(|| handle.address.clone());
        let place =
            location.label.clone().unwrap_or_else(|| format!("{:.5}, {:.5}", location.latitude, location.longitude));
        let mut detail = relative_time(location.timestamp, ui::now_ms());
        if let Some(accuracy) = location.accuracy.filter(|a| *a > 0.) {
            detail.push_str(&format!(" \u{b7} within {} m", accuracy.round() as i64));
        }
        let map = self.maps.get(&handle.address).and_then(|(_, path)| path.clone());
        let url = messages_core::findmy::maps_url(location.latitude, location.longitude, Some(&name));
        v_flex()
            .id(format!("location-{}", handle.address))
            .gap_1()
            .px_2()
            .child(label("Location").text_style(TextStyle::Caption1).secondary())
            .child(match map.and_then(|p| ui::image_source(&p.to_string_lossy())) {
                Some(source) => image(source)
                    .alt(format!("Map around {name}"))
                    .w(px(MAP_SIZE.0 as f32))
                    .h(px(MAP_SIZE.1 as f32))
                    .rounded_lg()
                    .into_any_element(),
                None => div()
                    .w(px(MAP_SIZE.0 as f32))
                    .h(px(MAP_SIZE.1 as f32))
                    .rounded_lg()
                    .bg(theme.surface.raised)
                    .into_any_element(),
            })
            .child(text(place).font_weight(FontWeight::SEMIBOLD))
            .child(text(detail).text_size(theme.type_scale.caption).text_color(theme.text.secondary))
            .child(link(format!("maps-{}", handle.address), "Open in Maps").url(url))
            .into_any_element()
    }
}

impl Render for Details {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync(cx);
        let Some(store) = bridge::store(cx) else { return div().into_any_element() };
        let Some(guid) = self.chat.clone() else { return div().into_any_element() };
        let (chat, handles, messages, has_older, loading, contacts, friends, find_my, capabilities, exporting) = {
            let state = store.state();
            let Some(chat) = state.chat(&guid).cloned() else { return div().into_any_element() };
            (
                chat,
                conversation_handles(&state, &guid),
                conversation_messages(&state, &guid),
                conversation_has_older(&state, &guid),
                conversation_loading(&state, &guid),
                state.contacts.clone(),
                state.locations.values().cloned().collect::<Vec<FriendLocation>>(),
                state.find_my,
                state.capabilities,
                state.exporting_chat.as_deref() == Some(guid.as_str()),
            )
        };
        let theme = cx.theme().clone();
        let manage = chat.is_group && capabilities.group_management;
        let title = chat_title(&chat);
        let subtitle = if chat.is_group {
            format!("{} people \u{b7} {}", chat.participants.len(), service_label(chat.service))
        } else {
            service_label(chat.service).to_owned()
        };

        let mut people_rows: Vec<AnyElement> = Vec::new();
        let mut wanted_maps = Vec::new();
        for (handle, addresses) in people(&chat, &handles) {
            let mut wanted = addresses.clone();
            wanted.extend(contact_addresses(&contacts, &handle.address));
            let location = match_friend(&friends, &wanted).cloned();
            let removable = manage && chat.participants.len() > 2;
            let (copy, remove) = (
                CopyAddress(handle.address.clone()),
                RemoveParticipant { chat: chat.guid.clone(), address: handle.address.clone() },
            );
            let menu_name = handle_name(&handle).to_owned();
            people_rows.push(
                h_flex()
                    .id(format!("participant-{}", handle.address))
                    .gap_2()
                    .px_2()
                    .py_1()
                    .context_menu({
                        let (copy, remove) = (copy.clone(), remove.clone());
                        move |menu, _, _| {
                            let menu = menu.item("Copy Address", copy.clone());
                            if removable {
                                menu.separator().item_destructive(format!("Remove {menu_name}"), remove.clone())
                            } else {
                                menu
                            }
                        }
                    })
                    .child(ui::avatar_of(&handle, 28.))
                    .child(v_flex().flex_1().min_w_0().child(text(handle_name(&handle).to_owned())).when(
                        handle.name.is_some(),
                        |col| {
                            col.children(addresses.iter().map(|a| {
                                text(format_address(a))
                                    .text_size(theme.type_scale.caption)
                                    .text_color(theme.text.secondary)
                            }))
                        },
                    ))
                    .when(removable, |row| {
                        row.child(
                            icon_button(format!("remove-{}", handle.address), Icon::Minus)
                                .tooltip("Remove")
                                .on_click(move |_, window, cx| crate::ui::dispatch(window, cx, remove.clone())),
                        )
                    })
                    .into_any_element(),
            );
            if let Some(friend) = location {
                wanted_maps.push((handle.address.clone(), friend.clone()));
                people_rows.push(self.location_card(&handle, &friend, &theme));
            }
        }
        let live: HashSet<String> = wanted_maps.iter().map(|(a, _)| a.clone()).collect();
        self.maps.retain(|address, _| live.contains(address));
        for (address, friend) in wanted_maps {
            self.want_map(&address, &friend, theme.is_dark(), cx);
        }

        let (images, files) = gallery_items(&messages);
        for (message, attachment) in &images {
            self.want_thumb(&store, message, attachment, cx);
        }
        let thumbs = images.iter().map(|(message, attachment)| {
            let show = ShowAttachment(reference(message, attachment));
            div()
                .id(format!("gallery-photo-{}", attachment.guid))
                .size(px(GALLERY_THUMB))
                .rounded_md()
                .bg(theme.surface.raised)
                .on_tap(move |_, window, cx| crate::ui::dispatch(window, cx, show.clone()))
                .when_some(
                    attachment.local_path.as_ref().and_then(|p| ui::image_source(&p.to_string_lossy())),
                    |tile, source| {
                        tile.child(image(source).alt(attachment.name.clone()).size(px(GALLERY_THUMB)).rounded_md())
                    },
                )
        });
        let file_rows = files.iter().map(|(message, attachment)| {
            let open = OpenAttachment(reference(message, attachment));
            let name = if message.is_audio { "Audio message".to_owned() } else { attachment.name.clone() };
            v_flex()
                .id(format!("gallery-file-{}", attachment.guid))
                .px_2()
                .py_1()
                .on_tap(move |_, window, cx| crate::ui::dispatch(window, cx, open.clone()))
                .child(text(name))
                .child(
                    text(format_bytes(attachment.bytes))
                        .text_size(theme.type_scale.caption)
                        .text_color(theme.text.secondary),
                )
        });
        let gallery = (!images.is_empty() || !files.is_empty() || has_older).then(|| {
            let older = guid.clone();
            v_flex()
                .id("gallery")
                .gap_2()
                .px_3()
                .child(label("Photos and Files").text_style(TextStyle::Caption1).secondary())
                .child(div().flex().flex_row().flex_wrap().gap_1().children(thumbs))
                .children(file_rows)
                .when(has_older, |col| {
                    col.child(
                        button("gallery-load-older", if loading { "Loading\u{2026}" } else { "Load Older" })
                            .disabled(loading)
                            .on_click(move |_, _, cx| {
                                if let Some(store) = bridge::store(cx) {
                                    let older = older.clone();
                                    crate::rt::detach(async move { store.load_earlier(&older).await });
                                }
                            }),
                    )
                })
        });

        let receipts_off = chat.read_receipts == Some(false);
        let (pin, mute, receipts) =
            (TogglePin(chat.guid.clone()), ToggleMute(chat.guid.clone()), ToggleReceipts(chat.guid.clone()));
        let unread_action: Box<dyn Action> = if chat.unread {
            Box::new(MarkChatRead(chat.guid.clone()))
        } else {
            Box::new(MarkChatUnread(chat.guid.clone()))
        };
        let unread_action = std::rc::Rc::new(unread_action);
        let (export, delete) = (ExportChat(chat.guid.clone()), DeleteChat(chat.guid.clone()));
        let leave_guid = chat.guid.clone();
        let leave_title = title.clone();

        let manage_fields = manage.then(|| {
            let (add_guid, rename_guid) = (chat.guid.clone(), chat.guid.clone());
            let name_unchanged = self.rename.trim() == chat.display_name.as_deref().unwrap_or("");
            v_flex()
                .gap_2()
                .px_3()
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            text_field("details-add")
                                .text(self.add.clone())
                                .placeholder("Phone number or email")
                                .label("Add a person")
                                .flex_1()
                                .on_change(cx.listener(|this, e: &TextChanged, _, cx| {
                                    this.add = e.text.clone();
                                    cx.notify();
                                })),
                        )
                        .child(button("details-add-button", "Add").disabled(self.add.trim().is_empty()).on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| {
                                let address = this.add.trim().to_owned();
                                this.add = SharedString::default();
                                if let Some(store) = bridge::store(cx) {
                                    let chat = add_guid.clone();
                                    crate::rt::detach(async move { store.add_participant(&chat, &address).await });
                                }
                                cx.notify();
                            }),
                        )),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            text_field("details-rename")
                                .text(self.rename.clone())
                                .placeholder("Group name")
                                .label("Group name")
                                .flex_1()
                                .on_change(cx.listener(|this, e: &TextChanged, _, cx| {
                                    this.rename = e.text.clone();
                                    cx.notify();
                                })),
                        )
                        .child(button("details-rename-button", "Rename").disabled(name_unchanged).on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| {
                                let name = this.rename.trim().to_owned();
                                if let Some(store) = bridge::store(cx) {
                                    let chat = rename_guid.clone();
                                    crate::rt::detach(async move { store.rename_group(&chat, &name).await });
                                }
                            }),
                        )),
                )
        });

        v_flex()
            .id("details-panel")
            .size_full()
            .overflow_y_scroll()
            .gap_3()
            .pb_4()
            .child(
                v_flex()
                    .items_center()
                    .gap_1()
                    .pt_3()
                    .child(ui::chat_avatar(&chat, 72.))
                    .child(text(title).font_weight(FontWeight::BOLD).text_size(theme.type_scale.headline))
                    .child(text(subtitle).text_size(theme.type_scale.caption).text_color(theme.text.secondary)),
            )
            .child(
                v_flex()
                    .px_3()
                    .gap_1()
                    .child(
                        switch_row("details-pin", "Pinned")
                            .checked(chat.pinned)
                            .on_toggle(move |_, window, cx| crate::ui::dispatch(window, cx, pin.clone())),
                    )
                    .child(
                        switch_row("details-mute", "Hide Alerts")
                            .checked(chat.muted)
                            .on_toggle(move |_, window, cx| crate::ui::dispatch(window, cx, mute.clone())),
                    )
                    .child(
                        switch_row("details-receipts", "Send Read Receipts")
                            .checked(!receipts_off)
                            .on_toggle(move |_, window, cx| crate::ui::dispatch(window, cx, receipts.clone())),
                    )
                    .child(
                        button("details-unread", if chat.unread { "Mark as Read" } else { "Mark as Unread" }).on_click(
                            move |_, window, cx| crate::ui::dispatch_boxed(window, cx, unread_action.boxed_clone()),
                        ),
                    ),
            )
            .child(separator())
            .child(
                label(if chat.is_group { "People" } else { "Contact" })
                    .text_style(TextStyle::Caption1)
                    .secondary()
                    .ml_3(),
            )
            .child(v_flex().gap_1().px_1().children(people_rows))
            .when(find_my == FindMyState::Unavailable, |col| {
                col.child(label("Find My needs the Mac agent (see README).").secondary().wraps().mx_3())
            })
            .children(manage_fields)
            .children(gallery.map(|g| v_flex().child(separator()).child(g)))
            .child(separator())
            .child(
                v_flex()
                    .px_3()
                    .gap_2()
                    .child(
                        button(
                            "details-export",
                            if exporting { "Exporting\u{2026}" } else { "Export Conversation\u{2026}" },
                        )
                        .disabled(exporting)
                        .on_click(move |_, window, cx| crate::ui::dispatch(window, cx, export.clone())),
                    )
                    .when(manage, |col| {
                        col.child(button("details-leave", "Leave Conversation").destructive().on_click(
                            move |_, window, cx| {
                                let guid = leave_guid.clone();
                                window.confirm(
                                    cx,
                                    format!("Leave \u{201c}{leave_title}\u{201d}?"),
                                    None,
                                    "Leave",
                                    move |confirmed, cx| {
                                        if confirmed && let Some(store) = bridge::store(cx) {
                                            crate::rt::detach(async move { store.leave_group(&guid).await });
                                        }
                                    },
                                );
                            },
                        ))
                    })
                    .child(
                        button("details-delete", "Delete Conversation")
                            .destructive()
                            .on_click(move |_, window, cx| crate::ui::dispatch(window, cx, delete.clone())),
                    ),
            )
            .into_any_element()
    }
}
