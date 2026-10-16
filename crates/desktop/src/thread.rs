//! The open conversation as nd's virtual list, bottom-aligned and following the tail. The list
//! keeps its position as an anchor (row key plus offset), so older pages prepend without a
//! jump, a row that grows once its picture loads pushes nothing out of reach, and a jump to a
//! message is `scroll_to` with an offset. Rows are versioned by what they show: a tapback
//! re-renders one row.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use messages_core::assistant::CanaryLlmClient;
use messages_core::conversations::{
    conversation_focus, conversation_handles, conversation_has_older, conversation_key, conversation_loading,
    conversation_messages, conversation_typing,
};
use messages_core::{Chat, ConnectionStatus, FocusStatus, Handle, Message, MessagesStore, Service, handle_name};
use nd::prelude::*;

use crate::actions::*;
use crate::bridge::{self, Bridge, Topic};
use crate::message::{self, Kind, Media, Quote, RowProps, Translation};
use crate::rows::{BuildOptions, MessageRowData, Row, build_rows};
use crate::ui;

const TYPING_STEP: Duration = Duration::from_millis(400);
const HIGHLIGHT_FOR: Duration = Duration::from_millis(2200);
/// A jump leaves this much room above the message.
const JUMP_OFFSET: f32 = -96.;
/// Pages a jump loads at most looking for its message.
const JUMP_PAGES: usize = 12;
/// A row dated after the thread opened fades in; everything older paints at once. The slack
/// covers the Mac's clock running a little behind this one.
const ENTER_SLACK_MS: i64 = 5000;
/// How long after its first render a new row still mounts with the fade.
const ENTER_WINDOW: f64 = 0.4;
const ENTER: Duration = Duration::from_millis(180);

#[derive(Clone)]
pub struct Assistant {
    pub client: Arc<CanaryLlmClient>,
    pub language: String,
}

/// One list row.
#[derive(Clone)]
pub enum Slot {
    Caption { text: String, strong: usize },
    Message { data: MessageRowData, props: RowProps },
    Typing,
}

/// A message to show once the thread holds it: the search result path.
#[derive(Default)]
struct PendingJump(std::cell::RefCell<Option<(String, String)>>);

/// Opens `chat_guid` at `message_guid` once the thread shows it. The caller selects the chat.
pub fn request_jump(chat_guid: &str, message_guid: &str, cx: &mut App) {
    if !cx.has_global::<PendingJump>() {
        cx.set_global(PendingJump::default());
    }
    cx.global::<PendingJump>().0.replace(Some((chat_guid.to_owned(), message_guid.to_owned())));
    if let Some(thread) = cx.try_global::<ThreadHost>().and_then(|h| h.0.upgrade()) {
        thread.update(cx, |_, cx| cx.notify());
    }
}

struct ThreadHost(WeakEntity<Thread>);

pub struct Thread {
    list: ListState<String>,
    slots: Rc<std::cell::RefCell<HashMap<String, Slot>>>,
    watched: HashSet<Topic>,
    primary: Option<String>,
    list_key: Option<String>,
    chat: Option<Arc<Chat>>,
    typing_who: Option<Handle>,
    is_group: bool,
    /// Which of the typing bubble's dots is lit; stepped while the bubble shows.
    lit: usize,
    typing_timer: bool,
    empty: bool,
    loading_empty: bool,
    media: Media,
    translations: HashMap<String, Translation>,
    highlight: Option<String>,
    pending_jump: Option<String>,
    /// The root of the reply thread shown instead of the conversation.
    thread_for: Option<String>,
    lightbox: Option<crate::lightbox::Lightbox>,
    /// The message the tapback picker is open on.
    picker: Option<String>,
    assistant: Option<Assistant>,
    assistant_loaded: bool,
    requested_older: bool,
    /// When the open conversation was opened (`now_ms`), and the row keys it has shown.
    opened_at: i64,
    known: HashSet<String>,
    /// New rows that fade in, with the frame time they first rendered at.
    entering: HashMap<String, Option<f64>>,
    /// The row under the pointer.
    hovered: Option<String>,
}

impl Thread {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let weak = cx.weak_entity();
        for topic in [Topic::Selection, Topic::Connection, Topic::Focus, Topic::Contacts, Topic::ChatList] {
            Bridge::watch(cx, topic, &weak);
        }
        cx.set_global(ThreadHost(weak));
        Self {
            list: ListState::new(cx).alignment(Alignment::Bottom).follow(Follow::Tail).estimated_row_height(px(44.)),
            slots: Rc::default(),
            watched: HashSet::new(),
            primary: None,
            list_key: None,
            chat: None,
            typing_who: None,
            is_group: false,
            lit: 0,
            typing_timer: false,
            empty: false,
            loading_empty: false,
            media: Media::default(),
            translations: HashMap::new(),
            highlight: None,
            pending_jump: None,
            thread_for: None,
            lightbox: None,
            picker: None,
            assistant: None,
            assistant_loaded: false,
            requested_older: false,
            opened_at: 0,
            known: HashSet::new(),
            entering: HashMap::new(),
            hovered: None,
        }
    }

    /// The list, for tests and the switcher: its keys and scroll state.
    pub fn list(&self) -> &ListState<String> {
        &self.list
    }

    fn watch(&mut self, topic: Topic, cx: &mut Context<Self>) {
        if self.watched.insert(topic.clone()) {
            {
                let weak = cx.weak_entity();
                Bridge::watch(cx, topic, &weak);
            }
        }
    }

    fn unwatch_chat(&mut self, guid: &str, cx: &mut Context<Self>) {
        for topic in [Topic::Thread(guid.to_owned()), Topic::Typing(guid.to_owned()), Topic::Chat(guid.to_owned())] {
            if self.watched.remove(&topic) {
                {
                    let id = cx.entity_id();
                    Bridge::unwatch(cx, &topic, id);
                }
            }
        }
    }

    fn load_assistant(&mut self, cx: &mut Context<Self>) {
        self.assistant_loaded = true;
        let Some(llm) = bridge::config(cx).and_then(|c| c.canaryllm.clone()).filter(|llm| !llm.api_key.is_empty())
        else {
            return;
        };
        crate::rt::spawn_for(
            cx.weak_entity(),
            async move {
                let language = messages_core::assistant::translation_language(llm.language.as_deref()).await;
                let client = CanaryLlmClient::new(llm.api_key, llm.model, None, reqwest::Client::new());
                Assistant { client: Arc::new(client), language }
            },
            |this, assistant, cx| {
                this.assistant = Some(assistant);
                cx.notify();
            },
        );
    }

    pub fn assistant(&self) -> Option<Assistant> {
        self.assistant.clone()
    }

    /// Reads what the rows depend on and hands the list its keys; the list re-renders only
    /// rows whose version moved.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(store) = bridge::store(cx) else { return };
        let (guid, chat, messages, key, typing, loading, online, capabilities, focus) = {
            let state = store.state();
            let Some(guid) = state.selected_chat.clone() else {
                drop(state);
                if let Some(previous) = self.primary.take() {
                    self.unwatch_chat(&previous, cx);
                }
                self.chat = None;
                self.list.set_items_versioned(Vec::new(), cx);
                return;
            };
            let chat = state.chat(&guid).cloned();
            let focus = (conversation_focus(&state, &guid) == FocusStatus::Silenced)
                .then(|| conversation_handles(&state, &guid).first().map(|h| handle_name(h).to_owned()))
                .flatten();
            (
                guid.clone(),
                chat,
                conversation_messages(&state, &guid),
                conversation_key(&state.grouping, &guid),
                conversation_typing(&state, &guid),
                conversation_loading(&state, &guid),
                state.status == ConnectionStatus::Online,
                state.capabilities,
                focus,
            )
        };
        if let Some(previous) = self.primary.clone().filter(|p| *p != guid) {
            self.unwatch_chat(&previous, cx);
        }
        for topic in [Topic::Thread(guid.clone()), Topic::Typing(guid.clone()), Topic::Chat(guid.clone())] {
            self.watch(topic, cx);
        }
        let reset = self.list_key.as_deref() != Some(key.as_str());
        if reset {
            self.list_key = Some(key);
            self.thread_for = None;
            self.highlight = None;
            self.pending_jump = None;
            self.translations.clear();
            self.lightbox = None;
            self.requested_older = false;
            self.opened_at = ui::now_ms();
            self.known.clear();
            self.entering.clear();
            self.hovered = None;
        }
        let is_group = chat.as_ref().is_some_and(|chat| chat.is_group);
        self.is_group = is_group;
        self.typing_who = chat.as_ref().and_then(|chat| chat.participants.first().cloned());
        self.fetch_media(&store, &messages, cx);
        let props = PropsInput { primary: guid.clone(), is_group, capabilities, assistant: self.assistant.is_some() };
        let rows: Vec<Row> = match self.thread_for.clone() {
            Some(root) => reply_rows(&messages, &root, is_group),
            None => build_rows(
                &messages,
                &BuildOptions {
                    is_group,
                    typing,
                    loading: loading && !messages.is_empty(),
                    online,
                    silenced_by: focus.as_deref(),
                    now: ui::now_ms(),
                },
            ),
        };
        let slots = self.slots_for(&rows, &messages, &props);
        if self.thread_for.is_none() {
            let primed = !self.known.is_empty();
            for (key, slot) in &slots {
                if self.known.insert(key.clone())
                    && primed
                    && let Slot::Message { data, .. } = slot
                    && data.message.date > self.opened_at - ENTER_SLACK_MS
                {
                    self.entering.insert(key.clone(), None);
                }
            }
        }
        let mut keyed = Vec::with_capacity(slots.len());
        let mut map = HashMap::with_capacity(slots.len());
        for (key, slot) in slots {
            keyed.push((key.clone(), version(&slot, &self.media)));
            map.insert(key, slot);
        }
        *self.slots.borrow_mut() = map;
        self.empty = messages.is_empty() && !loading;
        self.loading_empty = messages.is_empty() && loading;
        self.primary = Some(guid);
        self.chat = chat;
        self.list.set_items_versioned(keyed, cx);
        if reset {
            self.list.scroll_to_end(cx);
        }
        self.typing_tick(typing && self.thread_for.is_none(), cx);
        if let Some(target) = self.pending_jump.clone()
            && self.scroll_to(&target, cx)
        {
            self.pending_jump = None;
        }
    }

    fn slots_for(&self, rows: &[Row], messages: &[Arc<Message>], input: &PropsInput) -> Vec<(String, Slot)> {
        let by_guid: HashMap<&str, &Arc<Message>> = messages.iter().map(|m| (m.guid.as_str(), m)).collect();
        let mut reply_counts: HashMap<&str, usize> = HashMap::new();
        if self.thread_for.is_none() {
            for message in messages {
                if let Some(target) = message.reply_to.as_deref() {
                    *reply_counts.entry(target).or_default() += 1;
                }
            }
        }
        rows.iter()
            .map(|row| match row {
                Row::Separator { key, label } => {
                    let strong = message::separator_day(label).len();
                    (key.clone(), Slot::Caption { text: label.clone(), strong })
                }
                Row::Event { key, text } => (key.clone(), Slot::Caption { text: text.clone(), strong: 0 }),
                Row::Loading => {
                    ("loading".to_owned(), Slot::Caption { text: "Loading earlier messages\u{2026}".into(), strong: 0 })
                }
                Row::Typing => ("typing".to_owned(), Slot::Typing),
                Row::Message(data) => {
                    let message = &data.message;
                    let quote = message.reply_to.as_deref().filter(|_| data.show_quote).map(|target| {
                        let original = by_guid.get(target);
                        let who = match original {
                            Some(original) if original.from_me => "You".to_owned(),
                            Some(original) => {
                                original.sender.as_ref().map(|s| handle_name(s).to_owned()).unwrap_or_default()
                            }
                            None => "Earlier message".to_owned(),
                        };
                        let body = match original {
                            Some(original) if !original.text.is_empty() => original.text.clone(),
                            Some(original) if !original.attachments.is_empty() => "Attachment".to_owned(),
                            Some(_) => String::new(),
                            None => "Click to load it".to_owned(),
                        };
                        Quote { target: target.to_owned(), who, body }
                    });
                    let count = reply_counts.get(message.guid.as_str()).copied().unwrap_or(0);
                    let props = RowProps {
                        primary: input.primary.clone(),
                        is_group: input.is_group,
                        capabilities: input.capabilities,
                        assistant: input.assistant,
                        quote,
                        replies: match count {
                            0 => None,
                            1 => Some("1 reply".into()),
                            n => Some(format!("{n} replies")),
                        },
                        highlighted: self.highlight.as_deref() == Some(message.guid.as_str()),
                        translation: self.translations.get(&message.guid).cloned(),
                        picker: self.picker.as_deref() == Some(message.guid.as_str()),
                        hovered: self.hovered.as_deref() == Some(data.key.as_str()),
                    };
                    (data.key.clone(), Slot::Message { data: data.clone(), props })
                }
            })
            .collect()
    }

    /// Starts the downloads and posters the visible messages need: pictures up to 25 MB and
    /// videos up to 60 MB fetch on their own, stickers always.
    fn fetch_media(&mut self, store: &MessagesStore, messages: &[Arc<Message>], cx: &mut Context<Self>) {
        for message in messages.iter().rev().take(200) {
            for attachment in message.attachments.iter().filter(|a| !a.hidden) {
                let kind = message::kind_of(message, attachment);
                let wants = attachment.local_path.is_none()
                    && match kind {
                        Kind::Sticker => true,
                        Kind::Image => attachment.bytes <= message::AUTO_DOWNLOAD_BYTES,
                        Kind::Video => attachment.bytes <= message::AUTO_DOWNLOAD_VIDEO_BYTES,
                        Kind::Audio | Kind::File => false,
                    };
                if wants
                    && !self.media.fetching.contains(&attachment.guid)
                    && !self.media.failed.contains(&attachment.guid)
                {
                    self.fetch(store, message, attachment, cx);
                }
                if kind == Kind::Video
                    && let Some(path) = attachment.local_path.clone()
                    && !self.media.posters.contains_key(&attachment.guid)
                {
                    self.media.posters.insert(attachment.guid.clone(), std::path::PathBuf::new());
                    let guid = attachment.guid.clone();
                    let worker = poster_worker();
                    crate::rt::spawn_for(
                        cx.weak_entity(),
                        async move { worker.video_poster(&path, &guid).await.map(|p| (guid, p)) },
                        |this, poster, cx| {
                            if let Some((guid, path)) = poster {
                                this.media.posters.insert(guid, path);
                                cx.notify();
                            }
                        },
                    );
                }
            }
        }
    }

    fn fetch(
        &mut self,
        store: &MessagesStore,
        message: &Message,
        attachment: &messages_core::Attachment,
        cx: &mut Context<Self>,
    ) {
        self.media.fetching.insert(attachment.guid.clone());
        let store = store.clone();
        let (chat, guid, attachment_guid) = (message.chat_guid.clone(), message.guid.clone(), attachment.guid.clone());
        let (name, mime) = (attachment.name.clone(), attachment.mime.clone());
        let key = attachment_guid.clone();
        crate::rt::spawn_for(
            cx.weak_entity(),
            async move { store.attachment_src(&chat, &guid, &attachment_guid, &name, Some(&mime)).await.is_ok() },
            move |this, ok, cx| {
                this.media.fetching.remove(&key);
                if !ok {
                    this.media.failed.insert(key);
                }
                cx.notify();
            },
        );
    }

    /// Runs `then` with the attachment's file, downloading it first when it is not on disk.
    fn with_local(
        &mut self,
        reference: &AttachmentRef,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, std::path::PathBuf, String, &mut Context<Self>) + 'static,
    ) {
        let Some(store) = bridge::store(cx) else { return };
        let Some(attachment) = store
            .state()
            .find_message(&reference.at.chat, &reference.at.message)
            .and_then(|m| m.attachments.iter().find(|a| a.guid == reference.attachment).cloned())
        else {
            return;
        };
        if let Some(path) = attachment.local_path.clone().filter(|p| !p.to_string_lossy().starts_with("data:")) {
            then(self, path, attachment.mime.clone(), cx);
            return;
        }
        let (chat, message) = (reference.at.chat.clone(), reference.at.message.clone());
        let mime = attachment.mime.clone();
        let fetch_mime = mime.clone();
        crate::rt::spawn_for(
            cx.weak_entity(),
            async move {
                store.attachment_src(&chat, &message, &attachment.guid, &attachment.name, Some(&fetch_mime)).await.ok()
            },
            move |this, path, cx| {
                if let Some(path) = path {
                    then(this, path, mime, cx);
                }
            },
        );
    }

    pub fn open_attachment(&mut self, reference: &AttachmentRef, cx: &mut Context<Self>) {
        let audio = bridge::store(cx).is_some_and(|store| {
            store.state().find_message(&reference.at.chat, &reference.at.message).is_some_and(|m| {
                m.attachments.iter().any(|a| a.guid == reference.attachment && message::kind_of(m, a) == Kind::Audio)
            })
        });
        if audio {
            self.toggle_audio(reference, cx);
            return;
        }
        self.with_local(reference, cx, |_, path, _, _| {
            let _ = nd::system::open_path(&path);
        });
    }

    fn toggle_audio(&mut self, reference: &AttachmentRef, cx: &mut Context<Self>) {
        if self.media.playing.take().is_some() {
            messages_core::open::stop_audio();
            cx.notify();
            return;
        }
        let guid = reference.attachment.clone();
        self.with_local(reference, cx, move |this, path, _, cx| {
            if messages_core::open::play_audio(&path) {
                this.media.playing = Some(guid);
                this.watch_player(cx);
                cx.notify();
            }
        });
    }

    /// The player is a child process nobody hears back from, so it is checked every 400 ms.
    fn watch_player(&mut self, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        crate::rt::after(Duration::from_millis(400), move |cx| {
            weak.update(cx, |this, cx| {
                if this.media.playing.is_some() {
                    if messages_core::open::is_audio_playing() {
                        this.watch_player(cx);
                    } else {
                        this.media.playing = None;
                        cx.notify();
                    }
                }
            });
        });
    }

    pub fn copy_attachment(&mut self, reference: &AttachmentRef, cx: &mut Context<Self>) {
        self.with_local(reference, cx, |_, path, mime, _| {
            let mime = if mime.is_empty() { mime_guess(&path) } else { mime };
            crate::rt::detach(async move { messages_core::clipboard::copy_file(&path, &mime).await });
        });
    }

    /// Fetches an attachment the thread left for a click (over the size it downloads alone).
    pub fn download(&mut self, reference: &AttachmentRef, cx: &mut Context<Self>) {
        self.media.failed.remove(&reference.attachment);
        self.media.fetching.insert(reference.attachment.clone());
        let key = reference.attachment.clone();
        cx.notify();
        self.with_local(reference, cx, move |this, _, _, cx| {
            this.media.fetching.remove(&key);
            cx.notify();
        });
    }

    /// A voice note to text through CanaryLLM, shown under the note.
    pub fn transcribe(&mut self, reference: &AttachmentRef, cx: &mut Context<Self>) {
        let Some(assistant) = self.assistant.clone() else { return };
        if self.media.transcripts.get(&reference.attachment) == Some(&Translation::Loading) {
            return;
        }
        let key = reference.attachment.clone();
        self.media.transcripts.insert(key.clone(), Translation::Loading);
        cx.notify();
        self.with_local(reference, cx, move |_, path, _, cx| {
            crate::rt::spawn_for(
                cx.weak_entity(),
                async move { assistant.client.transcribe(&path).await },
                move |this, result, cx| {
                    let value = match result {
                        Ok(text) => Translation::Text(text),
                        Err(error) => Translation::Error(error.0),
                    };
                    this.media.transcripts.insert(key, value);
                    cx.notify();
                },
            );
        });
    }

    /// Previews an attachment as Messages does: Quick Look over every attachment of the loaded
    /// conversation that is on disk, zooming out of the one clicked. Where the platform has no
    /// preview panel, or the file is not here yet, the in-window lightbox shows it.
    pub fn show_attachment(&mut self, reference: &AttachmentRef, window: nd::core::WindowId, cx: &mut Context<Self>) {
        let Some(store) = bridge::store(cx) else { return };
        let items: Vec<(String, nd::system::PreviewItem)> = {
            let state = store.state();
            let Some(chat) = state.selected_chat.clone() else { return };
            conversation_messages(&state, &chat)
                .iter()
                .flat_map(|m| m.attachments.iter())
                .filter(|a| !a.hidden)
                .filter_map(|a| {
                    let path = a.local_path.clone()?;
                    let source = if a.mime.starts_with("image/") { format!("img-{}", a.guid) } else { format!("att-{}", a.guid) };
                    Some((a.guid.clone(), nd::system::PreviewItem { path, source: Some(source.into()) }))
                })
                .collect()
        };
        if let Some(index) = items.iter().position(|(guid, _)| *guid == reference.attachment) {
            let items = items.into_iter().map(|(_, item)| item).collect();
            if nd::system::quick_look(window, items, index, cx) {
                return;
            }
        }
        self.lightbox = crate::lightbox::Lightbox::open(&store, reference);
        self.fetch_lightbox(cx);
        cx.notify();
    }

    pub fn step_lightbox(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(lightbox) = self.lightbox.as_mut() {
            lightbox.step(delta);
            self.fetch_lightbox(cx);
            cx.notify();
        }
    }

    /// Downloads what the lightbox shows when the thread left it for a click.
    fn fetch_lightbox(&mut self, cx: &mut Context<Self>) {
        let Some(lightbox) = self.lightbox.as_ref() else { return };
        if lightbox.needs_download() && let Some(reference) = lightbox.current()
            && !self.media.fetching.contains(&reference.attachment)
        {
            self.download(&reference, cx);
        }
    }

    pub fn copy_lightbox(&mut self, cx: &mut Context<Self>) {
        if let Some(reference) = self.lightbox.as_mut().and_then(|l| {
            l.mark_copied();
            l.current()
        }) {
            self.copy_attachment(&reference, cx);
        }
        cx.notify();
    }

    pub fn lightbox_mut(&mut self) -> Option<&mut crate::lightbox::Lightbox> {
        self.lightbox.as_mut()
    }

    pub fn close_lightbox(&mut self, cx: &mut Context<Self>) {
        self.lightbox = None;
        cx.notify();
    }

    /// Steps the typing dots while the bubble shows, and stops when it goes.
    fn typing_tick(&mut self, typing: bool, cx: &mut Context<Self>) {
        if !typing || self.typing_timer {
            return;
        }
        self.typing_timer = true;
        let weak = cx.weak_entity();
        crate::rt::after(TYPING_STEP, move |cx| {
            weak.update(cx, |this, cx| {
                this.typing_timer = false;
                if this.slots.borrow().contains_key("typing") {
                    this.lit = (this.lit + 1) % 3;
                    this.list.invalidate(&"typing".to_owned(), cx);
                    this.typing_tick(true, cx);
                }
            });
        });
    }

    fn scroll_to(&mut self, guid: &str, cx: &mut Context<Self>) -> bool {
        let key = self.slots.borrow().iter().find_map(|(key, slot)| match slot {
            Slot::Message { data, .. } if data.message.guid == guid => Some(key.clone()),
            _ => None,
        });
        let Some(key) = key else { return false };
        self.list.scroll_to(key, Align::Start, px(JUMP_OFFSET), cx);
        self.highlight = Some(guid.to_owned());
        let target = guid.to_owned();
        let weak = cx.weak_entity();
        crate::rt::after(HIGHLIGHT_FOR, move |cx| {
            weak.update(cx, |this, cx| {
                if this.highlight.as_deref() == Some(target.as_str()) {
                    this.highlight = None;
                    cx.notify();
                }
            });
        });
        cx.notify();
        true
    }

    /// Scrolls to a quoted message, paging older history (up to twelve pages) until it is in
    /// memory.
    pub fn jump_to(&mut self, guid: &str, cx: &mut Context<Self>) {
        if self.thread_for.take().is_some() {
            cx.notify();
        }
        if self.scroll_to(guid, cx) {
            return;
        }
        let (Some(store), Some(primary)) = (bridge::store(cx), self.primary.clone()) else { return };
        self.pending_jump = Some(guid.to_owned());
        let target = guid.to_owned();
        crate::rt::spawn_for(
            cx.weak_entity(),
            async move {
                for _ in 0..JUMP_PAGES {
                    let (found, older) = {
                        let state = store.state();
                        (state.find_message(&primary, &target).is_some(), conversation_has_older(&state, &primary))
                    };
                    if found || !older {
                        break;
                    }
                    store.load_earlier(&primary).await;
                }
            },
            |_, (), cx| cx.notify(),
        );
    }

    pub fn set_picker(&mut self, guid: Option<String>, cx: &mut Context<Self>) {
        if self.picker != guid {
            self.picker = guid;
            cx.notify();
        }
    }

    pub fn open_reply_thread(&mut self, guid: &str, cx: &mut Context<Self>) {
        self.thread_for = Some(guid.to_owned());
        cx.notify();
    }

    pub fn close_reply_thread(&mut self, cx: &mut Context<Self>) {
        self.thread_for = None;
        cx.notify();
    }

    pub fn toggle_translation(&mut self, guid: &str, cx: &mut Context<Self>) {
        cx.notify();
        if self.translations.remove(guid).is_some() {
            return;
        }
        let Some(assistant) = self.assistant.clone() else { return };
        let Some(text) = bridge::store(cx).and_then(|store| {
            let state = store.state();
            self.primary.as_deref().and_then(|p| {
                conversation_messages(&state, p).into_iter().find(|m| m.guid == guid).map(|m| m.text.clone())
            })
        }) else {
            return;
        };
        self.translations.insert(guid.to_owned(), Translation::Loading);
        let key = guid.to_owned();
        crate::rt::spawn_for(
            cx.weak_entity(),
            async move { assistant.client.translate(&text, &assistant.language).await },
            move |this, result, cx| {
                if let Some(slot) = this.translations.get_mut(&key) {
                    *slot = match result {
                        Ok(text) => Translation::Text(text),
                        Err(error) => Translation::Error(error.0),
                    };
                    cx.notify();
                }
            },
        );
    }

    fn load_older(&mut self, cx: &mut Context<Self>) {
        if self.requested_older || self.empty || self.thread_for.is_some() {
            return;
        }
        let (Some(store), Some(primary)) = (bridge::store(cx), self.primary.clone()) else { return };
        {
            let state = store.state();
            if !conversation_has_older(&state, &primary) || conversation_loading(&state, &primary) {
                return;
            }
        }
        self.requested_older = true;
        crate::rt::spawn_for(cx.weak_entity(), async move { store.load_earlier(&primary).await }, |this, (), cx| {
            this.requested_older = false;
            cx.notify();
        });
    }

    fn row(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let slot = self.slots.borrow().get(&key).cloned();
        match slot {
            None => div().into_any_element(),
            Some(Slot::Caption { text, strong }) => message::caption_row(&key, &text, strong, &theme),
            Some(Slot::Typing) => message::typing_row(self.lit, self.typing_who.as_ref(), self.is_group, &theme),
            Some(Slot::Message { data, props }) => {
                let entering = match self.entering.get_mut(&key) {
                    Some(first) => {
                        let now = window.now();
                        let started = *first.get_or_insert(now);
                        let fresh = now - started < ENTER_WINDOW;
                        if !fresh {
                            self.entering.remove(&key);
                        }
                        fresh
                    }
                    None => false,
                };
                let hover_key = key.clone();
                div()
                    .w_full()
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| this.hover_row(&hover_key, *hovered, cx)))
                    .when(entering, |row| row.animate(Animation::ease_out(ENTER)).enter_from(|pose| pose.opacity(0.)))
                    .child(message::render(&data, &props, &self.media, &theme))
                    .into_any_element()
            }
        }
    }

    fn hover_row(&mut self, key: &str, hovered: bool, cx: &mut Context<Self>) {
        let next = if hovered {
            Some(key.to_owned())
        } else if self.hovered.as_deref() == Some(key) {
            None
        } else {
            return;
        };
        if next != self.hovered {
            self.hovered = next;
            cx.notify();
        }
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> AnyElement {
        let label = if self.chat.as_ref().is_some_and(|chat| chat.service != Service::IMessage) {
            "Text Message"
        } else {
            "iMessage"
        };
        let theme = cx.theme().clone();
        v_flex()
            .id("thread-empty")
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .children(self.chat.as_ref().map(|chat| ui::chat_avatar(chat, 64.)))
            .child(label_text(label, &theme))
            .child(text("Send the first message below.").text_color(theme.text.secondary))
            .into_any_element()
    }
}

fn label_text(value: &'static str, theme: &Theme) -> Text {
    text(value).font_weight(FontWeight::SEMIBOLD).text_size(theme.type_scale.headline).text_color(theme.text.primary)
}

fn mime_guess(path: &std::path::Path) -> String {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        _ => "application/octet-stream",
    }
    .to_owned()
}

fn poster_worker() -> Arc<messages_core::media::MediaWorker> {
    static WORKER: std::sync::OnceLock<Arc<messages_core::media::MediaWorker>> = std::sync::OnceLock::new();
    WORKER.get_or_init(messages_core::media::MediaWorker::new).clone()
}

struct PropsInput {
    primary: String,
    is_group: bool,
    capabilities: messages_core::Capabilities,
    assistant: bool,
}

/// The rows of a reply thread: the root message, then every reply, each standing alone.
fn reply_rows(messages: &[Arc<Message>], root: &str, is_group: bool) -> Vec<Row> {
    let mut subset: Vec<Arc<Message>> = messages.iter().filter(|m| m.guid == root).cloned().collect();
    subset.extend(messages.iter().filter(|m| m.reply_to.as_deref() == Some(root)).cloned());
    let options =
        BuildOptions { is_group, typing: false, loading: false, online: true, silenced_by: None, now: ui::now_ms() };
    build_rows(&subset, &options)
        .into_iter()
        .filter_map(|row| match row {
            Row::Message(mut data) => {
                data.position = RunPosition::Single;
                data.show_sender = is_group && !data.message.from_me;
                data.receipt = None;
                data.notify = false;
                data.show_quote = false;
                Some(Row::Message(data))
            }
            Row::Separator { .. } => Some(row),
            _ => None,
        })
        .collect()
}

/// Swiping a message right replies to it, as in Messages: the table's leading row action, run
/// by a full swipe. A bare arrow with no fill, as Messages shows it, so the message slides back
/// over it rather than cutting a button in half.
fn reply_action(slots: &HashMap<String, Slot>, key: &str) -> RowActions {
    match slots.get(key) {
        Some(Slot::Message { data, props }) if props.capabilities.replies && !message::failed(&data.message) => {
            RowActions::new()
                .leading(
                    row_action("Reply").icon("reply").tint(gpui::transparent_black()).dispatch(Reply(message::at(data))),
                )
                .full_swipe(RowEdge::Leading)
        }
        _ => RowActions::new(),
    }
}

/// What a row's output depends on, hashed: the list re-renders a row only when it moves.
fn version(slot: &Slot, media: &Media) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    match slot {
        Slot::Caption { text, strong } => (0u8, text, strong).hash(&mut h),
        Slot::Typing => 1u8.hash(&mut h),
        Slot::Message { data, props } => {
            2u8.hash(&mut h);
            (Arc::as_ptr(&data.message) as usize).hash(&mut h);
            (format!("{:?}", data.position), data.show_sender, data.notify, data.show_quote).hash(&mut h);
            data.receipt.as_ref().map(|r| (&r.text, r.failed)).hash(&mut h);
            (&props.quote, &props.replies, props.highlighted, &props.translation, props.assistant, props.picker)
                .hash(&mut h);
            props.hovered.hash(&mut h);
            format!("{:?}", props.capabilities).hash(&mut h);
            for attachment in &data.message.attachments {
                let guid = &attachment.guid;
                (
                    media.fetching.contains(guid),
                    media.failed.contains(guid),
                    media.posters.get(guid),
                    media.playing.as_ref() == Some(guid),
                    media.transcripts.get(guid),
                )
                    .hash(&mut h);
            }
        }
    }
    h.finish()
}

impl Render for Thread {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.assistant_loaded && bridge::config(cx).is_some() {
            self.load_assistant(cx);
        }
        self.refresh(cx);
        let jump = cx.try_global::<PendingJump>().and_then(|j| j.0.borrow().clone());
        if let Some((chat, message)) = jump
            && self.primary.as_deref() == Some(chat.as_str())
        {
            cx.global::<PendingJump>().0.replace(None);
            self.jump_to(&message, cx);
        }

        let header = self.thread_for.as_ref().map(|root| {
            let replies = self
                .slots
                .borrow()
                .values()
                .filter(|s| matches!(s, Slot::Message { data, .. } if data.message.guid != *root))
                .count();
            h_flex()
                .id("reply-thread-header")
                .gap_2()
                .h(px(40.))
                .px_3()
                .flex_none()
                .child(
                    button("thread-back", "Back")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_reply_thread(cx))),
                )
                .child(label("Thread").text_style(TextStyle::Headline))
                .child(
                    label(if replies == 1 { "1 reply".to_owned() } else { format!("{replies} replies") }).secondary(),
                )
        });
        let body = if self.primary.is_none() {
            div().into_any_element()
        } else if self.empty && self.thread_for.is_none() {
            self.render_empty(cx)
        } else if self.loading_empty {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(label("Loading earlier messages\u{2026}").secondary())
                .into_any_element()
        } else {
            let slots = self.slots.clone();
            list("thread", &self.list, cx.processor(Self::row))
                .estimated_row_height(px(44.))
                .row_actions(move |key: String, _| reply_action(&slots.borrow(), &key))
                .on_near_start({
                    let weak = cx.weak_entity();
                    move |_, cx| {
                        weak.update(cx, |this, cx| this.load_older(cx));
                    }
                })
                .size_full()
                .into_any_element()
        };
        let lightbox = self.lightbox.as_ref().map(|lightbox| lightbox.element(cx));
        v_flex()
            .id("thread-view")
            .size_full()
            .children(header)
            .child(div().flex_1().min_h_0().child(body))
            .children(lightbox)
    }
}
