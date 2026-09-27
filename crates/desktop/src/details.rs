//! The details panel (`InfoPanel`). `app.rs` mounts it in the sliding info
//! column, sized to `INFO_WIDTH` whatever holds it.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::conversations::{conversation_guid, conversation_handles, conversation_has_older, conversation_loading, conversation_messages};
use messages_core::findmy::{FriendLocation, contact_addresses, match_friend};
use messages_core::format::{format_address, format_bytes};
use messages_core::store::FindMyState;
use messages_core::{Attachment, Chat, Handle, Message, Service, chat_title, handle_name};

use crate::app::{AppRoot, root};
use crate::attachments::{AUTO_DOWNLOAD_BYTES, media_worker};
use crate::bridge::{Bridge, Topic, store};
use crate::confirm::ConfirmRequest;
use crate::icons::{Icon, IconName};
use crate::location::LocationCard;
use crate::menus::{MenuItem, MenuRequest, shortcut};
use crate::primitives::{Button, IconButton, avatar, divider, new_input_state, section_label};
use crate::theme::{INFO_WIDTH, Palette, TITLEBAR_HEIGHT, Theme, radius, spacing, type_scale};

const GALLERY_COLUMNS: f32 = 3.;
const GALLERY_GAP: Pixels = spacing::X1;
/// INFO_WIDTH minus the panel's own X4 inset on each side minus the two gaps
/// between columns, split three ways.
const GALLERY_THUMB: f32 = (280. - 16. * 2. - 4. * (GALLERY_COLUMNS - 1.)) / GALLERY_COLUMNS;
/// Twice the box, so the cut still has pixels to spare on a HiDPI screen.
const GALLERY_TILE: u32 = (GALLERY_THUMB * 2.) as u32;
/// Photos fetched at once. The panel wants every picture in the conversation
/// the moment it opens; unbounded, a chat with a hundred of them started a
/// hundred requests.
const GALLERY_SLOTS: usize = 3;

pub fn service_label(service: Service) -> &'static str {
    match service {
        Service::IMessage => "iMessage",
        Service::Sms => "SMS",
        Service::Rcs => "RCS",
    }
}

#[derive(Default)]
struct Thumb {
    fetching: bool,
    failed: bool,
    /// The local file, once downloaded (or already on disk).
    src: Option<PathBuf>,
    /// The square cut actually painted; the source path when no cut could be made.
    tile: Option<PathBuf>,
    cutting: bool,
}

pub struct InfoPanel {
    current_chat: Option<String>,
    locations: HashMap<String, Entity<LocationCard>>,
    thumbs: HashMap<String, Thumb>,
    slots: Arc<tokio::sync::Semaphore>,
    add_field: Entity<InputState>,
    name_field: Entity<InputState>,
    watched: HashSet<String>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl InfoPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade();
        for topic in [Topic::Selection, Topic::Locations, Topic::Contacts, Topic::Exporting, Topic::Connection, Topic::ChatList] {
            Bridge::watch(cx, topic, weak.clone().into());
        }
        let add_field = new_input_state(window, cx, "Phone number or email", false);
        let name_field = new_input_state(window, cx, "Group name", false);
        let subscriptions = vec![
            cx.subscribe(&add_field, |this: &mut Self, _, event: &InputEvent, cx| match event {
                InputEvent::PressEnter { .. } => this.add_person(cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            }),
            cx.subscribe(&name_field, |this: &mut Self, _, event: &InputEvent, cx| match event {
                InputEvent::PressEnter { .. } => this.rename(cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            }),
        ];
        Self {
            current_chat: None,
            locations: HashMap::new(),
            thumbs: HashMap::new(),
            slots: Arc::new(tokio::sync::Semaphore::new(GALLERY_SLOTS)),
            add_field,
            name_field,
            watched: HashSet::new(),
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    /// The details panel is the only Find My consumer, so while it is
    /// mounted it drives the store's stream and poll.
    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if let Some(store) = store(cx) {
            store.set_details_open(open);
        }
    }

    fn sync_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = store(cx) else { return };
        let (primary, display_name) = {
            let state = store.state();
            let primary = state.selected_chat.as_deref().map(|guid| conversation_guid(&state.grouping, guid).to_owned());
            let name = primary.as_deref().and_then(|guid| state.chat(guid)).and_then(|chat| chat.display_name.clone()).unwrap_or_default();
            (primary, name)
        };
        if primary == self.current_chat {
            return;
        }
        self.current_chat = primary.clone();
        self.thumbs.clear();
        self.locations.clear();
        self.add_field.update(cx, |field, cx| field.set_value("", window, cx));
        self.name_field.update(cx, |field, cx| field.set_value(display_name, window, cx));
        if let Some(guid) = primary {
            if self.watched.insert(guid.clone()) {
                let weak = cx.entity().downgrade();
                Bridge::watch(cx, Topic::Chat(guid.clone()), weak.clone().into());
                Bridge::watch(cx, Topic::Thread(guid), weak.into());
            }
        }
    }

    fn add_person(&mut self, cx: &mut Context<Self>) {
        let Some(store) = store(cx) else { return };
        let Some(chat) = self.current_chat.clone() else { return };
        let address = self.add_field.read(cx).value().trim().to_owned();
        if address.is_empty() {
            return;
        }
        let field = self.add_field.clone();
        cx.defer(move |cx| {
            if let Some(window) = cx.active_window() {
                let _ = window.update(cx, |_, window, cx| field.update(cx, |field, cx| field.set_value("", window, cx)));
            }
        });
        let task = store.clone();
        store.spawn(async move { task.add_participant(&chat, &address).await });
    }

    fn rename(&mut self, cx: &mut Context<Self>) {
        let Some(store) = store(cx) else { return };
        let Some(chat) = self.current_chat.clone() else { return };
        let name = self.name_field.read(cx).value().trim().to_owned();
        let task = store.clone();
        store.spawn(async move { task.rename_group(&chat, &name).await });
    }

    /// Downloads a gallery photo (three at a time) and cuts its square tile.
    fn want_thumb(&mut self, message: &Message, attachment: &Attachment, cx: &mut Context<Self>) {
        let Some(store) = store(cx) else { return };
        let thumb = self.thumbs.entry(attachment.guid.clone()).or_default();
        if thumb.src.is_none() {
            thumb.src = attachment.local_path.clone();
        }
        if let Some(src) = thumb.src.clone() {
            if thumb.tile.is_none() && !thumb.cutting {
                thumb.cutting = true;
                let guid = attachment.guid.clone();
                let (tx, rx) = tokio::sync::oneshot::channel();
                let worker = media_worker().clone();
                let source = src.clone();
                store.spawn(async move {
                    let _ = tx.send(worker.tile(&source, 1., GALLERY_TILE).await);
                });
                cx.spawn(async move |this, cx| {
                    let tile = rx.await.ok().flatten();
                    let _ = this.update(cx, |this, cx| {
                        if let Some(thumb) = this.thumbs.get_mut(&guid) {
                            thumb.tile = Some(tile.unwrap_or(src));
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            return;
        }
        // Anything past the cap waits for a click, the same bargain the thread makes.
        if thumb.fetching || thumb.failed || attachment.bytes > AUTO_DOWNLOAD_BYTES {
            return;
        }
        thumb.fetching = true;
        self.fetch(message, attachment, cx);
    }

    fn fetch(&mut self, message: &Message, attachment: &Attachment, cx: &mut Context<Self>) {
        let Some(store) = store(cx) else { return };
        let slots = self.slots.clone();
        let (chat, message_guid, guid, name, mime) = (message.chat_guid.clone(), message.guid.clone(), attachment.guid.clone(), attachment.name.clone(), attachment.mime.clone());
        let (tx, rx) = tokio::sync::oneshot::channel();
        let task = store.clone();
        let attachment_guid = guid.clone();
        store.spawn(async move {
            let _slot = slots.acquire_owned().await;
            let _ = tx.send(task.attachment_src(&chat, &message_guid, &attachment_guid, &name, Some(&mime)).await.ok());
        });
        cx.spawn(async move |this, cx| {
            let src = rx.await.ok().flatten();
            let _ = this.update(cx, |this, cx| {
                let thumb = this.thumbs.entry(guid).or_default();
                thumb.fetching = false;
                match src {
                    Some(src) => thumb.src = Some(src),
                    None => thumb.failed = true,
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_file(message: &Message, attachment: &Attachment, cx: &mut App) {
        if let Some(path) = &attachment.local_path {
            messages_core::open::open_external(&path.to_string_lossy());
            return;
        }
        let Some(store) = store(cx) else { return };
        let (chat, message_guid, guid, name, mime) = (message.chat_guid.clone(), message.guid.clone(), attachment.guid.clone(), attachment.name.clone(), attachment.mime.clone());
        let task = store.clone();
        store.spawn(async move {
            if let Ok(path) = task.attachment_src(&chat, &message_guid, &guid, &name, Some(&mime)).await {
                messages_core::open::open_external(&path.to_string_lossy());
            }
        });
    }
}

fn run(f: impl Fn(&messages_core::MessagesStore) + 'static) -> impl Fn(&mut Window, &mut App) + 'static {
    move |_window, cx| {
        if let Some(store) = store(cx) {
            f(&store);
        }
    }
}

fn spawn_store<F, Fut>(f: F) -> impl Fn(&mut Window, &mut App) + 'static
where
    F: Fn(messages_core::MessagesStore) -> Fut + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    move |_window, cx| {
        if let Some(store) = store(cx) {
            store.spawn(f(store.clone()));
        }
    }
}

/// One action in the panel: Enter or Space runs it too.
fn action_row(palette: &Palette, id: &str, icon: IconName, label: impl Into<SharedString>, value: Option<SharedString>, danger: bool, on_click: impl Fn(&mut Window, &mut App) + 'static) -> impl IntoElement {
    let on_click = std::rc::Rc::new(on_click);
    let on_key = on_click.clone();
    let wash = if danger { palette.danger_soft } else { palette.hover_wash };
    let press = if danger { palette.danger_soft } else { palette.press_wash };
    let ring = palette.focus_ring;
    div()
        .id(SharedString::from(format!("details-{id}")))
        .tab_index(0)
        .flex()
        .flex_row()
        .items_center()
        .gap(spacing::X2)
        .h(px(32.))
        .px(spacing::X2)
        .rounded(radius::CONTROL)
        .flex_shrink_0()
        .border_2()
        .border_color(palette.transparent)
        .focus_visible(move |style| style.border_color(ring))
        .hover(move |style| style.bg(wash))
        .active(move |style| style.bg(press))
        .on_click(move |_, window, cx| on_click(window, cx))
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                on_key(window, cx);
            }
        })
        .child(div().w(px(15.)).flex().items_center().justify_center().flex_shrink_0().child(Icon::new(icon).size(px(15.)).color(if danger { palette.danger } else { palette.secondary })))
        .child(
            div()
                .flex_grow(1.)
                .min_w(px(0.))
                .text_size(type_scale::BODY.font_size)
                .line_height(type_scale::BODY.line_height)
                .text_color(if danger { palette.danger } else { palette.text })
                .text_ellipsis()
                .child(label.into()),
        )
        .when_some(value, |el, value| el.child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).text_color(palette.secondary).flex_shrink_0().child(value)))
}

fn section_divider(palette: &Palette) -> impl IntoElement {
    div().py(spacing::X4).px(spacing::X4).flex_shrink_0().child(divider(palette.separator))
}

/// A group lists its participants; a one-to-one conversation is one person
/// with every address it is reached on.
fn people(chat: &Chat, handles: &[Handle]) -> Vec<(Handle, Vec<String>)> {
    if chat.is_group {
        return chat.participants.iter().map(|handle| (handle.clone(), vec![handle.address.clone()])).collect();
    }
    handles.first().map(|first| vec![(first.clone(), handles.iter().map(|handle| handle.address.clone()).collect())]).unwrap_or_default()
}

fn participant_menu(handle: &Handle, chat: &Chat, manage: bool) -> Vec<MenuItem> {
    let address = handle.address.clone();
    let mut items = vec![MenuItem::item("Copy address", move |_, cx| {
        if let Some(store) = store(cx) {
            let address = address.clone();
            store.spawn(async move { messages_core::clipboard::copy_text(&address).await });
        }
    })
    .icon(IconName::Copy)];
    if manage {
        let (chat_guid, address) = (chat.guid.clone(), handle.address.clone());
        items.push(MenuItem::Separator);
        items.push(
            MenuItem::item(format!("Remove {}", handle_name(handle)), spawn_store(move |store| {
                let (chat_guid, address) = (chat_guid.clone(), address.clone());
                async move { store.remove_participant(&chat_guid, &address).await }
            }))
            .icon(IconName::RemovePerson)
            .danger()
            .disabled(chat.participants.len() <= 2),
        );
    }
    items
}

/// Newest first: every visible, non-sticker attachment across what is
/// loaded, images for the grid and everything else for the file list.
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

impl Render for InfoPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_selection(window, cx);
        let palette = Theme::get(cx);
        let empty = || div().w(INFO_WIDTH).h_full().bg(palette.sidebar).border_l_1().border_color(palette.sidebar_border).into_any_element();
        let Some(live) = store(cx) else { return empty() };
        let state = live.state();
        let Some(chat) = self.current_chat.as_deref().and_then(|guid| state.chat(guid)).cloned() else {
            drop(state);
            return empty();
        };
        let guid = chat.guid.clone();
        let title = chat_title(&chat);
        let handles = conversation_handles(&state, &guid);
        let friends: Vec<FriendLocation> = state.locations.values().cloned().collect();
        let contacts = state.contacts.clone();
        let capabilities = state.capabilities;
        let find_my_unavailable = state.find_my == FindMyState::Unavailable;
        let exporting = state.exporting_chat.as_deref() == Some(guid.as_str());
        let messages = conversation_messages(&state, &guid);
        let has_older = conversation_has_older(&state, &guid);
        let loading = conversation_loading(&state, &guid);
        drop(state);
        let manage = capabilities.group_management && chat.is_group;
        let app = root(cx).map(|app| app.downgrade());

        let mut people_column = div().flex().flex_col().px(spacing::X2).flex_shrink_0();
        let mut shown_cards = HashSet::new();
        for (handle, addresses) in people(&chat, &handles) {
            let mut wanted = addresses.clone();
            wanted.extend(contact_addresses(&contacts, &handle.address));
            let location = match_friend(&friends, &wanted).cloned();
            people_column = people_column.child(participant_row(&palette, &handle, &chat, manage, &addresses, cx));
            if let Some(friend) = location {
                let card = match self.locations.get(&handle.address) {
                    Some(card) => {
                        card.update(cx, |card, cx| card.set_location(friend.clone(), cx));
                        card.clone()
                    }
                    None => {
                        let card = cx.new(|cx| LocationCard::new(handle.clone(), friend, window, cx));
                        self.locations.insert(handle.address.clone(), card.clone());
                        card
                    }
                };
                shown_cards.insert(handle.address.clone());
                people_column = people_column.child(div().pt(spacing::X3).child(section_label("Location", palette.tertiary, spacing::X2))).child(card);
            }
        }
        self.locations.retain(|address, _| shown_cards.contains(address));
        if find_my_unavailable {
            people_column = people_column.child(
                div().pl(spacing::X2).pt(spacing::X1).text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).child("Locations need the Mac agent. See the README to set it up."),
            );
        }

        let (images, files) = gallery_items(&messages);
        for (message, attachment) in &images {
            self.want_thumb(message, attachment, cx);
        }
        let gallery = (!images.is_empty() || !files.is_empty() || has_older).then(|| {
            let thumbs = images.iter().map(|(message, attachment)| {
                let tile = self.thumbs.get(&attachment.guid).and_then(|thumb| thumb.tile.clone());
                let (chat_guid, attachment_guid) = (message.chat_guid.clone(), attachment.guid.clone());
                div()
                    .id(SharedString::from(format!("gallery-photo-{}", attachment.guid)))
                    .debug_selector(|| format!("gallery-photo-{}", attachment.guid))
                    .w(px(GALLERY_THUMB))
                    .h(px(GALLERY_THUMB))
                    .rounded(radius::CONTROL)
                    .overflow_hidden()
                    .bg(palette.raised)
                    .border_1()
                    .border_color(crate::primitives::image_outline(&palette))
                    .hover(|style| style.opacity(0.9))
                    .on_click(move |_, window, cx| crate::lightbox::open(&chat_guid, &attachment_guid, window, cx))
                    .when_some(tile, |el, tile| el.child(img(crate::attachments::sized_image_source(&tile, px(GALLERY_THUMB), px(GALLERY_THUMB), ObjectFit::Contain)).w(px(GALLERY_THUMB)).h(px(GALLERY_THUMB)).object_fit(ObjectFit::Contain)))
            });
            let file_rows = files.iter().map(|(message, attachment)| {
                let label: SharedString = if message.is_audio { "Audio message".into() } else { attachment.name.clone().into() };
                let (for_click, for_key) = ((message.clone(), attachment.clone()), (message.clone(), attachment.clone()));
                div()
                    .id(SharedString::from(format!("gallery-file-{}", attachment.guid)))
                    .tab_index(0)
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(spacing::X2)
                    .h(px(36.))
                    .px(spacing::X2)
                    .rounded(radius::CONTROL)
                    .flex_shrink_0()
                    .border_2()
                    .border_color(palette.transparent)
                    .focus_visible(move |style| style.border_color(palette.focus_ring))
                    .hover(move |style| style.bg(palette.hover_wash))
                    .active(move |style| style.bg(palette.press_wash))
                    .on_click(move |_, _, cx| Self::open_file(&for_click.0, &for_click.1, cx))
                    .on_key_down(move |event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            Self::open_file(&for_key.0, &for_key.1, cx);
                        }
                    })
                    .child(Icon::new(if message.is_audio { IconName::Audio } else { IconName::File }).size(px(15.)).color(palette.secondary))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_grow(1.)
                            .min_w(px(0.))
                            .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).text_color(palette.text).text_ellipsis().child(label))
                            .child(div().text_size(type_scale::MICRO.font_size).line_height(type_scale::MICRO.line_height).text_color(palette.secondary).child(format_bytes(attachment.bytes))),
                    )
            });
            let guid = guid.clone();
            div()
                .flex()
                .flex_col()
                .flex_shrink_0()
                .child(section_divider(&palette))
                .child(section_label("Photos and files", palette.tertiary, spacing::X4))
                .when(!images.is_empty(), |el| el.child(div().flex().flex_row().flex_wrap().gap(GALLERY_GAP).px(spacing::X4).flex_shrink_0().children(thumbs)))
                .when(!files.is_empty(), |el| el.child(div().flex().flex_col().px(spacing::X2).when(!images.is_empty(), |el| el.pt(spacing::X2)).flex_shrink_0().children(file_rows)))
                .when(has_older, |el| {
                    el.child(div().px(spacing::X4).py(spacing::X2).flex_shrink_0().child(Button::new("gallery-load-older", if loading { "Loading…" } else { "Load older" }).disabled(loading).on_click(
                        move |_, _, cx| {
                            if let Some(store) = store(cx) {
                                let (task, guid) = (store.clone(), guid.clone());
                                store.spawn(async move { task.load_earlier(&guid).await });
                            }
                        },
                    )))
                })
        });

        let add_empty = self.add_field.read(cx).value().trim().is_empty();
        let name_unchanged = self.name_field.read(cx).value().trim() == chat.display_name.as_deref().unwrap_or("");
        let manage_fields = manage.then(|| {
            div()
                .flex()
                .flex_col()
                .gap(spacing::X2)
                .pt(spacing::X3)
                .px(spacing::X4)
                .flex_shrink_0()
                .child(
                    div().flex().flex_row().gap(spacing::X2).child(div().flex_grow(1.).min_w(px(0.)).child(Input::new(&self.add_field).bordered(true))).child(
                        Button::new("details-add", "Add").disabled(add_empty).on_click(cx.listener(|this, _, _, cx| this.add_person(cx))),
                    ),
                )
                .child(
                    div().flex().flex_row().gap(spacing::X2).child(div().flex_grow(1.).min_w(px(0.)).child(Input::new(&self.name_field).bordered(true))).child(
                        Button::new("details-rename", "Rename").disabled(name_unchanged).on_click(cx.listener(|this, _, _, cx| this.rename(cx))),
                    ),
                )
        });

        let pinned = chat.pinned;
        let muted = chat.muted;
        let receipts_off = chat.read_receipts == Some(false);
        let subtitle = if chat.is_group { format!("{} people · {}", chat.participants.len(), service_label(chat.service)) } else { service_label(chat.service).to_owned() };

        div()
            .id("details-panel")
            .debug_selector(|| "info-panel".into())
            .w(INFO_WIDTH)
            .h_full()
            .flex()
            .flex_col()
            .bg(palette.sidebar)
            .border_l_1()
            .border_color(palette.sidebar_border)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .h(TITLEBAR_HEIGHT)
                    .pl(spacing::X4)
                    .pr(spacing::X2 + crate::chrome::caption_reserve(window))
                    .flex_shrink_0()
                    .child(div().flex_grow(1.).text_size(type_scale::TITLE.font_size).line_height(type_scale::TITLE.line_height).font_weight(FontWeight::SEMIBOLD).text_color(palette.text).child("Details"))
                    .child(IconButton::new("close-details", IconName::Close, format!("Close details ({})", shortcut("I", false, false))).color(palette.secondary).on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(crate::app::ToggleInfo), cx);
                    })),
            )
            .child(
                div()
                    .id("details-scroll")
                    .flex()
                    .flex_col()
                    .flex_grow(1.)
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(spacing::X2)
                            .pt(spacing::X1)
                            .pb(spacing::X5)
                            .px(spacing::X4)
                            .flex_shrink_0()
                            .child(avatar(chat.participants.first(), Some(&chat), px(72.), cx))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(2.))
                                    .child(div().text_size(px(17.)).line_height(px(22.)).font_weight(FontWeight::BOLD).text_color(palette.text).text_align(TextAlign::Center).child(title.clone()))
                                    .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).text_align(TextAlign::Center).child(subtitle)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .px(spacing::X2)
                            .flex_shrink_0()
                            .child(action_row(&palette, "pin", if pinned { IconName::PinOff } else { IconName::Pin }, if pinned { "Unpin" } else { "Pin" }, None, false, {
                                let guid = guid.clone();
                                run(move |store| store.toggle_pin(&guid))
                            }))
                            .child(action_row(&palette, "mute", if muted { IconName::Unmute } else { IconName::Mute }, if muted { "Show alerts" } else { "Hide alerts" }, None, false, {
                                let guid = guid.clone();
                                run(move |store| store.toggle_mute(&guid))
                            }))
                            .child(action_row(
                                &palette,
                                "receipts",
                                if receipts_off { IconName::Eye } else { IconName::EyeOff },
                                if receipts_off { "Send read receipts" } else { "Read without receipts" },
                                None,
                                false,
                                {
                                    let guid = guid.clone();
                                    run(move |store| store.toggle_read_receipts(&guid))
                                },
                            ))
                            .child(action_row(&palette, "unread", IconName::MarkUnread, "Mark as unread", None, false, {
                                let guid = guid.clone();
                                spawn_store(move |store| {
                                    let guid = guid.clone();
                                    async move { store.mark_unread(&guid).await }
                                })
                            })),
                    )
                    .child(section_divider(&palette))
                    .child(section_label(if chat.is_group { "People" } else { "Contact" }, palette.tertiary, spacing::X4))
                    .child(people_column)
                    .children(gallery)
                    .children(manage_fields)
                    .child(section_divider(&palette))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .px(spacing::X2)
                            .pb(spacing::X4)
                            .flex_shrink_0()
                            .child(action_row(&palette, "export", IconName::Download, if exporting { "Exporting…" } else { "Export conversation…" }, None, false, {
                                let guid = guid.clone();
                                spawn_store(move |store| {
                                    let guid = guid.clone();
                                    async move { store.export_conversation(&guid).await }
                                })
                            }))
                            .when(chat.is_group && capabilities.group_management, |el| {
                                let (guid, title) = (guid.clone(), title.clone());
                                el.child(action_row(&palette, "leave", IconName::Leave, "Leave conversation", None, true, move |_, cx| {
                                    let guid = guid.clone();
                                    let request = ConfirmRequest::new(format!("Leave \u{201c}{title}\u{201d}?"), "Leave", spawn_store(move |store| {
                                        let guid = guid.clone();
                                        async move { store.leave_group(&guid).await }
                                    }))
                                    .body("You stop getting its messages. Someone in the group can add you back.")
                                    .danger();
                                    if let Some(app) = root(cx) {
                                        AppRoot::open_confirm(&app, request, cx);
                                    }
                                }))
                            })
                            .child({
                                let chat = chat.clone();
                                let app = app.clone();
                                action_row(&palette, "delete", IconName::Trash, "Delete conversation", None, true, move |_, cx| {
                                    let (Some(store), Some(app)) = (store(cx), app.clone()) else { return };
                                    crate::sidebar_row::confirm_delete(&chat, &store, app, cx);
                                })
                            }),
                    ),
            )
            .into_any_element()
    }
}

fn participant_row(palette: &Palette, handle: &Handle, chat: &Chat, manage: bool, addresses: &[String], cx: &mut Context<InfoPanel>) -> impl IntoElement {
    let menu_handle = handle.clone();
    let menu_chat = chat.clone();
    let copy = handle.address.clone();
    let named = handle.name.is_some();
    let removable = manage && chat.participants.len() > 2;
    let (remove_chat, remove_address) = (chat.guid.clone(), handle.address.clone());
    let _ = cx;
    div()
        .id(SharedString::from(format!("participant-{}", handle.address)))
        .tab_index(0)
        .flex()
        .flex_row()
        .items_center()
        .gap(spacing::X2)
        .min_h(px(40.))
        .py(spacing::X1)
        .pl(spacing::X2)
        .pr(spacing::X1)
        .rounded(radius::CONTROL)
        .flex_shrink_0()
        .border_2()
        .border_color(palette.transparent)
        .focus_visible({
            let ring = palette.focus_ring;
            move |style| style.border_color(ring)
        })
        .hover({
            let wash = palette.hover_wash;
            move |style| style.bg(wash)
        })
        .on_mouse_up(MouseButton::Right, move |event: &MouseUpEvent, window, cx| {
            if let Some(app) = root(cx) {
                AppRoot::open_menu(&app, MenuRequest::at(event.position, participant_menu(&menu_handle, &menu_chat, manage)), window, cx);
            }
        })
        .on_key_down(move |event: &KeyDownEvent, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                if let Some(store) = store(cx) {
                    let address = copy.clone();
                    store.spawn(async move { messages_core::clipboard::copy_text(&address).await });
                }
            }
        })
        .child(avatar(Some(handle), None, px(28.), cx))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_grow(1.)
                .min_w(px(0.))
                .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).text_color(palette.text).text_ellipsis().child(handle_name(handle).to_owned()))
                .when(named, |el| {
                    el.children(addresses.iter().map(|address| {
                        div().text_size(type_scale::MICRO.font_size).line_height(type_scale::MICRO.line_height).text_color(palette.secondary).text_ellipsis().child(format_address(address))
                    }))
                }),
        )
        .when(removable, |el| {
            el.child(IconButton::new(SharedString::from(format!("remove-{}", handle.address)), IconName::Close, format!("Remove {}", handle_name(handle))).size(px(12.)).hit(px(24.)).color(palette.secondary).on_click(
                move |_, _, cx| {
                    if let Some(store) = store(cx) {
                        let (task, chat, address) = (store.clone(), remove_chat.clone(), remove_address.clone());
                        store.spawn(async move { task.remove_participant(&chat, &address).await });
                    }
                },
            ))
        })
}
