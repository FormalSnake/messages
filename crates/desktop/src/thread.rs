//! Port of `Thread` in `apps/desktop/src/ui/thread.tsx`. The open
//! conversation as a virtualized, bottom-aligned `list` that follows the tail.
//! Rows are rebuilt only when what they are made of changes (compared by
//! `Arc` pointer every render, which costs one pass over the thread's
//! pointers); only rows the list actually lays out get a `MessageRow` entity,
//! and those are kept across rebuilds by key, so a chat switch paints from
//! memory in the frame it happens.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::assistant::CanaryLlmClient;
use messages_core::conversations::{conversation_focus, conversation_handles, conversation_has_older, conversation_key, conversation_loading, conversation_messages, conversation_typing};
use messages_core::{Capabilities, Chat, ConnectionStatus, FocusStatus, Handle, Message, MessagesStore, Service, handle_name};

use crate::attachments::run_for;
use crate::bridge::{Bridge, StoreHandle, Topic};
use crate::bubble::{AVATAR_COLUMN, MessageRow, Quote, RowProps, Translation};
use crate::lightbox::Lightbox;
use crate::reply_thread::ReplyThread;
use crate::theme::{THREAD_INSET, Theme, radius, spacing, type_scale};
use crate::thread_rows::{BuildOptions, MessageRowData, Row, build_rows, now_ms};

/// A row dated after the thread opened fades in; everything older paints at
/// once. The slack covers the Mac's clock running a little behind this one.
const ENTER_SLACK_MS: i64 = 5000;
const TYPING_STEP: Duration = Duration::from_millis(400);
const JUMP_OFFSET: Pixels = px(-96.);
const HIGHLIGHT_FOR: Duration = Duration::from_millis(2200);
const JUMP_PAGES: usize = 12;
/// Load the next page once the top row is this close to the viewport.
const OLDER_WITHIN_ROWS: usize = 3;

#[derive(Clone)]
pub struct Assistant {
    pub client: Arc<CanaryLlmClient>,
    pub language: String,
}

#[derive(Default)]
struct AssistantSlot(Option<Assistant>);

impl Global for AssistantSlot {}

/// The CanaryLLM client behind translate and transcribe, when config.json has a key.
pub fn assistant(cx: &App) -> Option<Assistant> {
    cx.try_global::<AssistantSlot>().and_then(|slot| slot.0.clone())
}

struct ThreadHost(WeakEntity<Thread>);

impl Global for ThreadHost {}

#[derive(Default)]
struct PendingJump(Option<(String, String)>);

impl Global for PendingJump {}

/// Opens `chat_guid` at `message_guid` once the thread shows it: the search
/// result path (thread.tsx `consumeJump`). The caller selects the chat.
pub fn request_jump(chat_guid: &str, message_guid: &str, cx: &mut App) {
    cx.set_global(PendingJump(Some((chat_guid.to_owned(), message_guid.to_owned()))));
    if let Some(host) = cx.try_global::<ThreadHost>().map(|host| host.0.clone()) {
        let _ = host.update(cx, |_, cx| cx.notify());
    }
}

pub(crate) fn with_thread(cx: &mut App, f: impl FnOnce(&mut Thread, &mut Context<Thread>)) {
    if let Some(host) = cx.try_global::<ThreadHost>().map(|host| host.0.clone()) {
        let _ = host.update(cx, f);
    }
}

/// One list item: a caption, a message, or the typing bubble.
#[derive(Clone)]
pub(crate) enum Slot {
    Caption { key: String, text: SharedString, top: Pixels, bottom: Pixels },
    Message { data: MessageRowData, props: RowProps, entering: bool },
    Typing,
}

impl Slot {
    fn key(&self) -> &str {
        match self {
            Slot::Caption { key, .. } => key,
            Slot::Message { data, .. } => &data.key,
            Slot::Typing => "typing",
        }
    }

    fn same(&self, other: &Slot) -> bool {
        match (self, other) {
            (Slot::Caption { text: a, top: at, bottom: ab, .. }, Slot::Caption { text: b, top: bt, bottom: bb, .. }) => a == b && at == bt && ab == bb,
            (Slot::Message { data: a, props: ap, .. }, Slot::Message { data: b, props: bp, .. }) => a == b && ap == bp,
            (Slot::Typing, Slot::Typing) => true,
            _ => false,
        }
    }
}

pub(crate) struct Slots {
    slots: Vec<Slot>,
    rows: HashMap<String, Entity<MessageRow>>,
    typing: Option<Entity<TypingRow>>,
    typing_who: Option<Handle>,
    is_group: bool,
    store: MessagesStore,
    thread: WeakEntity<Thread>,
}

/// A list of slots with the row entities behind it. The main thread and the
/// reply thread each own one.
pub(crate) struct RowList {
    slots: Rc<RefCell<Slots>>,
    pub(crate) state: ListState,
    follow: bool,
}

impl RowList {
    pub(crate) fn new(store: MessagesStore, thread: WeakEntity<Thread>, alignment: ListAlignment) -> Self {
        let state = ListState::new(0, alignment, px(700.));
        if alignment == ListAlignment::Bottom {
            state.set_follow_mode(FollowMode::Tail);
        }
        let follow = alignment == ListAlignment::Bottom;
        RowList { slots: Rc::new(RefCell::new(Slots { slots: Vec::new(), rows: HashMap::new(), typing: None, typing_who: None, is_group: false, store, thread })), state, follow }
    }

    fn state_follows(&self) -> bool {
        self.follow
    }

    pub(crate) fn index_of(&self, message_guid: &str) -> Option<usize> {
        self.slots.borrow().slots.iter().position(|slot| matches!(slot, Slot::Message { data, .. } if data.message.guid == message_guid))
    }

    /// Replaces the slots, keeping measured heights and scroll position for
    /// rows that did not move. `reset` starts over at the bottom (a new chat).
    pub(crate) fn apply(&self, next: Vec<Slot>, is_group: bool, typing_who: Option<Handle>, reset: bool, cx: &mut App) {
        let mut guard = self.slots.borrow_mut();
        let slots = &mut *guard;
        slots.is_group = is_group;
        slots.typing_who = typing_who;
        if reset {
            slots.rows.clear();
            slots.typing = None;
            self.state.reset(next.len());
            if self.state_follows() {
                self.state.set_follow_mode(FollowMode::Tail);
            }
            slots.slots = next;
            return;
        }
        let old = &slots.slots;
        let prefix = old.iter().zip(next.iter()).take_while(|(a, b)| a.key() == b.key()).count();
        let max_suffix = old.len().min(next.len()) - prefix;
        let suffix = old.iter().rev().zip(next.iter().rev()).take(max_suffix).take_while(|(a, b)| a.key() == b.key()).count();
        let mut changed = Vec::new();
        for index in (0..prefix).chain(next.len() - suffix..next.len()) {
            let old_index = if index < prefix { index } else { index + old.len() - next.len() };
            if !old[old_index].same(&next[index]) {
                changed.push(index);
            }
        }
        if prefix + suffix != old.len() || prefix + suffix != next.len() {
            self.state.splice(prefix..old.len() - suffix, next.len() - prefix - suffix);
        }
        for index in changed {
            self.state.remeasure_items(index..index + 1);
        }
        let keys: HashSet<&str> = next.iter().map(Slot::key).collect();
        slots.rows.retain(|key, _| keys.contains(key.as_str()));
        if !next.iter().any(|slot| matches!(slot, Slot::Typing)) {
            slots.typing = None;
        }
        for slot in &next {
            if let Slot::Message { data, props, .. } = slot {
                if let Some(row) = slots.rows.get(&data.key) {
                    let (data, props) = (data.clone(), props.clone());
                    row.update(cx, |row, cx| {
                        if row.data != data || row.props != props {
                            row.set(data, props, cx);
                        }
                    });
                }
            }
        }
        slots.slots = next;
    }

    pub(crate) fn element(&self) -> List {
        let slots = self.slots.clone();
        list(self.state.clone(), move |index, _window, cx| {
            let mut guard = slots.borrow_mut();
            let Slots { slots, rows, typing, typing_who, is_group, store, thread } = &mut *guard;
            match slots.get(index) {
                None => div().into_any_element(),
                Some(Slot::Caption { text, top, bottom, .. }) => caption(text.clone(), *top, *bottom, cx).into_any_element(),
                Some(Slot::Typing) => {
                    let (who, group) = (typing_who.clone(), *is_group);
                    typing.get_or_insert_with(|| cx.new(|_| TypingRow::new(who, group))).clone().into_any_element()
                }
                Some(Slot::Message { data, props, entering }) => rows
                    .entry(data.key.clone())
                    .or_insert_with(|| {
                        let (store, thread, data, props, entering) = (store.clone(), thread.clone(), data.clone(), props.clone(), *entering);
                        cx.new(|cx| MessageRow::new(store, thread, data, props, entering, cx))
                    })
                    .clone()
                    .into_any_element(),
            }
        })
    }
}

fn caption(text: SharedString, top: Pixels, bottom: Pixels, cx: &App) -> Div {
    let palette = Theme::get(cx);
    div().w_full().flex().flex_row().items_center().justify_center().pt(top).pb(bottom).px(spacing::X10).child(
        div()
            .text_size(type_scale::MICRO.font_size)
            .line_height(type_scale::MICRO.line_height)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette.tertiary)
            .text_center()
            .child(text),
    )
}

/// The bubble fades in, then one dot at a time lights, the way Messages shows
/// someone typing. The step timer runs only while the row is being rendered.
pub struct TypingRow {
    who: Option<Handle>,
    is_group: bool,
    lit: usize,
    entered: Instant,
    rendered: bool,
    timer: Option<Task<()>>,
}

impl TypingRow {
    fn new(who: Option<Handle>, is_group: bool) -> Self {
        TypingRow { who, is_group, lit: 0, entered: Instant::now(), rendered: false, timer: None }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        self.timer = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TYPING_STEP).await;
                let running = this
                    .update(cx, |row, cx| {
                        if !std::mem::take(&mut row.rendered) {
                            row.timer = None;
                            return false;
                        }
                        row.lit = (row.lit + 1) % 3;
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        }));
    }
}

impl Render for TypingRow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        self.rendered = true;
        if self.timer.is_none() {
            self.start(cx);
        }
        let opacity = {
            let elapsed = self.entered.elapsed();
            if elapsed < crate::motion::DURATION_BASE {
                window.request_animation_frame();
                let (x1, y1, x2, y2) = crate::motion::EASE_OUT;
                Some(crate::motion::cubic_bezier(x1, y1, x2, y2)(elapsed.as_secs_f32() / crate::motion::DURATION_BASE.as_secs_f32()))
            } else {
                None
            }
        };
        let lit = self.lit;
        div()
            .flex()
            .flex_row()
            .items_end()
            .gap(spacing::X2)
            .w_full()
            .px(THREAD_INSET)
            .pt(px(10.))
            .when_some(opacity, |el, opacity| el.opacity(opacity))
            .when(self.is_group, |el| {
                el.child(div().w(px(AVATAR_COLUMN)).flex_shrink_0().when_some(self.who.clone(), |el, who| el.child(crate::primitives::avatar(Some(&who), None, px(AVATAR_COLUMN), cx))))
            })
            .child(crate::attachments::tail_box(
                false,
                Some(crate::attachments::TailFill::Color(palette.received)),
                &palette,
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(5.))
                    .h(px(34.))
                    .px(spacing::X3)
                    .rounded(radius::BUBBLE)
                    .bg(palette.received)
                    .children((0..3).map(|index| div().w(px(7.)).h(px(7.)).rounded(px(4.)).bg(palette.secondary).opacity(if index == lit { 1. } else { 0.35 }))),
            ))
    }
}

/// What the rows are made of; a render that finds these unchanged does no work.
#[derive(Default, PartialEq)]
struct Inputs {
    primary: Option<String>,
    chat: usize,
    messages: Vec<usize>,
    typing: bool,
    loading: bool,
    online: bool,
    capabilities: Capabilities,
    focus: Option<String>,
    contacts: usize,
    assistant: bool,
}

pub struct Thread {
    store: Option<MessagesStore>,
    watched: HashSet<Topic>,
    inputs: Inputs,
    list_key: Option<String>,
    primary: Option<String>,
    chat: Option<Arc<Chat>>,
    rows: Option<RowList>,
    /// Rows other state (translations, highlight, reply thread) depends on changed.
    dirty: bool,
    empty: bool,
    opened_at: i64,
    message_count: usize,
    requested_older: bool,
    highlight: Option<String>,
    highlight_task: Option<Task<()>>,
    translations: HashMap<String, Translation>,
    pending_jump: Option<String>,
    jump_task: Option<Task<()>>,
    thread_for: Option<String>,
    reply_thread: Option<Entity<ReplyThread>>,
    lightbox: Option<Entity<Lightbox>>,
    assistant_loaded: bool,
    /// The rows moved; once the list has laid them out, see whether the top is near.
    check_older: bool,
    _activation: Subscription,
}

impl Thread {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        for topic in [Topic::Selection, Topic::Connection, Topic::Focus, Topic::Contacts, Topic::ChatList] {
            { let entity = cx.entity().downgrade().into(); Bridge::watch(cx, topic, entity); }
        }
        cx.set_global(ThreadHost(cx.entity().downgrade()));
        // A GIF's clock stops while the window is inactive; coming back repaints it.
        let activation = cx.observe_window_activation(window, |_, _, cx| cx.notify());
        Thread {
            store: None,
            watched: HashSet::new(),
            inputs: Inputs::default(),
            list_key: None,
            primary: None,
            chat: None,
            rows: None,
            dirty: true,
            empty: false,
            opened_at: now_ms(),
            message_count: 0,
            requested_older: false,
            highlight: None,
            highlight_task: None,
            translations: HashMap::new(),
            pending_jump: None,
            jump_task: None,
            thread_for: None,
            reply_thread: None,
            lightbox: None,
            assistant_loaded: false,
            check_older: false,
            _activation: activation,
        }
    }

    fn watch(&mut self, topic: Topic, cx: &mut Context<Self>) {
        if self.watched.insert(topic.clone()) {
            { let entity = cx.entity().downgrade().into(); Bridge::watch(cx, topic, entity); }
        }
    }

    fn load_assistant(&mut self, store: &MessagesStore, cx: &mut Context<Self>) {
        self.assistant_loaded = true;
        run_for(store, messages_core::config::load_config(), cx, |this, config, cx| {
            let Some(llm) = config.canaryllm.filter(|llm| !llm.api_key.is_empty()) else { return };
            let client = CanaryLlmClient::new(llm.api_key, llm.model, None, reqwest::Client::new());
            cx.set_global(AssistantSlot(Some(Assistant { client: Arc::new(client), language: llm.language.unwrap_or_else(|| "English".to_owned()) })));
            this.dirty = true;
            cx.notify();
        });
    }

    /// Reads what the rows depend on and rebuilds them when any of it moved.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else { return };
        let assistant = assistant(cx).is_some();
        let (inputs, build) = {
            let state = store.state();
            let primary = state.selected_chat.clone();
            let Some(guid) = primary.clone() else {
                drop(state);
                if self.primary.is_some() || self.dirty {
                    self.primary = None;
                    self.chat = None;
                    self.inputs = Inputs::default();
                    if let Some(rows) = &self.rows {
                        rows.apply(Vec::new(), false, None, true, cx);
                    }
                }
                self.dirty = false;
                return;
            };
            let chat = state.chat(&guid).cloned();
            let mut pointers = Vec::new();
            for member in messages_core::conversations::conversation_members(&state.grouping, &guid) {
                pointers.push(usize::MAX);
                if let Some(list) = state.messages.get(&member) {
                    pointers.extend(list.iter().map(|message| Arc::as_ptr(message) as usize));
                }
            }
            let focus = (conversation_focus(&state, &guid) == FocusStatus::Silenced).then(|| conversation_handles(&state, &guid).first().map(|handle| handle_name(handle).to_owned())).flatten();
            let inputs = Inputs {
                primary: primary.clone(),
                chat: chat.as_ref().map(|chat| Arc::as_ptr(chat) as usize).unwrap_or(0),
                messages: pointers,
                typing: conversation_typing(&state, &guid),
                loading: conversation_loading(&state, &guid),
                online: state.status == ConnectionStatus::Online,
                capabilities: state.capabilities,
                focus,
                contacts: Arc::as_ptr(&state.contacts) as usize,
                assistant,
            };
            if inputs == self.inputs && !self.dirty {
                return;
            }
            let messages = conversation_messages(&state, &guid);
            let key = conversation_key(&state.grouping, &guid);
            let has_older = conversation_has_older(&state, &guid);
            (inputs, (guid, chat, messages, key, has_older))
        };
        let (guid, chat, messages, key, _has_older) = build;
        self.watch(Topic::Thread(guid.clone()), cx);
        self.watch(Topic::Typing(guid.clone()), cx);
        self.watch(Topic::Chat(guid.clone()), cx);

        let reset = self.list_key.as_deref() != Some(key.as_str());
        if reset {
            self.list_key = Some(key);
            self.opened_at = now_ms();
            self.requested_older = false;
            self.thread_for = None;
            self.reply_thread = None;
            self.highlight = None;
            self.highlight_task = None;
            self.pending_jump = None;
            self.jump_task = None;
            self.translations.clear();
        }
        if messages.len() != self.message_count {
            self.message_count = messages.len();
            self.requested_older = false;
        }
        let is_group = chat.as_ref().is_some_and(|chat| chat.is_group);
        let options = BuildOptions {
            is_group,
            typing: inputs.typing,
            loading: inputs.loading && !messages.is_empty(),
            online: inputs.online,
            silenced_by: inputs.focus.as_deref(),
            now: now_ms(),
        };
        let built = build_rows(&messages, &options);
        let slots = self.slots_for(&built, &guid, &messages, &inputs, is_group, true);
        let typing_who = chat.as_ref().and_then(|chat| chat.participants.first().cloned());
        self.empty = messages.is_empty() && !inputs.loading;
        if let Some(rows) = &self.rows {
            rows.apply(slots, is_group, typing_who, reset, cx);
        }
        self.primary = Some(guid.clone());
        self.chat = chat;
        self.inputs = inputs;
        self.dirty = false;
        self.check_older = true;

        if let Some(root) = self.thread_for.clone() {
            self.refresh_reply_thread(&root, &messages, is_group, cx);
        }
        if let Some(target) = self.pending_jump.clone() {
            if self.scroll_to(&target, cx) {
                self.pending_jump = None;
                self.jump_task = None;
            }
        }
    }

    fn slots_for(&self, rows: &[Row], primary: &str, messages: &[Arc<Message>], inputs: &Inputs, is_group: bool, main: bool) -> Vec<Slot> {
        let by_guid: HashMap<&str, &Arc<Message>> = messages.iter().map(|message| (message.guid.as_str(), message)).collect();
        let mut reply_counts: HashMap<&str, usize> = HashMap::new();
        if main {
            for message in messages {
                if let Some(target) = message.reply_to.as_deref() {
                    *reply_counts.entry(target).or_default() += 1;
                }
            }
        }
        rows.iter()
            .map(|row| match row {
                Row::Separator { key, label } => Slot::Caption { key: key.clone(), text: label.clone().into(), top: spacing::X4, bottom: spacing::X2 },
                Row::Event { key, text } => Slot::Caption { key: key.clone(), text: text.clone().into(), top: spacing::X3, bottom: spacing::X1 },
                Row::Loading => Slot::Caption { key: "loading".into(), text: "Loading earlier messages\u{2026}".into(), top: spacing::X3, bottom: spacing::X1 },
                Row::Typing => Slot::Typing,
                Row::Message(data) => {
                    let message = &data.message;
                    let quote = message.reply_to.as_deref().filter(|_| data.show_quote).map(|target| {
                        let original = by_guid.get(target);
                        let who: SharedString = match original {
                            Some(original) if original.from_me => "You".into(),
                            Some(original) => original.sender.as_ref().map(|sender| handle_name(sender).to_owned()).unwrap_or_default().into(),
                            None => "Earlier message".into(),
                        };
                        let body: SharedString = match original {
                            Some(original) if !original.text.is_empty() => original.text.clone().into(),
                            Some(original) if !original.attachments.is_empty() => "Attachment".into(),
                            Some(_) => "".into(),
                            None => "Click to load it".into(),
                        };
                        Quote { target: target.to_owned(), who, body }
                    });
                    Slot::Message {
                        entering: main && message.date > self.opened_at - ENTER_SLACK_MS,
                        props: RowProps {
                            primary: primary.to_owned(),
                            is_group,
                            capabilities: inputs.capabilities,
                            assistant: inputs.assistant,
                            quote,
                            reply_count: reply_counts.get(message.guid.as_str()).copied().unwrap_or(0),
                            replies: match reply_counts.get(message.guid.as_str()).copied().unwrap_or(0) {
                                0 => None,
                                1 => Some("1 reply".into()),
                                count => Some(format!("{count} replies").into()),
                            },
                            highlighted: main && self.highlight.as_deref() == Some(message.guid.as_str()),
                            translation: self.translations.get(&message.guid).cloned(),
                        },
                        data: data.clone(),
                    }
                }
            })
            .collect()
    }

    fn refresh_reply_thread(&mut self, root: &str, messages: &[Arc<Message>], is_group: bool, cx: &mut Context<Self>) {
        let Some(view) = self.reply_thread.clone() else { return };
        let primary = self.primary.clone().unwrap_or_default();
        let mut subset: Vec<Arc<Message>> = messages.iter().filter(|message| message.guid == root).cloned().collect();
        let replies = messages.iter().filter(|message| message.reply_to.as_deref() == Some(root)).count();
        subset.extend(messages.iter().filter(|message| message.reply_to.as_deref() == Some(root)).cloned());
        let options = BuildOptions { is_group, typing: false, loading: false, online: true, silenced_by: None, now: now_ms() };
        let rows: Vec<Row> = build_rows(&subset, &options)
            .into_iter()
            .filter_map(|row| match row {
                Row::Message(mut data) => {
                    data.position = crate::thread_rows::Position::Single;
                    data.show_sender = is_group && !data.message.from_me;
                    data.receipt = None;
                    data.notify = false;
                    data.show_quote = false;
                    Some(Row::Message(data))
                }
                Row::Separator { .. } => Some(row),
                _ => None,
            })
            .collect();
        let slots = self.slots_for(&rows, &primary, messages, &self.inputs, is_group, false);
        view.update(cx, |view, cx| view.set(slots, replies, is_group, cx));
    }

    fn scroll_to(&mut self, guid: &str, cx: &mut Context<Self>) -> bool {
        let Some(rows) = &self.rows else { return false };
        let Some(index) = rows.index_of(guid) else { return false };
        rows.state.scroll_to(ListOffset { item_ix: index, offset_in_item: px(0.) });
        rows.state.scroll_by(JUMP_OFFSET);
        self.highlight = Some(guid.to_owned());
        self.dirty = true;
        let target = guid.to_owned();
        self.highlight_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HIGHLIGHT_FOR).await;
            let _ = this.update(cx, |this, cx| {
                if this.highlight.as_deref() == Some(target.as_str()) {
                    this.highlight = None;
                    this.dirty = true;
                    cx.notify();
                }
            });
        }));
        cx.notify();
        true
    }

    /// Scrolls to a quoted message, paging older history (up to twelve pages) until it is in memory.
    pub fn jump_to(&mut self, guid: &str, cx: &mut Context<Self>) {
        if self.thread_for.take().is_some() {
            self.reply_thread = None;
            self.dirty = true;
        }
        if self.scroll_to(guid, cx) {
            return;
        }
        let (Some(store), Some(primary)) = (self.store.clone(), self.primary.clone()) else { return };
        self.pending_jump = Some(guid.to_owned());
        let target = guid.to_owned();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let pager = store.clone();
        store.spawn(async move {
            for _ in 0..JUMP_PAGES {
                let (found, older) = {
                    let state = pager.state();
                    (state.find_message(&primary, &target).is_some(), conversation_has_older(&state, &primary))
                };
                if found || !older {
                    break;
                }
                pager.load_earlier(&primary).await;
            }
            let _ = tx.send(());
        });
        let target = guid.to_owned();
        self.jump_task = Some(cx.spawn(async move |this, cx| {
            let _ = rx.await;
            let _ = this.update(cx, |this, cx| {
                if this.pending_jump.as_deref() == Some(target.as_str()) {
                    this.dirty = true;
                    cx.notify();
                }
            });
        }));
    }

    pub fn open_reply_thread(&mut self, guid: &str, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else { return };
        self.thread_for = Some(guid.to_owned());
        let thread = cx.entity().downgrade();
        self.reply_thread = Some(cx.new(|_| ReplyThread::new(store, thread)));
        self.dirty = true;
        cx.notify();
    }

    pub fn close_reply_thread(&mut self, cx: &mut Context<Self>) {
        self.thread_for = None;
        self.reply_thread = None;
        self.dirty = true;
        cx.notify();
    }

    pub fn toggle_translation(&mut self, guid: &str, text: &str, cx: &mut Context<Self>) {
        self.dirty = true;
        cx.notify();
        if self.translations.remove(guid).is_some() {
            return;
        }
        let (Some(store), Some(assistant)) = (self.store.clone(), assistant(cx)) else { return };
        self.translations.insert(guid.to_owned(), Translation::Loading);
        let key = guid.to_owned();
        let text = text.to_owned();
        run_for(&store, async move { assistant.client.translate(&text, &assistant.language).await }, cx, move |this, result, cx| {
            // A translation hidden while it was loading stays hidden.
            if let Some(slot) = this.translations.get_mut(&key) {
                *slot = match result {
                    Ok(text) => Translation::Text(text.into()),
                    Err(error) => Translation::Error(error.0.into()),
                };
                this.dirty = true;
                cx.notify();
            }
        });
    }

    pub(crate) fn open_lightbox(&mut self, chat_guid: &str, attachment_guid: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else { return };
        let (chat, attachment) = (chat_guid.to_owned(), attachment_guid.to_owned());
        self.lightbox = Some(cx.new(|cx| Lightbox::open(store, chat, attachment, window, cx)));
        cx.notify();
    }

    pub(crate) fn close_lightbox(&mut self, cx: &mut Context<Self>) {
        self.lightbox = None;
        cx.notify();
    }

    fn load_older_if_near_top(&mut self, cx: &mut Context<Self>) {
        if self.requested_older || self.empty || self.thread_for.is_some() {
            return;
        }
        let (Some(store), Some(primary)) = (self.store.clone(), self.primary.clone()) else { return };
        let Some(rows) = &self.rows else { return };
        if rows.state.item_count() == 0 || rows.state.logical_scroll_top().item_ix > OLDER_WITHIN_ROWS {
            return;
        }
        {
            let state = store.state();
            if !conversation_has_older(&state, &primary) || conversation_loading(&state, &primary) {
                return;
            }
        }
        self.requested_older = true;
        let pager = store.clone();
        store.spawn(async move { pager.load_earlier(&primary).await });
        let _ = cx;
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = Theme::get(cx);
        let chat = self.chat.clone();
        let label = if chat.as_ref().is_some_and(|chat| chat.service != Service::IMessage) { "Text message" } else { "iMessage" };
        div()
            .flex_grow(1.)
            .min_h(px(0.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(spacing::X3)
            .px(spacing::X6)
            .child(crate::primitives::avatar(None, chat.as_deref(), px(64.), cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(spacing::X1)
                    .child(div().text_size(type_scale::TITLE.font_size).line_height(type_scale::TITLE.line_height).font_weight(FontWeight::SEMIBOLD).text_color(palette.text).child(label))
                    .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).child("Send the first message below.")),
            )
            .into_any_element()
    }
}

impl Render for Thread {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.store.is_none() {
            if let Some(store) = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone()) {
                self.rows = Some(RowList::new(store.clone(), cx.entity().downgrade(), ListAlignment::Bottom));
                self.store = Some(store);
                self.dirty = true;
            }
        }
        if let Some(store) = self.store.clone() {
            if !self.assistant_loaded {
                self.load_assistant(&store, cx);
            }
        }
        self.refresh(cx);
        if let Some((chat, message)) = cx.try_global::<PendingJump>().and_then(|jump| jump.0.clone()) {
            if self.primary.as_deref() == Some(chat.as_str()) {
                cx.set_global(PendingJump(None));
                self.jump_to(&message, cx);
            }
        }
        self.load_older_if_near_top(cx);
        if std::mem::take(&mut self.check_older) {
            cx.on_next_frame(window, |this, _, cx| this.load_older_if_near_top(cx));
        }

        let lightbox = self.lightbox.clone().map(|lightbox| deferred(anchored().position_mode(AnchoredPositionMode::Window).position(point(px(0.), px(0.))).child(lightbox)).with_priority(5));
        let body = if self.primary.is_none() {
            div().flex_grow(1.).into_any_element()
        } else if let Some(view) = self.reply_thread.clone() {
            view.into_any_element()
        } else if self.empty {
            self.render_empty(cx)
        } else if let Some(rows) = &self.rows {
            rows.element().flex_grow(1.).w_full().pb(spacing::X2).into_any_element()
        } else {
            div().flex_grow(1.).into_any_element()
        };
        div().id("thread").flex_grow(1.).min_h(px(0.)).w_full().flex().flex_col().child(body).children(lightbox)
    }
}
