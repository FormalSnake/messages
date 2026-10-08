//! The public API of `MessagesStore`.
//!
//! Threading: the store lives on the tokio runtime the desktop app creates at
//! startup. Every network call, timer, cache write and JSON parse runs there.
//! The GPUI foreground thread only takes the read lock for the length of a
//! render and calls the synchronous methods (`set_draft`, `set_replying_to`,
//! ...), which mutate in memory and hand any I/O to `spawn`.
//!
//! Change notification: every mutation records the `StoreEvent`s it caused and
//! sends them on a broadcast channel after the write lock is released. A batch
//! (a page of messages, a reconcile pass) coalesces duplicates and sends once.
//! A receiver that lags gets `RecvError::Lagged` and must treat it as "everything
//! changed". tokio's sync channels are executor-agnostic, so a GPUI foreground
//! task can await `recv()` directly.

use std::cmp::Ordering as CmpOrdering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use parking_lot::{Mutex, RwLock, RwLockReadGuard};
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, broadcast, mpsc, oneshot};
use tokio::task::AbortHandle;

use crate::agent::{AgentConfig, AgentError, ChatPrefs, MacAgentClient, SharedPrefs, is_pinned};
use crate::cache::{StateCache, snapshot_for_cache};
use crate::conversations::{
    Grouping, contact_owners, conversation_chats, conversation_guid, conversation_handles, conversation_has_older, conversation_members, conversation_messages,
    focus_key, group_chats_with,
};
use crate::findmy::{FriendLocation, normalize_address};
use crate::gifs::{Gif, GifFavorite, merge_gif_favorites};
use crate::model::{
    Attachment, Capabilities, Chat, Contact, FocusStatus, Handle, Message, Millis, ScheduledMessage, Service, ServerInfo, Tapback, TapbackKind,
    capabilities_for, handle_name,
};
use crate::transport::{
    AttachmentPathOptions, ConnectionStatus, EditOptions, FaceTimeStatus, ListChatsOptions, LoadMessagesOptions, ReactOptions, SearchFilters,
    SendAttachmentOptions, SendTextOptions, Transport, TransportError, TransportEvent, TransportKind,
};


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceTimeCallStatus {
    Incoming,
    Answering,
    Ready,
    Failed,
    Ended,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FaceTimeCall {
    pub call_uuid: String,
    /// Display name of the caller, already resolved.
    pub from: Option<String>,
    pub status: FaceTimeCallStatus,
    pub can_answer: bool,
    pub link: Option<String>,
    pub error: Option<String>,
}

/// `Off` when no Mac agent is configured; `Unavailable` once a fetch has failed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FindMyState {
    #[default]
    Off,
    Unavailable,
    Ok,
}

/// Every field the UI needs to render. Chats and messages are `Arc` so a view can
/// keep what it rendered and compare by pointer: an unchanged row keeps its Arc.
#[derive(Clone, Debug)]
pub struct AppState {
    pub status: ConnectionStatus,
    /// Why the last connection attempt failed while `status` is not Online; while
    /// Online, that the last few reconcile passes failed, cleared by the next that completes.
    pub connection_error: Option<String>,
    /// A failed action worth a toast. Connection trouble stays in `connection_error`.
    pub error: Option<String>,
    pub server: Option<ServerInfo>,
    pub capabilities: Capabilities,
    /// Pinned first, then by `last_activity` descending.
    pub chats: Vec<Arc<Chat>>,
    /// Visible rows per chat guid, ascending by date. Reactions are folded into `tapbacks`.
    pub messages: HashMap<String, Vec<Arc<Message>>>,
    /// Absent means the chat was never paged, which `select_chat` and `warm_threads` rely on.
    pub has_older: HashMap<String, bool>,
    pub loading: HashSet<String>,
    pub typing: HashSet<String>,
    pub selected_chat: Option<String>,
    pub drafts: HashMap<String, String>,
    pub contacts: Arc<Vec<Contact>>,
    pub scheduled: Vec<ScheduledMessage>,
    /// Guid of the message being replied to in the composer, per chat.
    pub replying_to: HashMap<String, String>,
    /// Guid of the message being edited in the composer, per chat.
    pub editing: HashMap<String, String>,
    pub facetime: Option<FaceTimeCall>,
    pub last_sync_at: Millis,
    /// Find My friend locations, keyed by `normalize_address`.
    pub locations: HashMap<String, FriendLocation>,
    /// When one of those locations last changed, not when they were last read.
    pub locations_updated_at: Millis,
    pub find_my: FindMyState,
    /// Conversation currently paging its history for an export.
    pub exporting_chat: Option<String>,
    /// Keyed by gif id, synced through the Mac agent like chat prefs.
    pub gif_favorites: HashMap<String, GifFavorite>,
    /// Who is on a Focus, keyed by `focus_key`.
    pub focus: HashMap<String, FocusStatus>,
    /// Recomputed whenever `chats` or `contacts` change.
    pub grouping: Grouping,
}


impl AppState {
    pub fn new(gif_favorites: HashMap<String, GifFavorite>) -> Self {
        Self {
            status: ConnectionStatus::Connecting,
            connection_error: None,
            error: None,
            server: None,
            capabilities: capabilities_for(None),
            chats: Vec::new(),
            messages: HashMap::new(),
            has_older: HashMap::new(),
            loading: HashSet::new(),
            typing: HashSet::new(),
            selected_chat: None,
            drafts: HashMap::new(),
            contacts: Arc::new(Vec::new()),
            scheduled: Vec::new(),
            replying_to: HashMap::new(),
            editing: HashMap::new(),
            facetime: None,
            last_sync_at: now_ms(),
            locations: HashMap::new(),
            locations_updated_at: 0,
            find_my: FindMyState::Off,
            exporting_chat: None,
            gif_favorites,
            focus: HashMap::new(),
            grouping: Grouping::default(),
        }
    }

    /// The chat row with this exact guid (not resolved to its primary).
    pub fn chat(&self, guid: &str) -> Option<&Arc<Chat>> {
        self.chats.iter().find(|chat| chat.guid == guid)
    }

    /// Looks through every member of the conversation, so a guid from the merged thread resolves.
    pub fn find_message(&self, chat_guid: &str, message_guid: &str) -> Option<&Arc<Message>> {
        for member in conversation_members(&self.grouping, chat_guid) {
            if let Some(found) = self.messages.get(&member).and_then(|list| list.iter().find(|message| message.guid == message_guid)) {
                return Some(found);
            }
        }
        None
    }
}

/// An incoming message or tapback worth a desktop notification. Only sent for unmuted chats.
#[derive(Clone, Debug, PartialEq)]
pub struct Incoming {
    pub chat: Arc<Chat>,
    pub message: Arc<Message>,
    /// The message a tapback landed on, when it is loaded.
    pub target: Option<Arc<Message>>,
}

/// What changed. Chat guids are the raw member guid; the UI maps them to the
/// conversation it shows with `conversation_guid`.
#[derive(Clone, Debug, PartialEq)]
pub enum StoreEvent {
    /// `status`, `connection_error`, `server` or `capabilities`.
    Connection,
    /// Membership or order of `chats`, or `grouping`: the sidebar re-splices its list.
    ChatList,
    /// One chat row changed in place: unread, pinned, muted, read receipts, last message, name, participants, icon.
    Chat(String),
    /// Rows of this chat were added, removed or reordered, or `has_older` / `loading` changed.
    Thread(String),
    /// One message changed in place without moving: tapbacks, local path, measured size, receipts, edit, unsend, error.
    Message { chat_guid: String, message_guid: String },
    Typing(String),
    Selection,
    Draft(String),
    /// `replying_to` or `editing` for this chat.
    ComposerMode(String),
    Contacts,
    Scheduled,
    FaceTime,
    Locations,
    Focus,
    /// `error` set or cleared.
    Error,
    Exporting,
    GifFavorites,
    Incoming(Incoming),
}

impl StoreEvent {
    /// Whether the change reaches what `snapshot_for_cache` writes.
    fn cacheable(&self) -> bool {
        matches!(
            self,
            StoreEvent::ChatList | StoreEvent::Chat(_) | StoreEvent::Thread(_) | StoreEvent::Message { .. } | StoreEvent::Selection | StoreEvent::Contacts
        )
    }
}

pub type PrefsCallback = Arc<dyn Fn(&HashMap<String, ChatPrefs>) + Send + Sync>;
pub type GifFavoritesCallback = Arc<dyn Fn(&HashMap<String, GifFavorite>) + Send + Sync>;

#[derive(Clone, Default)]
pub struct StoreOptions {
    pub prefs: HashMap<String, ChatPrefs>,
    /// Called on the runtime thread; the desktop app writes `chats` into config.json.
    pub on_prefs_change: Option<PrefsCallback>,
    pub gif_favorites: HashMap<String, GifFavorite>,
    pub on_gif_favorites_change: Option<GifFavoritesCallback>,
    /// Default 50.
    pub page_size: Option<u32>,
    /// Default 30 000. 0 turns the reconcile timer and the Focus poll off.
    pub reconcile_every_ms: Option<u64>,
    /// How many recent chats the background pass keeps paged. Default 40; 0 turns warming off.
    pub warm_chats: Option<usize>,
    /// Omit to leave Find My and prefs sync off.
    pub agent: Option<AgentConfig>,
    /// Last known state, painted before the server answers and kept current afterwards.
    pub cache: Option<Arc<StateCache>>,
}

/// Cheap to clone; every clone is the same store.
#[derive(Clone)]
pub struct MessagesStore {
    inner: Arc<Inner>,
}

struct Inner {
    transport: Arc<dyn Transport>,
    runtime: tokio::runtime::Handle,
    state: RwLock<AppState>,
    events: broadcast::Sender<StoreEvent>,
    options: StoreOptions,
    agent: Option<MacAgentClient>,
    /// Internal state not exposed to views: prefs, pending reactions,
    /// typing timers, forced-unread set, draft sync timers, outbox, focus
    /// check times, Find My stream handle, batch depth. Never held across an await.
    ///
    /// Lock order: `state` may be held while taking `private` or `owners`,
    /// never the other way round.
    private: Mutex<Private>,
    /// Normalized address to contact id, rebuilt only when the contacts change,
    /// so regrouping after a message does not walk the whole address book.
    owners: RwLock<Arc<HashMap<String, String>>>,
    stopped: AtomicBool,
    /// Cuts every `pause` short: `stop` and a reconnect.
    wake: Notify,
    me: Weak<Inner>,
}

/// Time between reconnects to the Find My stream starts at one second and doubles to this.
/// How long after the last click, scroll or keystroke in a thread it still counts as being read.
const ENGAGED_FOR: Duration = Duration::from_secs(60);

const LOCATIONS_STREAM_RETRY_MAX_MS: u64 = 30_000;
/// The fallback behind the push stream: an agent that cannot stream, or one whose stream is down.
const LOCATIONS_POLL_MS: u64 = 60_000;
const LOCATIONS_MIN_INTERVAL_MS: Millis = 20_000;
/// How often the open conversation re-asks whether the other person is on a Focus.
const FOCUS_POLL_MS: u64 = 60_000;
/// An answer about the open conversation younger than this is reused rather than asked for again.
const FOCUS_TTL_MS: u64 = 45_000;
/// The background pass is only feeding the sidebar, so it settles for a much older answer.
const FOCUS_WARM_TTL_MS: u64 = 10 * 60_000;
const FOCUS_CHATS: usize = 15;
/// Each background Focus check is a round trip through the helper.
const FOCUS_GAP_MS: u64 = 150;
const PAGE: u32 = 50;
/// Rows kept for a conversation once it is not the open one, the same depth the cache keeps on disk.
const KEEP_MESSAGES: usize = 100;
/// The server is slow per message once attributedBody is requested, and one big request can hang it for minutes.
const SWEEP_PAGE: u32 = 10;
const WARM_CHATS: usize = 40;
/// Gap between background pages, so warming never crowds out a send or an open thread.
const WARM_GAP_MS: u64 = 400;
/// A thread thinner than this is topped up in the background; the sweep leaves a chat holding two rows.
const WARM_MESSAGES: usize = 20;
/// Fewer chats get their media than get paged: the files are the expensive part.
const WARM_MEDIA_CHATS: usize = 15;
const WARM_MEDIA_MESSAGES: usize = 12;
const WARM_MEDIA_GAP_MS: u64 = 150;
const TYPING_IDLE_MS: u64 = 3000;
/// Messages.app drops a typing bubble after about a minute if the other side never sends; so do we, in case the stop event is lost.
const TYPING_SHOWN_MAX_MS: u64 = 60_000;
const CONNECT_RETRY_MS: u64 = 2000;
const CONNECT_RETRY_MAX_MS: u64 = 30_000;
const SEND_ATTEMPTS: u32 = 4;
const SEND_RETRY_MS: u64 = 2000;
const DRAFT_SYNC_DEBOUNCE_MS: u64 = 2000;
const EXPORT_MAX_MESSAGES: usize = 2000;
const CHAT_PAGE: u32 = 200;
const CHAT_LIMIT: u32 = 5000;
/// Passes in a row that have to fail before the footer says so; one is usually a request the server dropped.
const RECONCILE_FAILURES: u32 = 3;
const RECONCILE_FAILED: &str = "Could not refresh from the Mac.";

/// Wall clock in epoch milliseconds. Tests read tokio's clock instead, so a
/// paused runtime moves `Date.now()` the way vitest's fake timers did.
#[cfg(not(test))]
pub(crate) fn now_ms() -> Millis {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
pub(crate) fn now_ms() -> Millis {
    use std::sync::OnceLock;
    static ANCHOR: OnceLock<(Millis, std::time::Instant)> = OnceLock::new();
    let (wall, base) = *ANCHOR.get_or_init(|| (chrono::Utc::now().timestamp_millis(), std::time::Instant::now()));
    let now = tokio::time::Instant::now().into_std();
    match now.checked_duration_since(base) {
        Some(ahead) => wall + ahead.as_millis() as Millis,
        None => wall - base.duration_since(now).as_millis() as Millis,
    }
}

/// The mime type a local file is sent with, from its extension.
pub(crate) fn mime_for_path(path: &Path) -> String {
    mime_guess::from_path(path).first().map(|mime| mime.essence_str().to_owned()).unwrap_or_else(|| crate::model::UNKNOWN_MIME.to_owned())
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_temp_guid() -> String {
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    format!("temp-{}-{n}", now_ms())
}

/// How big a file of this kind the background pass will pull, matching what the
/// thread downloads the moment it comes into view. Anything else (a PDF, a zip)
/// waits for a click there, so it waits here too.
fn warm_budget(mime: &str) -> u64 {
    if mime.starts_with("video/") {
        60 * 1024 * 1024
    } else if mime.starts_with("image/") || mime.starts_with("audio/") {
        25 * 1024 * 1024
    } else {
        0
    }
}

/// Pinned first, then by last activity, newest first.
fn chat_order(a: &Chat, b: &Chat) -> CmpOrdering {
    b.pinned.cmp(&a.pinned).then(b.last_activity.cmp(&a.last_activity))
}

fn sort_chats(chats: &mut [Arc<Chat>]) {
    chats.sort_by(|a, b| chat_order(a, b));
}

fn insert_sorted(list: &mut Vec<Arc<Message>>, message: Arc<Message>) -> usize {
    let index = list.partition_point(|item| item.date <= message.date);
    list.insert(index, message);
    index
}

fn push_event(events: &mut Vec<StoreEvent>, event: StoreEvent) {
    if !events.contains(&event) {
        events.push(event);
    }
}

/// The row's own copy of the last message is richer (a downloaded path, a temp
/// guid), so it is compared by identity of the message, not by shape.
/// A chat read here stays read while the newest message the server shows is
/// no newer than that read.
fn read_here_covers(private: &Private, chat: &Chat) -> bool {
    private.read_locally.get(&chat.guid).is_some_and(|read_at| chat.last_message.as_ref().is_none_or(|message| message.date <= *read_at))
}

fn same_chat(a: &Chat, b: &Chat) -> bool {
    let Chat {
        guid,
        identifier,
        group_id,
        service,
        is_group,
        display_name,
        icon,
        participants,
        pinned,
        muted,
        read_receipts,
        archived,
        unread,
        last_message,
        last_activity,
    } = a;
    guid == &b.guid
        && identifier == &b.identifier
        && group_id == &b.group_id
        && service == &b.service
        && is_group == &b.is_group
        && display_name == &b.display_name
        && icon == &b.icon
        && participants == &b.participants
        && pinned == &b.pinned
        && muted == &b.muted
        && read_receipts == &b.read_receipts
        && archived == &b.archived
        && unread == &b.unread
        && last_activity == &b.last_activity
        && last_message.as_ref().map(|m| (&m.guid, m.date)) == b.last_message.as_ref().map(|m| (&m.guid, m.date))
}

/// The server re-sends the pixel size chat.db holds, which ignores EXIF, so a
/// refresh of the open thread would flip a portrait photo back to a landscape
/// box and the row would visibly recrop. What the client read from the file
/// header wins, and so does a path it already has.
fn merge_attachments(existing: &[Attachment], incoming: &[Attachment]) -> Vec<Attachment> {
    incoming
        .iter()
        .map(|item| {
            let Some(previous) = existing.iter().find(|entry| entry.guid == item.guid) else {
                return item.clone();
            };
            let mut merged = item.clone();
            if merged.local_path.is_none() {
                merged.local_path = previous.local_path.clone();
            }
            if previous.measured {
                merged.width = previous.width;
                merged.height = previous.height;
                merged.measured = true;
            }
            merged
        })
        .collect()
}

/// An optimistic row and a server row describe the same send when text and attachment names agree.
fn same_send(mine: &Message, theirs: &Message) -> bool {
    mine.text == theirs.text
        && mine.attachments.len() == theirs.attachments.len()
        && mine.attachments.iter().zip(&theirs.attachments).all(|(a, b)| a.name == b.name)
}

/// One person holds one tapback per message. A reaction replaces the author's
/// earlier one, and a copy the server sends again replaces itself by guid,
/// keeping the sender the first copy carried when the new one has none. Two
/// senders the server left blank are never the same person.
fn merge_tapback(existing: &[Tapback], entry: Tapback, removed: bool) -> Vec<Tapback> {
    let prior = existing.iter().find(|item| item.guid == entry.guid);
    let tapback = match prior.and_then(|prior| prior.sender.clone()) {
        Some(sender) if entry.sender.is_none() => Tapback { sender: Some(sender), ..entry },
        _ => entry,
    };
    let same_author = |item: &Tapback| {
        if item.from_me || tapback.from_me {
            item.from_me && tapback.from_me
        } else {
            match (&item.sender, &tapback.sender) {
                (Some(a), Some(b)) => a.address == b.address,
                _ => false,
            }
        }
    };
    let same = |item: &Tapback| item.guid == tapback.guid || same_author(item);
    if removed {
        return existing.iter().filter(|item| !(same(item) && item.kind == tapback.kind && item.emoji == tapback.emoji)).cloned().collect();
    }
    let mut next: Vec<Tapback> = existing.iter().filter(|item| !same(item)).cloned().collect();
    next.push(tapback);
    next
}

#[derive(Clone)]
struct PendingReaction {
    guid: String,
    kind: TapbackKind,
    emoji: Option<String>,
    removed: bool,
    from_me: bool,
    sender: Option<Handle>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub(crate) enum Outgoing {
    Text { text: String, reply_to: Option<String>, effect: Option<String> },
    Attachment { path: PathBuf, name: String },
}

/// A send waiting its turn. Sends go out one at a time, in order, and wait for
/// the connection to come back. The queue is written to the state cache with
/// everything else, so a relaunch while offline picks the sends up again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OutboxItem {
    chat_guid: String,
    temp_guid: String,
    optimistic: Message,
    #[serde(skip)]
    attempts: u32,
    send: Outgoing,
}

/// What `apply_message` leaves to do once the write lock is released.
#[derive(Default)]
struct Followup {
    read_receipt: Option<String>,
    fetch_chat: Option<String>,
}

#[derive(Default)]
struct Private {
    prefs: HashMap<String, ChatPrefs>,
    pending_reactions: HashMap<String, Vec<PendingReaction>>,
    typing_timers: HashMap<String, AbortHandle>,
    typing_sent: HashSet<String>,
    /// Chats marked unread here: the Mac's mark-unread leaves `dateRead` alone, so a re-read of the list would clear the dot.
    forced_unread: HashSet<String>,
    /// Chats read here, with the date of the newest message that read
    /// covered. chat.db only records a read when the Mac sends the receipt, so
    /// without this the next pass over the list would put the dot back.
    read_locally: HashMap<String, Millis>,
    /// The conversation last clicked, scrolled or typed in, and when. Only it
    /// reads incoming messages, and only for `ENGAGED_FOR` after that, so a
    /// thread left open on an idle screen leaves the phone and watch to ring.
    engaged: Option<(String, tokio::time::Instant)>,
    typing_shown: HashMap<String, AbortHandle>,
    draft_sync_timers: HashMap<String, AbortHandle>,
    /// When the composer for a chat was last typed into, so a stale remote draft never overwrites newer local text.
    draft_edited_at: HashMap<String, Millis>,
    reconcile_timer: Option<AbortHandle>,
    reconciling: bool,
    /// Passes claimed so far, so `start` can tell the reconnect already began one.
    reconcile_passes: u64,
    /// Passes in a row that ended in an error; reset by the first that completes.
    reconcile_failures: u32,
    warming: bool,
    /// Attachments the background pass could not download this connection. The server answers the same way every 30 s, so they wait for a reconnect or a click.
    warm_failed: HashSet<String>,
    warming_focus: bool,
    locations_timer: Option<AbortHandle>,
    locations_stream: Option<AbortHandle>,
    last_locations_fetch_at: Millis,
    /// When a read (a poll answer or a push) last landed, so a slow poll cannot overwrite a newer push.
    last_locations_at: Millis,
    focus_timer: Option<AbortHandle>,
    /// Focus key to when it was last asked about, so a poll and the background pass do not ask twice.
    focus_checked_at: HashMap<String, Millis>,
    outbox: VecDeque<OutboxItem>,
    flushing: bool,
    event_loop: Option<AbortHandle>,
    /// Asks the event loop to apply everything the transport has already queued.
    barrier: Option<mpsc::UnboundedSender<oneshot::Sender<()>>>,
    batch_depth: usize,
    batched: Vec<StoreEvent>,
}

impl MessagesStore {
    /// Builds the store; nothing runs until `start`. `runtime` is where every task the store spawns goes.
    pub fn new(transport: Arc<dyn Transport>, options: StoreOptions, runtime: tokio::runtime::Handle) -> Self {
        let agent = options.agent.clone().map(|config| MacAgentClient::new(config, reqwest::Client::new()));
        let state = AppState::new(options.gif_favorites.clone());
        let private = Private { prefs: options.prefs.clone(), ..Private::default() };
        let (events, _) = broadcast::channel(1024);
        let inner = Arc::new_cyclic(|me| Inner {
            transport,
            runtime,
            state: RwLock::new(state),
            events,
            options,
            agent,
            private: Mutex::new(private),
            owners: RwLock::new(Arc::new(HashMap::new())),
            stopped: AtomicBool::new(false),
            wake: Notify::new(),
            me: me.clone(),
        });
        Self { inner }
    }

    /// Port of `subscribe`. Capacity is large enough that only a stalled UI lags.
    pub fn events(&self) -> broadcast::Receiver<StoreEvent> {
        self.inner.events.subscribe()
    }

    /// Port of `getSnapshot`. Hold the guard for a render at most, never across an await.
    pub fn state(&self) -> RwLockReadGuard<'_, AppState> {
        self.inner.state.read()
    }

    /// The Mac agent, when one is configured. Map snapshots go through it without touching the store.
    pub fn agent(&self) -> Option<&MacAgentClient> {
        self.inner.agent.as_ref()
    }

    pub fn transport(&self) -> &Arc<dyn Transport> {
        &self.inner.transport
    }

    /// Selects `chat_guid` and reads it: what a click in the sidebar, the
    /// switcher or a next/previous shortcut means. `select_chat` alone, which
    /// startup uses, opens without reading.
    pub async fn open_chat(&self, chat_guid: &str) {
        self.select_chat(Some(chat_guid)).await;
        self.engage(chat_guid);
    }

    /// The runtime every async method runs on, for work that must be aborted
    /// as a unit rather than fired and forgotten (video playback).
    pub fn runtime(&self) -> &tokio::runtime::Handle {
        &self.inner.runtime
    }

    /// Runs `future` on the store's runtime. The UI uses this for every async method.
    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.inner.runtime.spawn(future);
    }

    fn spawn_task<F>(&self, future: F) -> AbortHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.inner.runtime.spawn(future).abort_handle()
    }

    fn stopped(&self) -> bool {
        self.inner.stopped.load(Ordering::SeqCst)
    }

    fn online(&self) -> bool {
        self.inner.state.read().status == ConnectionStatus::Online
    }

    fn page_size(&self) -> u32 {
        self.inner.options.page_size.unwrap_or(PAGE)
    }

    /// A sleep that `stop()` and a reconnect can cut short.
    async fn pause(&self, ms: u64) {
        let woken = self.inner.wake.notified();
        tokio::pin!(woken);
        woken.as_mut().enable();
        if self.stopped() {
            return;
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(ms)) => {}
            _ = woken => {}
        }
    }

    /// Port of `start`: paints from the cache, connects with backoff, then reconciles or loads chats and starts the timers.
    pub async fn start(&self) {
        let cached = match &self.inner.options.cache {
            Some(cache) => cache.load().await,
            None => None,
        };
        let from_cache = cached.is_some_and(|cached| !cached.chats.is_empty() && {
            self.paint_cached(cached);
            true
        });
        self.spawn_event_loop();
        let passes = self.inner.private.lock().reconcile_passes;
        if self.connect_until_up().await.is_none() {
            return;
        }
        if from_cache && !self.inner.state.read().chats.is_empty() {
            // Painted from disk already; bring the list and the open thread up to date.
            self.inner.update(|state, events| {
                if state.status != ConnectionStatus::Online {
                    state.status = ConnectionStatus::Online;
                    push_event(events, StoreEvent::Connection);
                }
            });
            // The transition to online usually started a pass already; a second would re-read the open thread for nothing.
            if self.inner.private.lock().reconcile_passes == passes {
                self.reconcile().await;
            }
            let unpaged = {
                let state = self.inner.state.read();
                state.selected_chat.clone().filter(|selected| !state.has_older.contains_key(selected))
            };
            if let Some(selected) = unpaged {
                self.load_older(&selected, false).await;
            }
        } else {
            if let Err(error) = self.refresh_chats().await {
                tracing::warn!("chats: {error}");
            }
            let first = {
                let state = self.inner.state.read();
                if state.selected_chat.is_none() { state.chats.first().map(|chat| chat.guid.clone()) } else { None }
            };
            if let Some(first) = first {
                self.select_chat(Some(&first)).await;
            }
        }
        if self.inner.state.read().capabilities.scheduled_messages {
            self.refresh_scheduled().await;
        }
        let every = self.inner.options.reconcile_every_ms.unwrap_or(30_000);
        if every > 0 {
            let store = self.clone();
            let reconcile = self.spawn_task(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(every)).await;
                    store.reconcile().await;
                }
            });
            let store = self.clone();
            let focus = self.spawn_task(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(FOCUS_POLL_MS)).await;
                    let selected = store.inner.state.read().selected_chat.clone();
                    store.refresh_focus(selected.as_deref(), None).await;
                }
            });
            let mut private = self.inner.private.lock();
            private.reconcile_timer = Some(reconcile);
            private.focus_timer = Some(focus);
        }
        let store = self.clone();
        self.spawn(async move { store.sync_prefs().await });
        let store = self.clone();
        self.spawn(async move { store.refresh_locations().await });
        let store = self.clone();
        self.spawn(async move { store.warm_threads().await });
        let store = self.clone();
        self.spawn(async move {
            let selected = store.inner.state.read().selected_chat.clone();
            store.refresh_focus(selected.as_deref(), None).await;
        });
    }

    fn paint_cached(&self, mut cached: crate::cache::CachedState) {
        let contacts = cached.contacts.clone();
        let outbox = std::mem::take(&mut cached.outbox);
        self.inner.private.lock().read_locally = std::mem::take(&mut cached.read_at);
        *self.inner.owners.write() = Arc::new(contact_owners(&contacts));
        self.inner.update(|state, events| {
            let chats: Vec<Arc<Chat>> = cached.chats.iter().map(|chat| self.inner.with_prefs_arc(chat)).collect();
            let selected = cached
                .selected_chat
                .filter(|selected| cached.chats.iter().any(|chat| &chat.guid == selected))
                .or_else(|| cached.chats.first().map(|chat| chat.guid.clone()));
            state.has_older = cached.messages.keys().map(|guid| (guid.clone(), true)).collect();
            for guid in cached.messages.keys() {
                push_event(events, StoreEvent::Thread(guid.clone()));
            }
            state.messages = cached.messages;
            state.contacts = cached.contacts;
            state.last_sync_at = cached.saved_at;
            state.selected_chat = selected;
            push_event(events, StoreEvent::Contacts);
            push_event(events, StoreEvent::Selection);
            self.inner.set_chats(state, events, chats);
        });
        self.inner.transport.seed_contacts(&contacts);
        // Sends that never left: back on their rows and back in the queue, to go
        // out once the connection is up.
        for item in outbox {
            self.apply_message(item.optimistic.clone(), false, false);
            self.inner.private.lock().outbox.push_back(item);
        }
    }

    fn spawn_event_loop(&self) {
        let mut rx = self.inner.transport.subscribe();
        let (barrier_tx, mut barrier_rx) = mpsc::unbounded_channel::<oneshot::Sender<()>>();
        let store = self.clone();
        let task = self.spawn_task(async move {
            loop {
                tokio::select! {
                    biased;
                    event = rx.recv() => match event {
                        Some(event) => store.handle(event),
                        None => {
                            while let Some(ack) = barrier_rx.recv().await {
                                let _ = ack.send(());
                            }
                            return;
                        }
                    },
                    ack = barrier_rx.recv() => {
                        let Some(ack) = ack else { return };
                        while let Ok(event) = rx.try_recv() {
                            store.handle(event);
                        }
                        let _ = ack.send(());
                    }
                }
            }
        });
        let mut private = self.inner.private.lock();
        if let Some(previous) = private.event_loop.replace(task) {
            previous.abort();
        }
        private.barrier = Some(barrier_tx);
    }

    /// Applies every transport event queued so far. A socket event the transport
    /// sent before `connect` returned has landed once this resolves.
    async fn sync_events(&self) {
        let barrier = self.inner.private.lock().barrier.clone();
        let Some(barrier) = barrier else { return };
        let (tx, rx) = oneshot::channel();
        if barrier.send(tx).is_ok() {
            let _ = rx.await;
        }
    }

    /// Port of `stop`: cancels every timer and stream, sends owed typing stops, disconnects, flushes the cache.
    pub async fn stop(&self) {
        self.inner.stopped.store(true, Ordering::SeqCst);
        self.inner.wake.notify_waiters();
        let owed: Vec<String> = {
            let mut guard = self.inner.private.lock();
            let private = &mut *guard;
            let handles = [
                private.event_loop.take(),
                private.reconcile_timer.take(),
                private.locations_timer.take(),
                private.locations_stream.take(),
                private.focus_timer.take(),
            ];
            for handle in handles.into_iter().flatten() {
                handle.abort();
            }
            private.barrier = None;
            for (_, handle) in private.typing_timers.drain().chain(private.typing_shown.drain()).chain(private.draft_sync_timers.drain()) {
                handle.abort();
            }
            private.typing_sent.drain().collect()
        };
        let transport = self.inner.transport.clone();
        futures_util::future::join_all(owed.iter().map(|chat_guid| transport.set_typing(chat_guid, false))).await;
        transport.disconnect().await;
        if let Some(cache) = &self.inner.options.cache {
            cache.flush().await;
        }
    }

    /// Keeps trying with backoff until the server answers. Only `stop()` gives up.
    async fn connect_until_up(&self) -> Option<ServerInfo> {
        let mut delay = CONNECT_RETRY_MS;
        while !self.stopped() {
            match self.inner.transport.connect().await {
                Ok(info) => {
                    self.sync_events().await;
                    self.inner.update(|state, events| {
                        state.capabilities = capabilities_for(Some(&info));
                        state.server = Some(info.clone());
                        state.connection_error = None;
                        push_event(events, StoreEvent::Connection);
                    });
                    return Some(info);
                }
                Err(error) => {
                    self.sync_events().await;
                    self.inner.update(|state, events| {
                        state.status = ConnectionStatus::Offline;
                        state.connection_error = Some(error.to_string());
                        push_event(events, StoreEvent::Connection);
                    });
                    self.pause(delay).await;
                    delay = (delay * 2).min(CONNECT_RETRY_MAX_MS);
                }
            }
        }
        None
    }

    /// Port of `setDetailsOpen`: the details panel drives the Find My stream and poll.
    pub fn set_details_open(&self, open: bool) {
        if !open {
            let mut private = self.inner.private.lock();
            for handle in [private.locations_timer.take(), private.locations_stream.take()].into_iter().flatten() {
                handle.abort();
            }
            return;
        }
        if self.inner.transport.kind() == TransportKind::Demo {
            self.apply_friends(&crate::demo::friends());
            return;
        }
        if self.inner.agent.is_none() || self.inner.private.lock().locations_timer.is_some() {
            return;
        }
        let store = self.clone();
        let timer = self.spawn_task(async move {
            store.refresh_locations().await;
            loop {
                tokio::time::sleep(Duration::from_millis(LOCATIONS_POLL_MS)).await;
                store.refresh_locations().await;
            }
        });
        let store = self.clone();
        let stream = self.spawn_task(async move { store.stream_locations().await });
        let mut private = self.inner.private.lock();
        private.locations_timer = Some(timer);
        if let Some(previous) = private.locations_stream.replace(stream) {
            previous.abort();
        }
    }

    fn apply_friends(&self, friends: &[FriendLocation]) {
        let mut locations = HashMap::new();
        for friend in friends {
            for address in &friend.addresses {
                locations.insert(normalize_address(address), friend.clone());
            }
        }
        let now = now_ms();
        self.inner.private.lock().last_locations_at = now;
        self.inner.update(|state, events| {
            // A push arrives whenever any Find My cache changes, which is usually no news about anyone in a chat.
            if state.find_my == FindMyState::Ok && state.locations == locations {
                return;
            }
            state.locations = locations;
            state.locations_updated_at = now;
            state.find_my = FindMyState::Ok;
            push_event(events, StoreEvent::Locations);
        });
    }

    /// The agent pushes a snapshot as Find My writes one, so an open panel shows
    /// a move within a second or so instead of at the next poll. Reconnects with
    /// backoff for as long as the panel is open; an agent too old to serve the
    /// stream is left to the poll.
    async fn stream_locations(&self) {
        let Some(agent) = &self.inner.agent else { return };
        let mut delay = 1000;
        while !self.stopped() {
            let (tx, mut rx) = mpsc::channel(8);
            let stream = agent.stream_findmy(tx);
            tokio::pin!(stream);
            let result = loop {
                tokio::select! {
                    result = &mut stream => break result,
                    Some(snapshot) = rx.recv() => {
                        if let Some(friends) = &snapshot.friends {
                            self.apply_friends(friends);
                        }
                    }
                }
            };
            while let Ok(snapshot) = rx.try_recv() {
                if let Some(friends) = &snapshot.friends {
                    self.apply_friends(friends);
                }
            }
            match result {
                Ok(()) => delay = 1000,
                Err(AgentError::StreamUnsupported) => return,
                Err(error) => tracing::warn!("findmy: {error}"),
            }
            self.pause(delay).await;
            delay = (delay * 2).min(LOCATIONS_STREAM_RETRY_MAX_MS);
        }
    }

    /// Port of `refreshLocations`. At most once per 20 s; a push that landed meanwhile wins.
    pub async fn refresh_locations(&self) {
        let Some(agent) = &self.inner.agent else { return };
        let now = now_ms();
        {
            let mut private = self.inner.private.lock();
            if now - private.last_locations_fetch_at < LOCATIONS_MIN_INTERVAL_MS {
                return;
            }
            private.last_locations_fetch_at = now;
        }
        match agent.friends().await {
            Ok(response) => {
                // A push that landed while this request was in flight is the newer answer of the two.
                if self.inner.private.lock().last_locations_at >= now {
                    return;
                }
                self.apply_friends(&response.friends);
            }
            Err(error) => {
                // Quiet: a Mac with no agent running, or one that's asleep, is a normal state.
                tracing::warn!("findmy: {error}");
                self.inner.update(|state, events| {
                    if state.find_my != FindMyState::Unavailable {
                        state.find_my = FindMyState::Unavailable;
                        push_event(events, StoreEvent::Locations);
                    }
                });
            }
        }
    }

    /// Port of `refreshFocus`. Returns whether it asked the helper. `ttl_ms` defaults to 45 000.
    pub async fn refresh_focus(&self, guid: Option<&str>, ttl_ms: Option<u64>) -> bool {
        let Some(guid) = guid else { return false };
        let address = {
            let state = self.inner.state.read();
            if !state.capabilities.focus_status || state.status != ConnectionStatus::Online {
                return false;
            }
            match state.chat(conversation_guid(&state.grouping, guid)) {
                Some(chat) if !chat.is_group => {}
                _ => return false,
            }
            match conversation_handles(&state, guid).into_iter().next() {
                Some(handle) => handle.address,
                None => return false,
            }
        };
        let key = focus_key(&address);
        let now = now_ms();
        {
            let mut private = self.inner.private.lock();
            let checked = private.focus_checked_at.get(&key).copied().unwrap_or(0);
            if now - checked < ttl_ms.unwrap_or(FOCUS_TTL_MS) as Millis {
                return false;
            }
            private.focus_checked_at.insert(key.clone(), now);
        }
        match self.inner.transport.focus_status(&address).await {
            Ok(status) => self.inner.update(|state, events| {
                if state.focus.get(&key) != Some(&status) {
                    state.focus.insert(key, status);
                    push_event(events, StoreEvent::Focus);
                }
            }),
            // A failure waits out the TTL like an answer does: retrying every pass
            // floods a helper that is already failing, and every call queues behind it.
            Err(error) => tracing::debug!("focus: {error}"),
        }
        true
    }

    /// Keeps an answer for the most recent one-to-one chats so the sidebar can
    /// show the moon without each thread being opened first.
    async fn warm_focus(&self) {
        let capable = self.inner.state.read().capabilities.focus_status;
        {
            let mut private = self.inner.private.lock();
            if private.warming_focus || !capable {
                return;
            }
            private.warming_focus = true;
        }
        let chats: Vec<String> = {
            let state = self.inner.state.read();
            conversation_chats(&state).into_iter().filter(|chat| !chat.is_group).take(FOCUS_CHATS).map(|chat| chat.guid.clone()).collect()
        };
        for guid in chats {
            if self.stopped() || !self.online() {
                break;
            }
            if self.refresh_focus(Some(&guid), Some(FOCUS_WARM_TTL_MS)).await {
                self.pause(FOCUS_GAP_MS).await;
            }
        }
        self.inner.private.lock().warming_focus = false;
    }

    fn handle(&self, event: TransportEvent) {
        match event {
            TransportEvent::Connection { status, error } => {
                let was_offline = self.inner.update(|state, events| {
                    let was_offline = state.status != ConnectionStatus::Online;
                    let connection_error = if status == ConnectionStatus::Online { None } else { error };
                    if state.status != status || state.connection_error != connection_error {
                        state.status = status;
                        state.connection_error = connection_error;
                        push_event(events, StoreEvent::Connection);
                    }
                    was_offline
                });
                if status == ConnectionStatus::Online && was_offline {
                    self.inner.wake.notify_waiters();
                    self.inner.private.lock().warm_failed.clear();
                    let has_chats = !self.inner.state.read().chats.is_empty();
                    // The slot is claimed here, before the task runs, so a `reconcile`
                    // that `start` calls next finds it taken rather than running a second pass.
                    if has_chats && self.claim_reconcile() {
                        let store = self.clone();
                        self.spawn(async move { store.reconcile_claimed().await });
                    }
                    let store = self.clone();
                    self.spawn(async move { store.flush_outbox().await });
                    let store = self.clone();
                    self.spawn(async move { store.sync_prefs().await });
                }
            }
            TransportEvent::Server(info) => self.inner.update(|state, events| {
                state.capabilities = capabilities_for(Some(&info));
                state.server = Some(info);
                push_event(events, StoreEvent::Connection);
            }),
            TransportEvent::Contacts(contacts) => self.set_contacts(contacts),
            TransportEvent::Message(message) => self.apply_message(message, true, false),
            TransportEvent::Chat(chat) => self.upsert_chat(chat),
            TransportEvent::ChatRemoved { chat_guid } => self.inner.update(|state, events| {
                let next: Vec<Arc<Chat>> = state.chats.iter().filter(|chat| chat.guid != chat_guid).cloned().collect();
                self.inner.set_chats(state, events, next);
            }),
            TransportEvent::Typing { chat_guid, typing } => self.inner.update(|state, events| {
                let guid = resolve_chat_guid(state, &chat_guid);
                self.inner.show_typing(state, events, &guid, typing);
            }),
            TransportEvent::Read { chat_guid, read } => self.inner.update(|state, events| {
                let guid = resolve_chat_guid(state, &chat_guid);
                let mut private = self.inner.private.lock();
                if read {
                    private.forced_unread.remove(&guid);
                } else {
                    private.read_locally.remove(&guid);
                }
                drop(private);
                self.inner.patch_chat(state, events, &guid, |chat| chat.unread = !read);
            }),
            TransportEvent::FaceTime { call_uuid, status, from, can_answer } => self.inner.update(|state, events| {
                let from = from.as_ref().map(|handle| handle_name(handle).to_owned());
                if status == FaceTimeStatus::Ended {
                    if let Some(current) = &mut state.facetime {
                        if current.call_uuid == call_uuid && current.status != FaceTimeCallStatus::Ready {
                            current.status = FaceTimeCallStatus::Ended;
                            push_event(events, StoreEvent::FaceTime);
                        }
                    }
                    return;
                }
                state.facetime = Some(FaceTimeCall { call_uuid, from, status: FaceTimeCallStatus::Incoming, can_answer, link: None, error: None });
                push_event(events, StoreEvent::FaceTime);
            }),
        }
    }

    fn set_contacts(&self, contacts: Vec<Contact>) {
        let owners = Arc::new(contact_owners(&contacts));
        self.inner.update(|state, events| {
            *self.inner.owners.write() = owners;
            state.contacts = Arc::new(contacts);
            push_event(events, StoreEvent::Contacts);
            self.inner.regroup(state, events);
        });
    }

    /// Port of `reconcile`: chat list, open thread, then the sweep in pages of ten since `last_sync_at`.
    pub async fn reconcile(&self) {
        if self.claim_reconcile() {
            self.reconcile_claimed().await;
        }
    }

    /// One pass at a time, and only while online.
    fn claim_reconcile(&self) -> bool {
        let state = self.inner.state.read();
        if state.status != ConnectionStatus::Online {
            return false;
        }
        let mut private = self.inner.private.lock();
        if private.reconciling {
            return false;
        }
        private.reconciling = true;
        private.reconcile_passes += 1;
        true
    }

    async fn reconcile_claimed(&self) {
        let started_at = now_ms();
        let outcome = self.reconcile_pass(started_at).await;
        if let Err(error) = &outcome {
            tracing::warn!("reconcile: {error}");
        }
        self.note_reconcile(outcome.is_ok());
        self.inner.private.lock().reconciling = false;
        self.trim_messages();
        let store = self.clone();
        self.spawn(async move { store.warm_threads().await });
        let store = self.clone();
        self.spawn(async move { store.warm_focus().await });
    }

    /// Three failed passes in a row put a line in the footer without touching
    /// `status`: the socket is up, the REST side is not answering.
    fn note_reconcile(&self, completed: bool) {
        let failures = {
            let mut private = self.inner.private.lock();
            private.reconcile_failures = if completed { 0 } else { private.reconcile_failures + 1 };
            private.reconcile_failures
        };
        self.inner.update(|state, events| {
            if state.status != ConnectionStatus::Online {
                return;
            }
            let next = (failures >= RECONCILE_FAILURES).then(|| RECONCILE_FAILED.to_owned());
            if state.connection_error != next {
                state.connection_error = next;
                push_event(events, StoreEvent::Connection);
            }
        });
    }

    async fn reconcile_pass(&self, started_at: Millis) -> Result<(), TransportError> {
        self.refresh_chats().await?;
        // The open thread comes before the sweep: after a long absence the sweep
        // has many pages to walk and what is on screen must not wait for them.
        let selected = self.inner.state.read().selected_chat.clone();
        if let Some(selected) = selected {
            let page = self.inner.transport.load_messages(&selected, LoadMessagesOptions { limit: self.page_size(), before: None }).await?;
            self.apply_page(page.items);
        }
        let mut since = self.inner.state.read().last_sync_at;
        loop {
            let filters = SearchFilters { limit: Some(SWEEP_PAGE), after: Some(since), ..SearchFilters::default() };
            let recent = self.inner.transport.search_messages("", &filters).await?;
            let count = recent.len();
            let newest = recent.iter().map(|message| message.date).max().unwrap_or(since);
            self.apply_page(recent);
            if count < SWEEP_PAGE as usize {
                break;
            }
            // `after` is exclusive, so the newest date of the page is the next cursor.
            since = (since + 1).max(newest);
            self.inner.state.write().last_sync_at = since;
            if self.stopped() || !self.online() {
                return Ok(());
            }
        }
        self.inner.state.write().last_sync_at = started_at;
        if self.inner.state.read().capabilities.scheduled_messages {
            self.refresh_scheduled().await;
        }
        Ok(())
    }

    fn apply_page(&self, messages: Vec<Message>) {
        self.inner.batch(|| {
            for message in messages {
                self.apply_message(message, true, true);
            }
        });
    }

    /// Pages the threads nobody has opened yet, most recent conversation first,
    /// one page at a time with a gap between them, so opening one paints from memory.
    async fn warm_threads(&self) {
        let budget = self.inner.options.warm_chats.unwrap_or(WARM_CHATS);
        if budget == 0 {
            return;
        }
        {
            let mut private = self.inner.private.lock();
            if private.warming {
                return;
            }
            private.warming = true;
        }
        let chats: Vec<String> = self.inner.state.read().chats.iter().take(budget).map(|chat| chat.guid.clone()).collect();
        for (index, guid) in chats.iter().enumerate() {
            if self.stopped() || !self.online() {
                break;
            }
            // Never paged, or paged so thin that opening it would page again: the
            // sweep leaves a chat holding the two rows it happened to touch.
            let (has_older, loaded) = {
                let state = self.inner.state.read();
                (state.has_older.get(guid).copied(), state.messages.get(guid).map_or(0, Vec::len))
            };
            if has_older.is_none() || (has_older == Some(true) && loaded < WARM_MESSAGES) {
                self.load_older(guid, true).await;
                self.pause(WARM_GAP_MS).await;
            }
            if index < WARM_MEDIA_CHATS {
                self.warm_media(guid).await;
            }
        }
        self.inner.private.lock().warming = false;
    }

    /// Downloads the photos, videos and voice notes in a chat's most recent
    /// messages, so a thread opens with its media on disk.
    async fn warm_media(&self, chat_guid: &str) {
        let recent: Vec<Arc<Message>> = {
            let state = self.inner.state.read();
            let list = state.messages.get(chat_guid).map(Vec::as_slice).unwrap_or_default();
            list[list.len().saturating_sub(WARM_MEDIA_MESSAGES)..].to_vec()
        };
        for message in recent {
            for attachment in &message.attachments {
                if self.stopped() || !self.online() {
                    return;
                }
                if attachment.local_path.is_some() || attachment.hidden {
                    continue;
                }
                let budget = warm_budget(&attachment.mime);
                if budget == 0 || attachment.bytes > budget || self.inner.private.lock().warm_failed.contains(&attachment.guid) {
                    continue;
                }
                // A file the server cannot produce is left to the click on the placeholder.
                if self.attachment_src(chat_guid, &message.guid, &attachment.guid, &attachment.name, Some(&attachment.mime)).await.is_err() {
                    self.inner.private.lock().warm_failed.insert(attachment.guid.clone());
                }
                self.pause(WARM_MEDIA_GAP_MS).await;
            }
        }
    }

    /// Drops the oldest rows of every conversation but the open one. A chat that
    /// loses rows gets `has_older` back, since paging into them again has to keep working.
    fn trim_messages(&self) {
        self.inner.update(|state, events| {
            let open: HashSet<String> =
                state.selected_chat.as_deref().map(|selected| conversation_members(&state.grouping, selected).into_iter().collect()).unwrap_or_default();
            for (guid, list) in state.messages.iter_mut() {
                if open.contains(guid) || list.len() <= KEEP_MESSAGES {
                    continue;
                }
                list.drain(..list.len() - KEEP_MESSAGES);
                state.has_older.insert(guid.clone(), true);
                push_event(events, StoreEvent::Thread(guid.clone()));
            }
        });
    }

    /// Port of `refreshScheduled`.
    pub async fn refresh_scheduled(&self) {
        match self.inner.transport.list_scheduled().await {
            Ok(scheduled) => self.inner.update(|state, events| {
                if state.scheduled != scheduled {
                    state.scheduled = scheduled;
                    push_event(events, StoreEvent::Scheduled);
                }
            }),
            Err(error) => tracing::warn!("scheduled: {error}"),
        }
    }

    /// Port of `refreshChats`: pages of 200 up to 5000, keeps unchanged rows' Arcs.
    pub async fn refresh_chats(&self) -> Result<(), crate::transport::TransportError> {
        let started_at = now_ms();
        let mut chats: Vec<Chat> = Vec::new();
        let mut offset = 0;
        let mut complete = false;
        while offset < CHAT_LIMIT {
            let page = self.inner.transport.list_chats(ListChatsOptions { limit: Some(CHAT_PAGE), offset: Some(offset) }).await?;
            chats.extend(page.items);
            complete = !page.has_more;
            // On first load the first page is enough to paint. A refresh keeps the
            // full list on screen until the new one is complete.
            if offset == 0 {
                self.inner.update(|state, events| {
                    if state.chats.is_empty() && !chats.is_empty() {
                        let first: Vec<Arc<Chat>> = chats.iter().map(|chat| Arc::new(self.inner.with_prefs(chat))).collect();
                        self.inner.set_chats(state, events, first);
                    }
                });
            }
            if !page.has_more {
                break;
            }
            offset += CHAT_PAGE;
        }
        self.inner.batch(|| {
            for chat in &chats {
                if let Some(last) = &chat.last_message {
                    if last.reaction.is_some() {
                        self.inner.update(|state, events| self.inner.stash_reaction(state, events, last, false, false));
                    }
                }
            }
        });
        self.inner.update(|state, events| {
            // A row the server sent unchanged keeps its Arc, so the open thread and
            // the rendered rows see the same chat and nothing repaints for no reason.
            let current: HashMap<&str, &Arc<Chat>> = state.chats.iter().map(|chat| (chat.guid.as_str(), chat)).collect();
            let mut seen: HashSet<String> = HashSet::with_capacity(chats.len());
            let mut next: Vec<Arc<Chat>> = Vec::with_capacity(chats.len().max(state.chats.len()));
            for chat in &chats {
                if !seen.insert(chat.guid.clone()) {
                    continue;
                }
                let fresh = self.inner.with_prefs(chat);
                match current.get(chat.guid.as_str()) {
                    Some(existing) if same_chat(existing, &fresh) => next.push((*existing).clone()),
                    _ => next.push(Arc::new(fresh)),
                }
            }
            // A row the server no longer lists was deleted on the Mac, unless the
            // socket brought it in after the pass started. A list cut short by
            // `CHAT_LIMIT` says nothing about the rows past its end.
            for chat in &state.chats {
                if !seen.contains(&chat.guid) && (!complete || chat.last_activity > started_at) {
                    next.push(chat.clone());
                }
            }
            self.inner.set_chats(state, events, next);
        });
        Ok(())
    }

    /// Port of `selectChat`. Sets the selection synchronously, then marks read and pages unpaged members.
    pub async fn select_chat(&self, guid: Option<&str>) {
        let (chat_guid, previous) = {
            let state = self.inner.state.read();
            (guid.map(|guid| conversation_guid(&state.grouping, guid).to_owned()), state.selected_chat.clone())
        };
        if let Some(previous) = previous.filter(|previous| Some(previous) != chat_guid.as_ref()) {
            let store = self.clone();
            self.spawn(async move { store.stop_typing(&previous).await });
        }
        self.inner.update(|state, events| {
            if state.selected_chat != chat_guid {
                state.selected_chat = chat_guid.clone();
                push_event(events, StoreEvent::Selection);
            }
        });
        // The thread just left keeps only its recent rows.
        self.trim_messages();
        // Opening a thread reads nothing; `engage` does, once the thread is used.
        self.inner.private.lock().engaged = None;
        let Some(chat_guid) = chat_guid else { return };
        let store = self.clone();
        let focus_guid = chat_guid.clone();
        self.spawn(async move {
            store.refresh_focus(Some(&focus_guid), None).await;
        });
        // A chat the socket or the sweep touched holds a few recent rows and no page boundary yet; page it before showing it.
        let unpaged: Vec<String> = {
            let state = self.inner.state.read();
            conversation_members(&state.grouping, &chat_guid).into_iter().filter(|member| !state.has_older.contains_key(member)).collect()
        };
        futures_util::future::join_all(unpaged.iter().map(|member| self.load_older(member, false))).await;
    }

    /// Port of `loadEarlier`: pages every member of the conversation back by one page.
    pub async fn load_earlier(&self, guid: &str) {
        let members = conversation_members(&self.inner.state.read().grouping, guid);
        futures_util::future::join_all(members.iter().map(|member| self.load_older(member, false))).await;
    }

    /// Port of `loadOlder`. `quiet` keeps a failure out of `error`.
    pub async fn load_older(&self, chat_guid: &str, quiet: bool) {
        let oldest = self.inner.update(|state, events| {
            if state.loading.contains(chat_guid) {
                return None;
            }
            if state.messages.contains_key(chat_guid) && state.has_older.get(chat_guid) == Some(&false) {
                return None;
            }
            state.loading.insert(chat_guid.to_owned());
            push_event(events, StoreEvent::Thread(chat_guid.to_owned()));
            Some(state.messages.get(chat_guid).and_then(|list| list.first()).map(|message| message.date))
        });
        let Some(oldest) = oldest else { return };
        let page_size = self.page_size();
        match self.inner.transport.load_messages(chat_guid, LoadMessagesOptions { limit: page_size, before: oldest }).await {
            Ok(page) => self.inner.batch(|| {
                for message in page.items {
                    self.apply_message(message, true, true);
                }
                self.inner.update(|state, events| {
                    state.messages.entry(chat_guid.to_owned()).or_default();
                    state.has_older.insert(chat_guid.to_owned(), page.has_more);
                    push_event(events, StoreEvent::Thread(chat_guid.to_owned()));
                });
            }),
            Err(error) => {
                // A background page that fails is retried by the next pass; it has no business in front of the user.
                if !quiet {
                    self.inner.set_error(error.to_string());
                }
            }
        }
        self.inner.update(|state, events| {
            state.loading.remove(chat_guid);
            push_event(events, StoreEvent::Thread(chat_guid.to_owned()));
        });
    }

    pub fn apply_message(&self, message: Message, from_server: bool, silent: bool) {
        let followup = self.inner.update(|state, events| self.inner.apply_message(state, events, message, from_server, silent));
        if let Some(chat_guid) = followup.read_receipt {
            let store = self.clone();
            self.spawn(async move { store.send_read_receipt(&chat_guid).await });
        }
        if let Some(chat_guid) = followup.fetch_chat {
            let store = self.clone();
            self.spawn(async move {
                if let Ok(chat) = store.inner.transport.get_chat(&chat_guid).await {
                    store.upsert_chat(chat);
                }
            });
        }
    }

    /// Port of `setDraft`. Synchronous: the keystroke path. Typing indicator and draft sync are spawned.
    pub fn set_draft(&self, chat_guid: &str, text: &str) {
        let can_type = self.inner.update(|state, events| {
            if state.drafts.get(chat_guid).map(String::as_str) != Some(text) {
                state.drafts.insert(chat_guid.to_owned(), text.to_owned());
                push_event(events, StoreEvent::Draft(chat_guid.to_owned()));
            }
            state.capabilities.typing
        });
        self.schedule_draft_sync(chat_guid, text);
        if !can_type {
            return;
        }
        if text.is_empty() {
            let store = self.clone();
            let guid = chat_guid.to_owned();
            self.spawn(async move { store.stop_typing(&guid).await });
            return;
        }
        let start_typing = self.inner.private.lock().typing_sent.insert(chat_guid.to_owned());
        if start_typing {
            let store = self.clone();
            let guid = chat_guid.to_owned();
            self.spawn(async move {
                if store.inner.transport.set_typing(&guid, true).await.is_err() {
                    store.inner.private.lock().typing_sent.remove(&guid);
                }
            });
        }
        let store = self.clone();
        let guid = chat_guid.to_owned();
        let timer = self.spawn_task(async move {
            tokio::time::sleep(Duration::from_millis(TYPING_IDLE_MS)).await;
            store.stop_typing(&guid).await;
        });
        if let Some(previous) = self.inner.private.lock().typing_timers.insert(chat_guid.to_owned(), timer) {
            previous.abort();
        }
    }

    /// Debounces the draft into `prefs` and out to the agent, off for the demo transport.
    fn schedule_draft_sync(&self, chat_guid: &str, text: &str) {
        if self.inner.transport.kind() == TransportKind::Demo {
            return;
        }
        let store = self.clone();
        let guid = chat_guid.to_owned();
        let draft = text.to_owned();
        let timer = self.spawn_task(async move {
            tokio::time::sleep(Duration::from_millis(DRAFT_SYNC_DEBOUNCE_MS)).await;
            store.inner.private.lock().draft_sync_timers.remove(&guid);
            store.save_prefs(&guid, ChatPrefs { draft: Some(draft), ..ChatPrefs::default() });
        });
        let mut private = self.inner.private.lock();
        private.draft_edited_at.insert(chat_guid.to_owned(), now_ms());
        if let Some(previous) = private.draft_sync_timers.insert(chat_guid.to_owned(), timer) {
            previous.abort();
        }
    }

    /// Cancels a pending draft sync and, if the agent might still hold one, tells it the draft is gone.
    fn clear_draft_sync(&self, chat_guid: &str) {
        let synced = {
            let mut private = self.inner.private.lock();
            if let Some(timer) = private.draft_sync_timers.remove(chat_guid) {
                timer.abort();
            }
            private.draft_edited_at.remove(chat_guid);
            private.prefs.get(chat_guid).and_then(|prefs| prefs.draft.as_deref()).is_some_and(|draft| !draft.is_empty())
        };
        if self.inner.transport.kind() == TransportKind::Demo || !synced {
            return;
        }
        self.save_prefs(chat_guid, ChatPrefs { draft: Some(String::new()), ..ChatPrefs::default() });
    }

    async fn stop_typing(&self, chat_guid: &str) {
        {
            let mut private = self.inner.private.lock();
            if let Some(timer) = private.typing_timers.remove(chat_guid) {
                timer.abort();
            }
            if !private.typing_sent.contains(chat_guid) {
                return;
            }
        }
        // A stop the network lost stays owed, so the next idle timer or send tries again.
        if self.inner.transport.set_typing(chat_guid, false).await.is_ok() {
            self.inner.private.lock().typing_sent.remove(chat_guid);
        }
    }

    /// Port of `setReplyingTo`. Clears editing for the chat.
    pub fn set_replying_to(&self, chat_guid: &str, message_guid: Option<&str>) {
        self.inner.update(|state, events| {
            let before = (state.replying_to.get(chat_guid).cloned(), state.editing.get(chat_guid).cloned());
            set_or_remove(&mut state.replying_to, chat_guid, message_guid);
            state.editing.remove(chat_guid);
            if before != (state.replying_to.get(chat_guid).cloned(), None) {
                push_event(events, StoreEvent::ComposerMode(chat_guid.to_owned()));
            }
        });
    }

    /// Port of `setEditing`. Clears replying and loads the message text into the draft.
    pub fn set_editing(&self, chat_guid: &str, message_guid: Option<&str>) {
        self.inner.update(|state, events| {
            let text = message_guid.and_then(|guid| state.find_message(chat_guid, guid)).map(|message| message.text.clone()).unwrap_or_default();
            set_or_remove(&mut state.editing, chat_guid, message_guid);
            state.replying_to.remove(chat_guid);
            push_event(events, StoreEvent::ComposerMode(chat_guid.to_owned()));
            if state.drafts.get(chat_guid) != Some(&text) {
                state.drafts.insert(chat_guid.to_owned(), text);
                push_event(events, StoreEvent::Draft(chat_guid.to_owned()));
            }
        });
    }

    /// Clears the composer the way a send or a schedule does. Returns the message being edited, if any.
    fn clear_composer(&self, chat_guid: &str, clear_reply: bool) -> (Option<String>, Option<String>) {
        let cleared = self.inner.update(|state, events| {
            if state.drafts.get(chat_guid).is_some_and(|draft| !draft.is_empty()) {
                state.drafts.insert(chat_guid.to_owned(), String::new());
                push_event(events, StoreEvent::Draft(chat_guid.to_owned()));
            }
            let editing = state.editing.remove(chat_guid);
            let replying = if clear_reply && editing.is_none() { state.replying_to.remove(chat_guid) } else { None };
            if editing.is_some() || replying.is_some() {
                push_event(events, StoreEvent::ComposerMode(chat_guid.to_owned()));
            }
            (editing, replying)
        });
        self.clear_draft_sync(chat_guid);
        let store = self.clone();
        let guid = chat_guid.to_owned();
        self.spawn(async move { store.stop_typing(&guid).await });
        cleared
    }

    /// Port of `send`. Applies the optimistic row synchronously, then queues the send in the outbox.
    pub fn send(&self, chat_guid: &str, text: &str, effect: Option<&str>) {
        let body = text.trim();
        if body.is_empty() {
            return;
        }
        let (editing, reply_to) = self.clear_composer(chat_guid, true);
        if let Some(editing) = editing {
            let store = self.clone();
            let (guid, body) = (chat_guid.to_owned(), body.to_owned());
            self.spawn(async move { store.edit(&guid, &editing, &body).await });
            return;
        }
        let temp_guid = next_temp_guid();
        let service = self.inner.service_for(&self.inner.state.read(), chat_guid);
        let optimistic = Message {
            guid: temp_guid.clone(),
            temp_guid: Some(temp_guid.clone()),
            chat_guid: chat_guid.to_owned(),
            text: body.to_owned(),
            from_me: true,
            date: now_ms(),
            service,
            reply_to: reply_to.clone(),
            effect: effect.map(str::to_owned),
            ..Message::default()
        };
        self.apply_message(optimistic.clone(), false, false);
        self.enqueue(OutboxItem {
            chat_guid: chat_guid.to_owned(),
            temp_guid,
            optimistic,
            attempts: 0,
            send: Outgoing::Text { text: body.to_owned(), reply_to, effect: effect.map(str::to_owned) },
        });
    }

    /// Port of `scheduleSend`.
    pub async fn schedule_send(&self, chat_guid: &str, text: &str, send_at: Millis) {
        let body = text.trim();
        if body.is_empty() {
            return;
        }
        self.clear_composer(chat_guid, false);
        self.inner.update(|state, events| {
            if state.replying_to.remove(chat_guid).is_some() {
                push_event(events, StoreEvent::ComposerMode(chat_guid.to_owned()));
            }
        });
        match self.inner.transport.schedule_text(chat_guid, body, send_at).await {
            Ok(message) => self.inner.update(|state, events| {
                state.scheduled.push(message);
                push_event(events, StoreEvent::Scheduled);
            }),
            Err(error) => self.inner.set_error(error.to_string()),
        }
    }

    /// Port of `cancelScheduled`. Optimistic; restored with an error if the server refuses.
    pub async fn cancel_scheduled(&self, id: &str) {
        let previous = self.inner.update(|state, events| {
            let previous = state.scheduled.clone();
            state.scheduled.retain(|item| item.id != id);
            push_event(events, StoreEvent::Scheduled);
            previous
        });
        if let Err(error) = self.inner.transport.cancel_scheduled(id).await {
            self.inner.update(|state, events| {
                state.scheduled = previous;
                state.error = Some(error.to_string());
                push_event(events, StoreEvent::Scheduled);
                push_event(events, StoreEvent::Error);
            });
        }
    }

    /// Port of `sendAttachment`. Optimistic row with `local_path` set, then the
    /// outbox. `size` is the picture's pixel size when the caller already knows
    /// it (a staged file whose header was read, a GIF the picker described), so
    /// the row paints at its final size from the first frame; the byte count,
    /// and the size when none was given, are read off the caller's thread and
    /// patched in.
    pub fn send_attachment(&self, chat_guid: &str, path: &Path, size: Option<crate::image::ImageSize>) {
        let temp_guid = next_temp_guid();
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "attachment".into());
        let service = self.inner.service_for(&self.inner.state.read(), chat_guid);
        let optimistic = Message {
            guid: temp_guid.clone(),
            temp_guid: Some(temp_guid.clone()),
            chat_guid: chat_guid.to_owned(),
            from_me: true,
            date: now_ms(),
            service,
            attachments: vec![Attachment {
                guid: temp_guid.clone(),
                name: name.clone(),
                mime: mime_for_path(path),
                bytes: 0,
                width: size.map(|size| size.width),
                height: size.map(|size| size.height),
                measured: size.is_some(),
                is_sticker: false,
                local_path: Some(path.to_owned()),
                hidden: false,
                duration_ms: None,
            }],
            ..Message::default()
        };
        self.apply_message(optimistic.clone(), false, false);
        self.enqueue(OutboxItem {
            chat_guid: chat_guid.to_owned(),
            temp_guid: temp_guid.clone(),
            optimistic,
            attempts: 0,
            send: Outgoing::Attachment { path: path.to_owned(), name },
        });
        let store = self.clone();
        let (chat_guid, file) = (chat_guid.to_owned(), path.to_owned());
        let is_image = mime_for_path(path).starts_with("image/");
        self.spawn(async move {
            let bytes = tokio::fs::metadata(&file).await.map(|meta| meta.len()).unwrap_or(0);
            let measured = if size.is_none() && is_image { crate::image::image_size(&file.to_string_lossy()).await } else { None };
            store.set_optimistic_file(&chat_guid, &temp_guid, bytes, measured);
        });
    }

    /// Fills in the byte count, and the pixel size when it was not known at
    /// send time, of a queued attachment on the row and on the outbox copy a
    /// failure would put back. A row the server echo already replaced carries
    /// the server's own numbers and is left alone.
    fn set_optimistic_file(&self, chat_guid: &str, temp_guid: &str, bytes: u64, size: Option<crate::image::ImageSize>) {
        let patch = |attachment: &mut Attachment| {
            attachment.bytes = bytes;
            if let Some(size) = size {
                attachment.width = Some(size.width);
                attachment.height = Some(size.height);
                attachment.measured = true;
            }
        };
        self.inner.update(|state, events| {
            {
                let mut private = self.inner.private.lock();
                if let Some(item) = private.outbox.iter_mut().find(|item| item.temp_guid == temp_guid) {
                    item.optimistic.attachments.iter_mut().for_each(patch);
                }
            }
            let Some(row) = state.find_message(chat_guid, temp_guid).cloned() else { return };
            let mut next = (*row).clone();
            next.attachments.iter_mut().for_each(patch);
            self.inner.replace_message(state, events, next);
        });
    }

    fn enqueue(&self, item: OutboxItem) {
        self.inner.private.lock().outbox.push_back(item);
        let store = self.clone();
        self.spawn(async move { store.flush_outbox().await });
    }

    /// Port of `pendingSends`: sends still waiting for the server.
    pub fn pending_sends(&self) -> usize {
        self.inner.private.lock().outbox.len()
    }

    // One send at a time, in order. A send the server refused fails right away;
    // one the network lost is retried, and the whole queue waits out a dropped
    // connection instead of failing every message behind it.
    async fn flush_outbox(&self) {
        {
            let mut private = self.inner.private.lock();
            if private.flushing {
                return;
            }
            private.flushing = true;
        }
        loop {
            let next = if self.stopped() || !self.online() { None } else { self.inner.private.lock().outbox.front().cloned() };
            let Some(item) = next else {
                // Re-checked under the lock: a reconnect that saw `flushing` set
                // while this loop was leaving would otherwise strand the queue.
                let mut private = self.inner.private.lock();
                private.flushing = false;
                if private.outbox.is_empty() || self.stopped() || !self.online() {
                    return;
                }
                private.flushing = true;
                continue;
            };
            // A reply lost on the way back is not a lost send: once the socket echo has replaced the optimistic row, the send is done.
            if self.settled(&item) {
                self.inner.private.lock().outbox.pop_front();
                continue;
            }
            match self.run_send(&item).await {
                Ok(sent) => {
                    self.inner.private.lock().outbox.pop_front();
                    self.apply_message(Message { temp_guid: Some(item.temp_guid.clone()), ..sent }, false, false);
                }
                Err(error) => {
                    if self.settled(&item) {
                        self.inner.private.lock().outbox.pop_front();
                        continue;
                    }
                    let attempts = {
                        let mut private = self.inner.private.lock();
                        match private.outbox.front_mut() {
                            Some(front) => {
                                front.attempts += 1;
                                front.attempts
                            }
                            None => SEND_ATTEMPTS,
                        }
                    };
                    if !error.is_retryable() || attempts >= SEND_ATTEMPTS {
                        self.inner.private.lock().outbox.pop_front();
                        self.apply_message(Message { error: Some(error.to_string()), ..item.optimistic.clone() }, false, false);
                    } else {
                        self.pause(SEND_RETRY_MS * u64::from(attempts)).await;
                    }
                }
            }
        }
    }

    async fn run_send(&self, item: &OutboxItem) -> Result<Message, TransportError> {
        let transport = &self.inner.transport;
        match &item.send {
            Outgoing::Text { text, reply_to, effect } => {
                let options =
                    SendTextOptions { reply_to: reply_to.clone(), effect: effect.clone(), subject: None, temp_guid: Some(item.temp_guid.clone()) };
                transport.send_text(&item.chat_guid, text, options).await
            }
            Outgoing::Attachment { path, name } => {
                let options = SendAttachmentOptions { name: Some(name.clone()), is_audio: false, temp_guid: Some(item.temp_guid.clone()) };
                let mut sent = transport.send_attachment(&item.chat_guid, path, options).await?;
                for attachment in &mut sent.attachments {
                    if attachment.local_path.is_none() {
                        attachment.local_path = Some(path.clone());
                    }
                }
                Ok(sent)
            }
        }
    }

    fn settled(&self, item: &OutboxItem) -> bool {
        let state = self.inner.state.read();
        state
            .messages
            .get(&item.chat_guid)
            .and_then(|list| list.iter().find(|message| message.temp_guid.as_deref() == Some(item.temp_guid.as_str())))
            .is_some_and(|row| Some(row.guid.as_str()) != row.temp_guid.as_deref())
    }

    /// Port of `retry`: drops the failed row and queues it again as a fresh
    /// optimistic row with the same text, reply and effect. The composer is not
    /// involved: what is being typed and replied to stays as it is.
    pub fn retry(&self, chat_guid: &str, message_guid: &str) {
        let failed = self.inner.update(|state, events| {
            let failed = state.find_message(chat_guid, message_guid).filter(|message| message.error.is_some()).cloned()?;
            if let Some(list) = state.messages.get_mut(&failed.chat_guid) {
                list.retain(|item| item.guid != message_guid);
            }
            push_event(events, StoreEvent::Thread(failed.chat_guid.clone()));
            Some(failed)
        });
        let Some(failed) = failed else { return };
        let send = match failed.attachments.first().and_then(|attachment| attachment.local_path.clone().map(|path| (path, attachment.name.clone()))) {
            Some((path, name)) => Outgoing::Attachment { path, name },
            None => Outgoing::Text { text: failed.text.clone(), reply_to: failed.reply_to.clone(), effect: failed.effect.clone() },
        };
        let temp_guid = next_temp_guid();
        let mut optimistic = Message { guid: temp_guid.clone(), temp_guid: Some(temp_guid.clone()), date: now_ms(), error: None, ..(*failed).clone() };
        for attachment in optimistic.attachments.iter_mut().filter(|attachment| attachment.guid == failed.guid) {
            attachment.guid = temp_guid.clone();
        }
        self.apply_message(optimistic.clone(), false, false);
        self.enqueue(OutboxItem { chat_guid: failed.chat_guid.clone(), temp_guid, optimistic, attempts: 0, send });
    }

    fn find_message(&self, chat_guid: &str, message_guid: &str) -> Option<Arc<Message>> {
        self.inner.state.read().find_message(chat_guid, message_guid).cloned()
    }

    fn replace_message(&self, message: Message) {
        self.inner.update(|state, events| self.inner.replace_message(state, events, message));
    }

    /// Puts `target` back and reports `error`: the rollback every optimistic message action shares.
    fn roll_back(&self, target: Arc<Message>, error: TransportError) {
        self.inner.update(|state, events| {
            self.inner.replace_message(state, events, (*target).clone());
            state.error = Some(error.to_string());
            push_event(events, StoreEvent::Error);
        });
    }

    /// Port of `react`. Toggles my tapback optimistically, rolls back on failure.
    pub async fn react(&self, chat_guid: &str, message_guid: &str, kind: TapbackKind, emoji: Option<&str>) {
        let Some(target) = self.find_message(chat_guid, message_guid) else { return };
        let mine = target.tapbacks.iter().find(|item| item.from_me);
        let remove = mine.is_some_and(|mine| mine.kind == kind && mine.emoji.as_deref() == emoji);
        let mut tapbacks: Vec<Tapback> = target.tapbacks.iter().filter(|item| !item.from_me).cloned().collect();
        if !remove {
            tapbacks.push(Tapback { guid: format!("temp-tapback-{}", now_ms()), kind, emoji: emoji.map(str::to_owned), from_me: true, sender: None });
        }
        self.replace_message(Message { tapbacks, ..(*target).clone() });
        let options = ReactOptions { emoji: emoji.map(str::to_owned), remove, part_index: None };
        if let Err(error) = self.inner.transport.react(&target.chat_guid, message_guid, kind, options).await {
            self.roll_back(target, error);
        }
    }

    /// Port of `edit`. backwardsCompatText is `Edited to “<text>”`.
    pub async fn edit(&self, chat_guid: &str, message_guid: &str, text: &str) {
        let Some(target) = self.find_message(chat_guid, message_guid) else { return };
        self.replace_message(Message { text: text.to_owned(), date_edited: Some(now_ms()), ..(*target).clone() });
        let options = EditOptions { part_index: None, backwards_compat_text: Some(format!("Edited to \u{201C}{text}\u{201D}")) };
        match self.inner.transport.edit_message(&target.chat_guid, message_guid, text, options).await {
            Ok(updated) => self.apply_message(updated, false, false),
            Err(error) => self.roll_back(target, error),
        }
    }

    /// Port of `unsend`.
    pub async fn unsend(&self, chat_guid: &str, message_guid: &str) {
        let Some(target) = self.find_message(chat_guid, message_guid) else { return };
        self.replace_message(Message { date_retracted: Some(now_ms()), ..(*target).clone() });
        if let Err(error) = self.inner.transport.unsend_message(&target.chat_guid, message_guid, None).await {
            self.roll_back(target, error);
        }
    }

    /// Port of `notifySilenced`: iMessage's "Notify Anyway".
    pub async fn notify_silenced(&self, chat_guid: &str, message_guid: &str) {
        let Some(target) = self.find_message(chat_guid, message_guid) else { return };
        self.replace_message(Message { notified: Some(true), ..(*target).clone() });
        if let Err(error) = self.inner.transport.notify_silenced(&target.chat_guid, message_guid).await {
            self.roll_back(target, error);
        }
    }

    /// Clears the unread dot on every member at once. Returns the members that were unread, which are owed a receipt.
    fn clear_unread(&self, chat_guid: &str) -> Vec<String> {
        self.inner.update(|state, events| {
            let mut unread = Vec::new();
            let now = now_ms();
            for member in conversation_members(&state.grouping, chat_guid) {
                let Some(chat) = state.chat(&member) else { continue };
                // The Mac's clock can run ahead of this one, so the newest message's own date counts too.
                let read_at = chat.last_message.as_ref().map_or(now, |message| message.date.max(now));
                {
                    let mut private = self.inner.private.lock();
                    private.forced_unread.remove(&member);
                    private.read_locally.insert(member.clone(), read_at);
                }
                if !chat.unread {
                    continue;
                }
                self.inner.patch_chat(state, events, &member, |chat| chat.unread = false);
                unread.push(member);
            }
            unread
        })
    }

    /// A click, scroll or keystroke in the open thread or its composer. Reads
    /// the conversation and keeps reading what arrives in it for `ENGAGED_FOR`.
    pub fn engage(&self, chat_guid: &str) {
        let (primary, unread) = {
            let state = self.inner.state.read();
            let primary = conversation_guid(&state.grouping, chat_guid).to_owned();
            let unread = conversation_members(&state.grouping, &primary).iter().any(|member| state.chat(member).is_some_and(|chat| chat.unread));
            (primary, unread)
        };
        self.inner.private.lock().engaged = Some((primary.clone(), tokio::time::Instant::now()));
        if unread {
            let store = self.clone();
            self.spawn(async move { store.mark_read(&primary).await });
        }
    }

    /// The window lost focus: nothing is being read any more.
    pub fn disengage(&self) {
        self.inner.private.lock().engaged = None;
    }

    /// Every member, receipts only where the chat allows them.
    pub async fn mark_read(&self, chat_guid: &str) {
        for member in self.clear_unread(chat_guid) {
            self.send_read_receipt(&member).await;
        }
    }

    /// "Read without receipts" keeps the dot logic but skips the network call, so the other side never learns.
    async fn send_read_receipt(&self, chat_guid: &str) {
        let (primary, capable) = {
            let state = self.inner.state.read();
            (conversation_guid(&state.grouping, chat_guid).to_owned(), state.capabilities.read_receipts)
        };
        let wanted = self.inner.private.lock().prefs.get(&primary).and_then(|prefs| prefs.read_receipts).unwrap_or(true);
        if !wanted || !capable {
            return;
        }
        let _ = self.inner.transport.mark_read(chat_guid).await;
    }

    /// Port of `markUnread`. The dot is forced locally because the Mac leaves `dateRead` alone.
    pub async fn mark_unread(&self, chat_guid: &str) {
        let capable = self.inner.update(|state, events| {
            {
                let mut private = self.inner.private.lock();
                private.forced_unread.insert(chat_guid.to_owned());
                private.read_locally.remove(chat_guid);
            }
            self.inner.patch_chat(state, events, chat_guid, |chat| chat.unread = true);
            state.capabilities.mark_unread
        });
        if capable {
            let _ = self.inner.transport.mark_unread(chat_guid).await;
        }
    }

    fn toggle_pref(&self, chat_guid: &str, flip: impl FnOnce(&Chat) -> ChatPrefs) {
        let Some(chat) = self.inner.state.read().chat(chat_guid).cloned() else { return };
        let patch = flip(&chat);
        self.save_prefs(chat_guid, patch);
        self.inner.update(|state, events| {
            let fresh = self.inner.with_prefs(&chat);
            self.inner.patch_chat(state, events, chat_guid, |row| {
                row.pinned = fresh.pinned;
                row.muted = fresh.muted;
                row.read_receipts = fresh.read_receipts;
            });
        });
    }

    /// Port of `togglePin`.
    pub fn toggle_pin(&self, chat_guid: &str) {
        self.toggle_pref(chat_guid, |chat| ChatPrefs { pinned: Some(!chat.pinned), ..ChatPrefs::default() });
    }

    /// Port of `toggleMute`.
    pub fn toggle_mute(&self, chat_guid: &str) {
        self.toggle_pref(chat_guid, |chat| ChatPrefs { muted: Some(!chat.muted), ..ChatPrefs::default() });
    }

    /// Port of `toggleReadReceipts`.
    pub fn toggle_read_receipts(&self, chat_guid: &str) {
        self.toggle_pref(chat_guid, |chat| ChatPrefs { read_receipts: Some(!chat.read_receipts.unwrap_or(true)), ..ChatPrefs::default() });
    }

    fn notify_prefs_change(&self, prefs: HashMap<String, ChatPrefs>) {
        if let Some(callback) = self.inner.options.on_prefs_change.clone() {
            self.spawn(async move { callback(&prefs) });
        }
    }

    fn notify_gif_favorites_change(&self, favorites: HashMap<String, GifFavorite>) {
        if let Some(callback) = self.inner.options.on_gif_favorites_change.clone() {
            self.spawn(async move { callback(&favorites) });
        }
    }

    fn save_prefs(&self, chat_guid: &str, patch: ChatPrefs) {
        let now = now_ms();
        let prefs = {
            let mut private = self.inner.private.lock();
            let entry = private.prefs.entry(chat_guid.to_owned()).or_default();
            // A draft or a mute must not look like a pin change, or a chat pinned in Messages.app unpins itself the moment it is typed in.
            if patch.pinned.is_some() {
                entry.pinned = patch.pinned;
                entry.pinned_at = Some(now);
            }
            if patch.muted.is_some() {
                entry.muted = patch.muted;
            }
            if patch.read_receipts.is_some() {
                entry.read_receipts = patch.read_receipts;
            }
            if patch.draft.is_some() {
                entry.draft = patch.draft;
            }
            entry.updated_at = Some(now);
            private.prefs.clone()
        };
        self.notify_prefs_change(prefs);
        let store = self.clone();
        self.spawn(async move { store.sync_prefs().await });
    }

    /// Port of `toggleGifFavorite`. An unfavorite keeps a tombstone.
    pub fn toggle_gif_favorite(&self, gif: &Gif) {
        let favorites = self.inner.update(|state, events| {
            let was_favorite = state.gif_favorites.get(&gif.id).is_some_and(|entry| !entry.removed);
            state.gif_favorites.insert(gif.id.clone(), GifFavorite { gif: gif.clone(), updated_at: now_ms(), removed: was_favorite });
            push_event(events, StoreEvent::GifFavorites);
            state.gif_favorites.clone()
        });
        self.notify_gif_favorites_change(favorites);
        let store = self.clone();
        self.spawn(async move { store.sync_prefs().await });
    }

    /// Port of `syncPrefs`: pushes this client's entries, applies the merged set and the Mac's own pins.
    pub async fn sync_prefs(&self) {
        let Some(agent) = &self.inner.agent else { return };
        let gifs = {
            let state = self.inner.state.read();
            if state.status != ConnectionStatus::Online {
                return;
            }
            state.gif_favorites.clone()
        };
        let mine: HashMap<String, ChatPrefs> = self
            .inner
            .private
            .lock()
            .prefs
            .iter()
            .filter(|(_, entry)| entry.updated_at.is_some())
            .map(|(guid, entry)| (guid.clone(), ChatPrefs { mac_pinned: None, mac_pinned_at: None, ..entry.clone() }))
            .collect();
        match agent.sync_prefs(&mine, &gifs).await {
            Ok(shared) => self.apply_shared_prefs(shared),
            Err(error) => tracing::warn!("prefs: {error}"),
        }
    }

    fn apply_shared_prefs(&self, shared: SharedPrefs) {
        let pins: Vec<String> = {
            let state = self.inner.state.read();
            shared.mac_pinned.iter().filter_map(|identifier| chat_for_pin(&state, identifier)).collect()
        };
        // Pins made before sync existed carry no time. Once the Mac's own list is in, that list is the truth and they yield to it.
        let mac_list_known = shared.mac_pinned_at.is_some();
        let (changed, prefs, edited_at) = {
            let mut private = self.inner.private.lock();
            let mut next: HashMap<String, ChatPrefs> = HashMap::with_capacity(private.prefs.len());
            for (guid, entry) in &private.prefs {
                let mut entry = ChatPrefs { mac_pinned: None, mac_pinned_at: None, ..entry.clone() };
                if mac_list_known && entry.pinned == Some(true) && entry.updated_at.is_none() {
                    entry.pinned = None;
                }
                next.insert(guid.clone(), entry);
            }
            for (guid, entry) in &shared.chats {
                let local = next.entry(guid.clone()).or_default();
                if local.updated_at.is_none() || entry.updated_at.unwrap_or(0) > local.updated_at.unwrap_or(0) {
                    local.pinned = entry.pinned;
                    local.pinned_at = entry.pinned_at;
                    local.muted = entry.muted;
                    local.read_receipts = entry.read_receipts;
                    local.draft = entry.draft.clone();
                    local.updated_at = entry.updated_at;
                }
            }
            next.retain(|_, entry| *entry != ChatPrefs::default());
            for guid in &pins {
                let entry = next.entry(guid.clone()).or_default();
                entry.mac_pinned = Some(true);
                entry.mac_pinned_at = Some(shared.mac_pinned_at.unwrap_or(0));
            }
            let changed = next != private.prefs;
            private.prefs = next;
            (changed, private.prefs.clone(), private.draft_edited_at.clone())
        };
        if changed {
            self.notify_prefs_change(prefs.clone());
        }
        let incoming_gifs = shared.gifs.unwrap_or_default();
        let gif_favorites = self.inner.update(|state, events| {
            if changed {
                let next: Vec<Arc<Chat>> = state.chats.iter().map(|chat| self.inner.with_prefs_arc(chat)).collect();
                self.inner.set_chats(state, events, next);
            }
            // The draft that won the merge above still yields to text typed locally after its timestamp.
            for (guid, entry) in &prefs {
                let Some(draft) = &entry.draft else { continue };
                let current = state.drafts.get(guid).map(String::as_str).unwrap_or("");
                if draft == current {
                    continue;
                }
                if !current.is_empty() && edited_at.get(guid).copied().unwrap_or(0) >= entry.updated_at.unwrap_or(0) {
                    continue;
                }
                state.drafts.insert(guid.clone(), draft.clone());
                push_event(events, StoreEvent::Draft(guid.clone()));
            }
            // An agent that predates this feature answers with no `gifs` field at all.
            let merged = merge_gif_favorites(&state.gif_favorites, &incoming_gifs);
            if merged == state.gif_favorites {
                return None;
            }
            state.gif_favorites = merged;
            push_event(events, StoreEvent::GifFavorites);
            Some(state.gif_favorites.clone())
        });
        if let Some(favorites) = gif_favorites {
            self.notify_gif_favorites_change(favorites);
        }
    }

    /// Port of `deleteChat`. The row goes at once and comes back with an error if the server refuses.
    pub async fn delete_chat(&self, chat_guid: &str) {
        let target = self.inner.update(|state, events| {
            let target = state.chat(chat_guid).cloned()?;
            let next: Vec<Arc<Chat>> = state.chats.iter().filter(|chat| chat.guid != chat_guid).cloned().collect();
            if state.selected_chat.as_deref() == Some(chat_guid) {
                state.selected_chat = next.first().map(|chat| chat.guid.clone());
                push_event(events, StoreEvent::Selection);
            }
            self.inner.set_chats(state, events, next);
            // Everything keyed on a chat that has gone, or it would keep its thread, draft and flags for the life of the process.
            state.messages.remove(chat_guid);
            state.has_older.remove(chat_guid);
            state.loading.remove(chat_guid);
            if state.typing.remove(chat_guid) {
                push_event(events, StoreEvent::Typing(chat_guid.to_owned()));
            }
            if state.drafts.remove(chat_guid).is_some() {
                push_event(events, StoreEvent::Draft(chat_guid.to_owned()));
            }
            if state.replying_to.remove(chat_guid).is_some() | state.editing.remove(chat_guid).is_some() {
                push_event(events, StoreEvent::ComposerMode(chat_guid.to_owned()));
            }
            push_event(events, StoreEvent::Thread(chat_guid.to_owned()));
            Some(target)
        });
        let Some(target) = target else { return };
        if let Err(error) = self.inner.transport.delete_chat(chat_guid).await {
            self.inner.update(|state, events| {
                if state.chat(chat_guid).is_none() {
                    let mut next = state.chats.clone();
                    next.push(target);
                    self.inner.set_chats(state, events, next);
                }
                state.error = Some(error.to_string());
                push_event(events, StoreEvent::Error);
            });
        }
    }

    /// Port of `exportConversation`: pages up to 2000 messages, writes Markdown to Downloads, opens it.
    pub async fn export_conversation(&self, chat_guid: &str) {
        let primary = self.inner.update(|state, events| {
            if state.exporting_chat.is_some() {
                return None;
            }
            let primary = conversation_guid(&state.grouping, chat_guid).to_owned();
            state.exporting_chat = Some(primary.clone());
            push_event(events, StoreEvent::Exporting);
            Some(primary)
        });
        let Some(primary) = primary else { return };
        loop {
            let (more, count) = {
                let state = self.inner.state.read();
                (conversation_has_older(&state, &primary), conversation_messages(&state, &primary).len())
            };
            if !more || count >= EXPORT_MAX_MESSAGES {
                break;
            }
            self.load_earlier(&primary).await;
            let after = conversation_messages(&self.inner.state.read(), &primary).len();
            if after == count && conversation_has_older(&self.inner.state.read(), &primary) {
                // A page that failed would loop here forever; export what is in.
                break;
            }
        }
        let snapshot = {
            let state = self.inner.state.read();
            state.chat(&primary).cloned().map(|chat| (chat, conversation_handles(&state, &primary), conversation_messages(&state, &primary)))
        };
        if let Some((chat, handles, messages)) = snapshot {
            match crate::export::write_conversation_export(&chat, &handles, &messages).await {
                Ok(path) => crate::open::open_external(&path.to_string_lossy()),
                Err(error) => self.inner.set_error(error.to_string()),
            }
        }
        self.inner.update(|state, events| {
            state.exporting_chat = None;
            push_event(events, StoreEvent::Exporting);
        });
    }

    /// Port of `createChat`. Sets `error` and returns it on failure so the new-chat screen stays open.
    pub async fn create_chat(&self, addresses: &[String], first_message: &str) -> Result<(), crate::transport::TransportError> {
        let chat = match self.inner.transport.create_chat(addresses, first_message, None).await {
            Ok(chat) => chat,
            Err(error) => {
                self.inner.set_error(error.to_string());
                return Err(error);
            }
        };
        let guid = chat.guid.clone();
        self.upsert_chat(chat);
        self.select_chat(Some(&guid)).await;
        Ok(())
    }

    fn upsert_chat(&self, chat: Chat) {
        self.inner.update(|state, events| {
            let mut fresh = self.inner.with_prefs(&chat);
            match state.chat(&chat.guid).cloned() {
                Some(existing) => {
                    // A chat event without its last message must not blank the row's preview.
                    if fresh.last_message.is_none() {
                        fresh.last_message = existing.last_message.clone();
                    }
                    fresh.last_activity = fresh.last_activity.max(existing.last_activity);
                    self.inner.patch_chat(state, events, &chat.guid, |row| *row = fresh);
                }
                None => {
                    let mut next = state.chats.clone();
                    next.push(Arc::new(fresh));
                    self.inner.set_chats(state, events, next);
                }
            }
        });
    }

    /// Optimistic chat edit: `apply` now, `undo` with the error if the server refuses.
    async fn chat_action<F>(&self, chat_guid: &str, apply: impl FnOnce(&mut Chat), undo: impl FnOnce(&mut Chat, &Chat), request: F)
    where
        F: Future<Output = Result<(), TransportError>>,
    {
        let Some(before) = self.inner.state.read().chat(chat_guid).cloned() else { return };
        self.inner.update(|state, events| {
            self.inner.patch_chat(state, events, chat_guid, apply);
        });
        if let Err(error) = request.await {
            self.inner.update(|state, events| {
                self.inner.patch_chat(state, events, chat_guid, |chat| undo(chat, &before));
                state.error = Some(error.to_string());
                push_event(events, StoreEvent::Error);
            });
        }
    }

    /// Port of `renameGroup`.
    pub async fn rename_group(&self, chat_guid: &str, name: &str) {
        let request = self.inner.transport.rename_group(chat_guid, name);
        self.chat_action(chat_guid, |chat| chat.display_name = Some(name.to_owned()), |chat, before| chat.display_name = before.display_name.clone(), request)
            .await;
    }

    /// Port of `addParticipant`. The list shows the new person right away; the server's chat event brings their name.
    pub async fn add_participant(&self, chat_guid: &str, address: &str) {
        if address.is_empty() {
            return;
        }
        let request = self.inner.transport.add_participant(chat_guid, address);
        self.chat_action(
            chat_guid,
            |chat| chat.participants.push(Handle { address: address.to_owned(), service: chat.service, name: None, avatar: None }),
            |chat, before| chat.participants = before.participants.clone(),
            request,
        )
        .await;
    }

    /// Port of `removeParticipant`.
    pub async fn remove_participant(&self, chat_guid: &str, address: &str) {
        let request = self.inner.transport.remove_participant(chat_guid, address);
        self.chat_action(
            chat_guid,
            |chat| chat.participants.retain(|item| item.address != address),
            |chat, before| chat.participants = before.participants.clone(),
            request,
        )
        .await;
    }

    /// Port of `leaveGroup`.
    pub async fn leave_group(&self, chat_guid: &str) {
        if let Err(error) = self.inner.transport.leave_group(chat_guid).await {
            self.inner.set_error(error.to_string());
        }
    }

    fn set_facetime(&self, call: Option<FaceTimeCall>) {
        self.inner.update(|state, events| {
            if state.facetime != call {
                state.facetime = call;
                push_event(events, StoreEvent::FaceTime);
            }
        });
    }

    /// Port of `dismissFaceTime`.
    pub fn dismiss_facetime(&self) {
        self.set_facetime(None);
    }

    /// Port of `answerFaceTime`: answers on the Mac and opens the returned link.
    pub async fn answer_facetime(&self) {
        let call = self.inner.state.read().facetime.clone();
        let Some(call) = call.filter(|call| call.can_answer && call.status == FaceTimeCallStatus::Incoming) else { return };
        self.set_facetime(Some(FaceTimeCall { status: FaceTimeCallStatus::Answering, ..call.clone() }));
        match self.inner.transport.answer_facetime(&call.call_uuid).await {
            Ok(link) => {
                self.set_facetime(Some(FaceTimeCall { status: FaceTimeCallStatus::Ready, link: Some(link.clone()), ..call }));
                crate::open::open_external(&link);
            }
            Err(error) => self.set_facetime(Some(FaceTimeCall { status: FaceTimeCallStatus::Failed, error: Some(error.to_string()), ..call })),
        }
    }

    /// Port of `declineFaceTime`.
    pub async fn decline_facetime(&self) {
        let call = self.inner.state.read().facetime.clone();
        self.set_facetime(None);
        if let Some(call) = call.filter(|call| call.can_answer) {
            let _ = self.inner.transport.leave_facetime(&call.call_uuid).await;
        }
    }

    /// Port of `startFaceTime`: creates a link, sends it in the chat, opens it here.
    pub async fn start_facetime(&self, chat_guid: &str) {
        match self.inner.transport.create_facetime_link().await {
            Ok(link) => {
                self.send(chat_guid, &link, None);
                crate::open::open_external(&link);
            }
            Err(error) => self.inner.set_error(error.to_string()),
        }
    }

    /// Port of `clearError`.
    pub fn clear_error(&self) {
        self.inner.update(|state, events| {
            if state.error.take().is_some() {
                push_event(events, StoreEvent::Error);
            }
        });
    }

    /// Port of `attachmentSrc`: downloads, measures images from the header, stores `local_path`.
    pub async fn attachment_src(
        &self,
        chat_guid: &str,
        message_guid: &str,
        attachment_guid: &str,
        name: &str,
        mime: Option<&str>,
    ) -> Result<std::path::PathBuf, crate::transport::TransportError> {
        let current = self.find_message(chat_guid, message_guid).and_then(|message| message.attachments.iter().find(|item| item.guid == attachment_guid).cloned());
        let options = AttachmentPathOptions {
            name: Some(name.to_owned()),
            mime: mime.map(str::to_owned),
            sticker: current.as_ref().is_some_and(|attachment| attachment.is_sticker),
        };
        let local = self.inner.transport.attachment_path(attachment_guid, options).await?;
        // chat.db often has no pixel size for an attachment, and the one the server
        // reports ignores EXIF orientation. The file header settles both.
        let is_image = current.as_ref().is_some_and(|attachment| mime.unwrap_or(&attachment.mime).starts_with("image/"));
        let size = if is_image { crate::image::image_size(&local.to_string_lossy()).await } else { None };
        self.inner.update(|state, events| {
            let Some(fresh) = state.find_message(chat_guid, message_guid).cloned() else { return };
            let mut next = (*fresh).clone();
            for attachment in next.attachments.iter_mut().filter(|item| item.guid == attachment_guid) {
                attachment.local_path = Some(local.clone());
                if let Some(size) = size {
                    attachment.width = Some(size.width);
                    attachment.height = Some(size.height);
                    attachment.measured = true;
                }
            }
            self.inner.replace_message(state, events, next);
        });
        Ok(local)
    }
}

fn set_or_remove(map: &mut HashMap<String, String>, key: &str, value: Option<&str>) {
    match value {
        Some(value) => {
            map.insert(key.to_owned(), value.to_owned());
        }
        None => {
            map.remove(key);
        }
    }
}

/// Private API events may name a chat with its old `iMessage;-;` prefix while chat.db on macOS 26 says `any;-;`. Match on the identifier.
fn resolve_chat_guid(state: &AppState, guid: &str) -> String {
    if state.chat(guid).is_some() {
        return guid.to_owned();
    }
    let identifier = match guid.find(';') {
        Some(first) => match guid[first + 1..].find(';') {
            Some(second) => &guid[first + 1 + second + 1..],
            None => &guid[..],
        },
        None => guid,
    };
    let suffix = format!(";{identifier}");
    state
        .chats
        .iter()
        .find(|chat| chat.identifier == identifier || chat.guid.ends_with(&suffix))
        .map(|chat| chat.guid.clone())
        .unwrap_or_else(|| guid.to_owned())
}

/// The chat behind an entry of the Mac's pin list, resolved to its conversation's primary.
fn chat_for_pin(state: &AppState, identifier: &str) -> Option<String> {
    let found = chat_matching(state, identifier)?;
    Some(conversation_guid(&state.grouping, &found.guid).to_owned())
}

fn chat_matching<'a>(state: &'a AppState, identifier: &str) -> Option<&'a Arc<Chat>> {
    if let Some(chat) = state.chats.iter().find(|chat| chat.group_id.as_deref() == Some(identifier) || chat.guid == identifier) {
        return Some(chat);
    }
    if identifier.contains(';') {
        let guid = resolve_chat_guid(state, identifier);
        return state.chat(&guid);
    }
    let wanted = normalize_address(identifier);
    state.chats.iter().find(|chat| !chat.is_group && chat.participants.iter().any(|handle| normalize_address(&handle.address) == wanted))
}

impl Inner {
    /// Applies `mutate` under the write lock and sends the events it recorded
    /// once the lock is released, or at the end of the outermost batch.
    fn update<R>(&self, mutate: impl FnOnce(&mut AppState, &mut Vec<StoreEvent>) -> R) -> R {
        let mut events = Vec::new();
        let result = {
            let mut state = self.state.write();
            mutate(&mut state, &mut events)
        };
        if !events.is_empty() {
            self.publish(events);
        }
        result
    }

    /// Port of `batch`: folds every `update` inside `run` into one send per distinct event.
    fn batch<R>(&self, run: impl FnOnce() -> R) -> R {
        self.private.lock().batch_depth += 1;
        let result = run();
        let events = {
            let mut private = self.private.lock();
            private.batch_depth -= 1;
            if private.batch_depth == 0 { std::mem::take(&mut private.batched) } else { Vec::new() }
        };
        if !events.is_empty() {
            self.send(events);
        }
        result
    }

    fn publish(&self, events: Vec<StoreEvent>) {
        {
            let mut private = self.private.lock();
            if private.batch_depth > 0 {
                for event in events {
                    if matches!(event, StoreEvent::Incoming(_)) {
                        private.batched.push(event);
                    } else {
                        push_event(&mut private.batched, event);
                    }
                }
                return;
            }
        }
        self.send(events);
    }

    fn send(&self, events: Vec<StoreEvent>) {
        let cacheable = events.iter().any(StoreEvent::cacheable);
        for event in events {
            let _ = self.events.send(event);
        }
        if !cacheable {
            return;
        }
        let (Some(cache), Some(me)) = (&self.options.cache, self.me.upgrade()) else { return };
        let _entered = self.runtime.enter();
        cache.schedule(Box::new(move || {
            let state = me.state.read();
            let (outbox, read_at) = {
                let private = me.private.lock();
                (private.outbox.iter().cloned().collect(), private.read_locally.clone())
            };
            snapshot_for_cache(&state, outbox, read_at)
        }));
    }

    fn set_error(&self, error: String) {
        self.update(|state, events| {
            state.error = Some(error);
            push_event(events, StoreEvent::Error);
        });
    }

    fn with_prefs(&self, chat: &Chat) -> Chat {
        let private = self.private.lock();
        let prefs = private.prefs.get(&chat.guid);
        Chat {
            pinned: is_pinned(prefs),
            muted: prefs.and_then(|prefs| prefs.muted).unwrap_or(false),
            read_receipts: Some(prefs.and_then(|prefs| prefs.read_receipts).unwrap_or(true)),
            unread: (chat.unread && !read_here_covers(&private, chat)) || private.forced_unread.contains(&chat.guid),
            ..chat.clone()
        }
    }

    /// `with_prefs` that keeps the Arc when nothing changed.
    fn with_prefs_arc(&self, chat: &Arc<Chat>) -> Arc<Chat> {
        let fresh = self.with_prefs(chat);
        if fresh == **chat { chat.clone() } else { Arc::new(fresh) }
    }

    /// Replaces the whole list: sorts it, reports rows whose Arc changed and
    /// whether membership or order did, then regroups.
    fn set_chats(&self, state: &mut AppState, events: &mut Vec<StoreEvent>, mut next: Vec<Arc<Chat>>) {
        sort_chats(&mut next);
        let same_order = next.len() == state.chats.len() && next.iter().zip(&state.chats).all(|(a, b)| a.guid == b.guid);
        if same_order {
            for (a, b) in next.iter().zip(&state.chats) {
                if !Arc::ptr_eq(a, b) {
                    push_event(events, StoreEvent::Chat(a.guid.clone()));
                }
            }
        } else {
            let before: HashMap<&str, &Arc<Chat>> = state.chats.iter().map(|chat| (chat.guid.as_str(), chat)).collect();
            for chat in &next {
                if before.get(chat.guid.as_str()).is_some_and(|old| !Arc::ptr_eq(old, chat)) {
                    push_event(events, StoreEvent::Chat(chat.guid.clone()));
                }
            }
            push_event(events, StoreEvent::ChatList);
        }
        state.chats = next;
        self.regroup(state, events);
    }

    /// Patches one row in place. Moves it only when its sort key changed, and
    /// regroups only when that can change a conversation's primary.
    fn patch_chat(&self, state: &mut AppState, events: &mut Vec<StoreEvent>, guid: &str, patch: impl FnOnce(&mut Chat)) -> bool {
        let Some(index) = state.chats.iter().position(|chat| chat.guid == guid) else { return false };
        let old = &state.chats[index];
        let mut next = (**old).clone();
        patch(&mut next);
        if next == **old {
            return false;
        }
        let regroup = next.participants != old.participants
            || next.is_group != old.is_group
            || (next.last_activity != old.last_activity && state.grouping.primary_of.contains_key(guid));
        let resort = next.pinned != old.pinned || next.last_activity != old.last_activity;
        state.chats[index] = Arc::new(next);
        push_event(events, StoreEvent::Chat(guid.to_owned()));
        if resort {
            let in_order = (index == 0 || chat_order(&state.chats[index - 1], &state.chats[index]) != CmpOrdering::Greater)
                && (index + 1 >= state.chats.len() || chat_order(&state.chats[index], &state.chats[index + 1]) != CmpOrdering::Greater);
            if !in_order {
                let chat = state.chats.remove(index);
                let position = state.chats.partition_point(|item| chat_order(item, &chat) != CmpOrdering::Greater);
                state.chats.insert(position, chat);
                push_event(events, StoreEvent::ChatList);
            }
        }
        if regroup {
            self.regroup(state, events);
        }
        true
    }

    fn regroup(&self, state: &mut AppState, events: &mut Vec<StoreEvent>) {
        let owners = self.owners.read().clone();
        let grouping = group_chats_with(&state.chats, &owners);
        if grouping == state.grouping {
            return;
        }
        state.grouping = grouping;
        push_event(events, StoreEvent::ChatList);
        // The open thread follows its conversation when a newer member becomes the primary.
        if let Some(selected) = &state.selected_chat {
            let primary = conversation_guid(&state.grouping, selected).to_owned();
            if &primary != selected {
                state.selected_chat = Some(primary);
                push_event(events, StoreEvent::Selection);
            }
        }
    }

    /// The service the conversation is actually on: the latest message wins over the chat row, which macOS 26 no longer types.
    fn service_for(&self, state: &AppState, chat_guid: &str) -> Service {
        if let Some(message) = state.messages.get(chat_guid).and_then(|list| list.iter().rev().find(|m| m.group_event.is_none() && m.reaction.is_none())) {
            return message.service;
        }
        state.chat(chat_guid).map(|chat| chat.service).unwrap_or_default()
    }

    /// Replaces the row with the same guid in the message's own chat, in place.
    fn replace_message(&self, state: &mut AppState, events: &mut Vec<StoreEvent>, message: Message) {
        let Some(list) = state.messages.get_mut(&message.chat_guid) else { return };
        let Some(index) = list.iter().position(|item| item.guid == message.guid) else { return };
        if *list[index] == message {
            return;
        }
        let event = StoreEvent::Message { chat_guid: message.chat_guid.clone(), message_guid: message.guid.clone() };
        list[index] = Arc::new(message);
        push_event(events, event);
    }

    /// The server sends some reactions without their handle. In a one-to-one chat the only person it can be is the other participant.
    fn reaction_sender(&self, state: &AppState, message: &Message) -> Option<Handle> {
        if message.sender.is_some() || message.from_me {
            return message.sender.clone();
        }
        let chat = state.chat(&message.chat_guid)?;
        if !chat.is_group && chat.participants.len() == 1 { chat.participants.first().cloned() } else { None }
    }

    fn stash_reaction(&self, state: &mut AppState, events: &mut Vec<StoreEvent>, message: &Message, from_server: bool, silent: bool) {
        let Some(reaction) = &message.reaction else { return };
        let target = state.find_message(&message.chat_guid, &reaction.target_guid).cloned();
        if from_server && !silent && !message.from_me && !reaction.removed {
            if let Some(chat) = state.chat(&message.chat_guid).filter(|chat| !chat.muted) {
                events.push(StoreEvent::Incoming(Incoming { chat: chat.clone(), message: Arc::new(message.clone()), target: target.clone() }));
            }
        }
        let sender = self.reaction_sender(state, message);
        match target {
            Some(target) => {
                let tapback = Tapback { guid: message.guid.clone(), kind: reaction.kind, emoji: reaction.emoji.clone(), from_me: message.from_me, sender };
                let tapbacks = merge_tapback(&target.tapbacks, tapback, reaction.removed);
                self.replace_message(state, events, Message { tapbacks, ..(*target).clone() });
            }
            None => {
                self.private.lock().pending_reactions.entry(reaction.target_guid.clone()).or_default().push(PendingReaction {
                    guid: message.guid.clone(),
                    kind: reaction.kind,
                    emoji: reaction.emoji.clone(),
                    removed: reaction.removed,
                    from_me: message.from_me,
                    sender,
                });
            }
        }
    }

    fn show_typing(&self, state: &mut AppState, events: &mut Vec<StoreEvent>, chat_guid: &str, typing: bool) {
        let timer = if typing {
            self.me.upgrade().map(|inner| {
                let store = MessagesStore { inner };
                let guid = chat_guid.to_owned();
                self.runtime
                    .spawn(async move {
                        tokio::time::sleep(Duration::from_millis(TYPING_SHOWN_MAX_MS)).await;
                        store.inner.update(|state, events| store.inner.show_typing(state, events, &guid, false));
                    })
                    .abort_handle()
            })
        } else {
            None
        };
        {
            let mut private = self.private.lock();
            let previous = match timer {
                Some(timer) => private.typing_shown.insert(chat_guid.to_owned(), timer),
                None => private.typing_shown.remove(chat_guid),
            };
            if let Some(previous) = previous {
                previous.abort();
            }
        }
        if state.typing.contains(chat_guid) == typing {
            return;
        }
        if typing {
            state.typing.insert(chat_guid.to_owned());
        } else {
            state.typing.remove(chat_guid);
        }
        push_event(events, StoreEvent::Typing(chat_guid.to_owned()));
    }

    fn apply_message(&self, state: &mut AppState, events: &mut Vec<StoreEvent>, incoming: Message, from_server: bool, silent: bool) -> Followup {
        let mut followup = Followup::default();
        if incoming.reaction.is_some() {
            self.stash_reaction(state, events, &incoming, from_server, silent);
            return followup;
        }
        let chat_guid = incoming.chat_guid.clone();
        let list = state.messages.entry(chat_guid.clone()).or_default();
        let mut existing_index = list.iter().position(|item| {
            item.guid == incoming.guid
                || incoming.temp_guid.as_ref().is_some_and(|temp| &item.guid == temp || item.temp_guid.as_ref() == Some(temp))
        });
        // The socket echoes a send before its HTTP reply lands, and for an
        // attachment the echo carries no temp guid, so match it to the optimistic
        // row by content instead of letting it sit beside it.
        if existing_index.is_none() && from_server && incoming.from_me && incoming.temp_guid.is_none() {
            existing_index = list.iter().position(|item| {
                item.temp_guid.as_deref() == Some(item.guid.as_str()) && item.error.is_none() && same_send(item, &incoming)
            });
        }
        let is_new = existing_index.is_none();
        let message = match existing_index {
            Some(index) => {
                let existing = list[index].clone();
                // The server leaves the receipt fields out of some copies (a notification's
                // message has no quiet-delivery flags), and a read never un-reads.
                let merged = Message {
                    date_read: incoming.date_read.or(existing.date_read),
                    date_delivered: incoming.date_delivered.or(existing.date_delivered),
                    delivered_quietly: incoming.delivered_quietly.or(existing.delivered_quietly),
                    notified: incoming.notified.or(existing.notified),
                    attachments: merge_attachments(&existing.attachments, &incoming.attachments),
                    tapbacks: if incoming.tapbacks.is_empty() { existing.tapbacks.clone() } else { incoming.tapbacks.clone() },
                    temp_guid: existing.temp_guid.clone().or_else(|| incoming.temp_guid.clone()),
                    ..incoming
                };
                // A re-read that brings back what is already here must not touch state at all.
                if *existing == merged {
                    return followup;
                }
                let message = Arc::new(merged);
                list.remove(index);
                let position = insert_sorted(list, message.clone());
                if position == index && existing.guid == message.guid {
                    push_event(events, StoreEvent::Message { chat_guid: chat_guid.clone(), message_guid: message.guid.clone() });
                } else {
                    push_event(events, StoreEvent::Thread(chat_guid.clone()));
                }
                message
            }
            None => {
                let pending = self.private.lock().pending_reactions.remove(&incoming.guid);
                let mut incoming = incoming;
                for entry in pending.unwrap_or_default() {
                    let tapback = Tapback { guid: entry.guid, kind: entry.kind, emoji: entry.emoji, from_me: entry.from_me, sender: entry.sender };
                    incoming.tapbacks = merge_tapback(&incoming.tapbacks, tapback, entry.removed);
                }
                let message = Arc::new(incoming);
                insert_sorted(list, message.clone());
                push_event(events, StoreEvent::Thread(chat_guid.clone()));
                message
            }
        };
        if is_new {
            self.private.lock().forced_unread.remove(&chat_guid);
        }
        let newest_is_this = state.messages.get(&chat_guid).and_then(|list| list.last()).is_some_and(|newest| newest.guid == message.guid);
        match state.chat(&chat_guid).cloned() {
            Some(chat) if newest_is_this => {
                let primary = conversation_guid(&state.grouping, &chat_guid);
                let selected_and_visible = state.selected_chat.as_deref() == Some(primary)
                    && self.private.lock().engaged.as_ref().is_some_and(|(engaged, at)| engaged == primary && at.elapsed() < ENGAGED_FOR);
                // A read date on the newest incoming message means it was read on another device; the dot goes without waiting for the sweep.
                let read_elsewhere = !message.from_me && message.date_read.is_some();
                let unread = if is_new && !message.from_me && !selected_and_visible {
                    true
                } else if selected_and_visible || read_elsewhere {
                    false
                } else {
                    chat.unread
                };
                self.patch_chat(state, events, &chat_guid, |row| {
                    row.last_message = Some((*message).clone());
                    row.last_activity = row.last_activity.max(message.date);
                    row.unread = unread;
                });
                if is_new && !message.from_me && from_server && !silent && !chat.muted {
                    let chat = state.chat(&chat_guid).cloned().unwrap_or(chat);
                    events.push(StoreEvent::Incoming(Incoming { chat, message: message.clone(), target: None }));
                }
                // The chat is already flagged read by now, so `mark_read` would skip it; tell the server directly.
                if is_new && !message.from_me && selected_and_visible && from_server {
                    followup.read_receipt = Some(chat_guid.clone());
                }
            }
            None if from_server => followup.fetch_chat = Some(chat_guid.clone()),
            _ => {}
        }
        if is_new && !message.from_me {
            for member in conversation_members(&state.grouping, &chat_guid) {
                self.show_typing(state, events, &member, false);
            }
        }
        followup
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversations::conversation_focus;
    use crate::gifs::favorite_gifs;
    use crate::model::Reaction;
    use crate::transport::{Page, TransportResult};
    use async_trait::async_trait;
    use std::sync::atomic::AtomicU64;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn info() -> ServerInfo {
        ServerInfo { version: "test".into(), macos_version: Some("15.0".into()), private_api: false, helper_connected: false, icloud_account: None }
    }

    fn private_api() -> ServerInfo {
        ServerInfo { private_api: true, helper_connected: true, ..info() }
    }

    fn handle(address: &str) -> Handle {
        Handle { address: address.into(), service: Service::IMessage, name: None, avatar: None }
    }

    fn chat(guid: &str, last_activity: Millis) -> Chat {
        Chat { guid: guid.into(), identifier: guid.into(), participants: vec![handle(guid)], last_activity, ..Chat::default() }
    }

    fn message(chat_guid: &str, text: &str, date: Millis, from_me: bool) -> Message {
        let n = SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        Message { guid: format!("msg-{n}"), chat_guid: chat_guid.into(), text: text.into(), from_me, date, ..Message::default() }
    }

    fn media(chat_guid: &str, attachment_guid: &str, date: Millis, bytes: u64, mime: &str) -> Message {
        Message {
            attachments: vec![Attachment {
                guid: attachment_guid.into(),
                name: format!("file-{attachment_guid}"),
                mime: mime.into(),
                bytes,
                width: None,
                height: None,
                measured: false,
                is_sticker: false,
                local_path: None,
                hidden: false,
                duration_ms: None,
            }],
            ..message(chat_guid, "", date, false)
        }
    }

    fn gif(id: &str) -> Gif {
        Gif {
            id: id.into(),
            preview_url: format!("https://static.klipy.com/{id}/sm.gif"),
            gif_url: format!("https://static.klipy.com/{id}/hd.gif"),
            width: 200,
            height: 200,
        }
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Failure {
        Network,
        Server,
    }

    struct Fake {
        listeners: Vec<mpsc::UnboundedSender<TransportEvent>>,
        connect_attempts: u32,
        fail_connects: u32,
        chats: Vec<Chat>,
        messages: Vec<Message>,
        attachment_source: Option<String>,
        send_calls: Vec<String>,
        /// The `reply_to` of each text send, in order.
        reply_calls: Vec<Option<String>>,
        send_failures: VecDeque<Failure>,
        list_failure: Option<TransportError>,
        search_calls: Vec<u32>,
        load_calls: Vec<String>,
        attachment_calls: Vec<String>,
        mark_read_calls: Vec<String>,
        delete_failure: Option<TransportError>,
        rename_failure: Option<TransportError>,
        cancel_failure: Option<TransportError>,
        /// Holds a delete, a rename or the chat list open, so a test can look at the state in between.
        action_delay_ms: u64,
        focus_calls: Vec<String>,
        focus: FocusStatus,
        notify_calls: Vec<String>,
        notify_failure: Option<TransportError>,
        scheduled: Vec<ScheduledMessage>,
        scheduled_seq: u32,
        server_info: ServerInfo,
    }

    /// Every failure the network can produce and every answer the server can give, on demand.
    struct FakeTransport(Mutex<Fake>);

    impl FakeTransport {
        fn new(chats: Vec<Chat>) -> Arc<Self> {
            Arc::new(Self(Mutex::new(Fake {
                listeners: Vec::new(),
                connect_attempts: 0,
                fail_connects: 0,
                chats,
                messages: Vec::new(),
                attachment_source: None,
                send_calls: Vec::new(),
                reply_calls: Vec::new(),
                send_failures: VecDeque::new(),
                list_failure: None,
                search_calls: Vec::new(),
                load_calls: Vec::new(),
                attachment_calls: Vec::new(),
                mark_read_calls: Vec::new(),
                delete_failure: None,
                rename_failure: None,
                cancel_failure: None,
                action_delay_ms: 0,
                focus_calls: Vec::new(),
                focus: FocusStatus::None,
                notify_calls: Vec::new(),
                notify_failure: None,
                scheduled: Vec::new(),
                scheduled_seq: 0,
                server_info: info(),
            })))
        }

        fn lock(&self) -> parking_lot::MutexGuard<'_, Fake> {
            self.0.lock()
        }

        fn emit(&self, event: TransportEvent) {
            self.lock().listeners.retain(|tx| tx.send(event.clone()).is_ok());
        }

        async fn delay(&self) {
            let ms = self.lock().action_delay_ms;
            if ms > 0 {
                tokio::time::sleep(Duration::from_millis(ms)).await;
            }
        }
    }

    fn unsupported<T>() -> TransportResult<T> {
        Err(TransportError::Network("not in this test".into()))
    }

    #[async_trait]
    impl Transport for FakeTransport {
        fn kind(&self) -> TransportKind {
            TransportKind::BlueBubbles
        }

        async fn connect(&self) -> TransportResult<ServerInfo> {
            let (fail, info) = {
                let mut fake = self.lock();
                fake.connect_attempts += 1;
                let fail = fake.fail_connects > 0;
                if fail {
                    fake.fail_connects -= 1;
                }
                (fail, fake.server_info.clone())
            };
            if fail {
                self.emit(TransportEvent::Connection { status: ConnectionStatus::Offline, error: Some("fetch failed".into()) });
                return Err(TransportError::Network("fetch failed".into()));
            }
            self.emit(TransportEvent::Connection { status: ConnectionStatus::Online, error: None });
            Ok(info)
        }

        async fn disconnect(&self) {}

        fn subscribe(&self) -> mpsc::UnboundedReceiver<TransportEvent> {
            let (tx, rx) = mpsc::unbounded_channel();
            self.lock().listeners.push(tx);
            rx
        }

        fn seed_contacts(&self, _contacts: &[Contact]) {}

        /// Like the real server, every row carries its newest message.
        async fn list_chats(&self, _options: ListChatsOptions) -> TransportResult<Page<Chat>> {
            self.delay().await;
            let fake = self.lock();
            if let Some(error) = fake.list_failure.clone() {
                return Err(error);
            }
            let items = fake
                .chats
                .iter()
                .map(|chat| {
                    let newest = fake.messages.iter().filter(|item| item.chat_guid == chat.guid).max_by_key(|item| item.date).cloned();
                    Chat { last_message: newest.or_else(|| chat.last_message.clone()), ..chat.clone() }
                })
                .collect();
            Ok(Page { items, has_more: false })
        }

        async fn get_chat(&self, chat_guid: &str) -> TransportResult<Chat> {
            self.lock()
                .chats
                .iter()
                .find(|chat| chat.guid == chat_guid)
                .cloned()
                .ok_or(TransportError::Server { status: 404, message: "no chat".into() })
        }

        async fn load_messages(&self, chat_guid: &str, options: LoadMessagesOptions) -> TransportResult<Page<Message>> {
            let mut fake = self.lock();
            fake.load_calls.push(chat_guid.into());
            let mut all: Vec<Message> = fake
                .messages
                .iter()
                .filter(|item| item.chat_guid == chat_guid && options.before.is_none_or(|before| item.date < before))
                .cloned()
                .collect();
            all.sort_by_key(|item| item.date);
            let start = all.len().saturating_sub(options.limit as usize);
            let items = all[start..].to_vec();
            Ok(Page { has_more: items.len() < all.len(), items })
        }

        async fn search_messages(&self, _query: &str, filters: &SearchFilters) -> TransportResult<Vec<Message>> {
            let mut fake = self.lock();
            let limit = filters.limit.unwrap_or(50);
            fake.search_calls.push(limit);
            let mut all: Vec<Message> = fake.messages.iter().filter(|item| filters.after.is_none_or(|after| item.date > after)).cloned().collect();
            all.sort_by_key(|item| item.date);
            all.truncate(limit as usize);
            Ok(all)
        }

        async fn list_contacts(&self) -> TransportResult<Vec<Contact>> {
            Ok(Vec::new())
        }

        async fn send_text(&self, chat_guid: &str, text: &str, options: SendTextOptions) -> TransportResult<Message> {
            let mut fake = self.lock();
            fake.send_calls.push(text.into());
            fake.reply_calls.push(options.reply_to);
            match fake.send_failures.pop_front() {
                Some(Failure::Network) => return Err(TransportError::Network("fetch failed".into())),
                Some(Failure::Server) => return Err(TransportError::Server { status: 400, message: "the server said no".into() }),
                None => {}
            }
            let sent = message(chat_guid, text, now_ms(), true);
            fake.messages.push(sent.clone());
            Ok(sent)
        }

        async fn send_attachment(&self, _chat_guid: &str, _path: &Path, _options: SendAttachmentOptions) -> TransportResult<Message> {
            unsupported()
        }

        async fn attachment_path(&self, attachment_guid: &str, _options: AttachmentPathOptions) -> TransportResult<PathBuf> {
            let mut fake = self.lock();
            fake.attachment_calls.push(attachment_guid.into());
            Ok(PathBuf::from(fake.attachment_source.clone().unwrap_or_else(|| format!("/cache/{attachment_guid}.jpg"))))
        }

        async fn create_chat(&self, _addresses: &[String], _first_message: &str, _service: Option<Service>) -> TransportResult<Chat> {
            unsupported()
        }

        async fn mark_read(&self, chat_guid: &str) -> TransportResult<()> {
            self.lock().mark_read_calls.push(chat_guid.into());
            Ok(())
        }

        async fn delete_chat(&self, _chat_guid: &str) -> TransportResult<()> {
            self.delay().await;
            self.lock().delete_failure.clone().map_or(Ok(()), Err)
        }

        async fn schedule_text(&self, chat_guid: &str, text: &str, send_at: Millis) -> TransportResult<ScheduledMessage> {
            let mut fake = self.lock();
            fake.scheduled_seq += 1;
            let message = ScheduledMessage { id: format!("sched-{}", fake.scheduled_seq), chat_guid: chat_guid.into(), text: text.into(), send_at };
            fake.scheduled.push(message.clone());
            Ok(message)
        }

        async fn list_scheduled(&self) -> TransportResult<Vec<ScheduledMessage>> {
            Ok(self.lock().scheduled.clone())
        }

        async fn cancel_scheduled(&self, id: &str) -> TransportResult<()> {
            let mut fake = self.lock();
            if let Some(error) = fake.cancel_failure.clone() {
                return Err(error);
            }
            fake.scheduled.retain(|item| item.id != id);
            Ok(())
        }

        async fn react(&self, _chat_guid: &str, _message_guid: &str, _kind: TapbackKind, _options: ReactOptions) -> TransportResult<()> {
            Ok(())
        }

        async fn set_typing(&self, _chat_guid: &str, _typing: bool) -> TransportResult<()> {
            Ok(())
        }

        async fn mark_unread(&self, _chat_guid: &str) -> TransportResult<()> {
            Ok(())
        }

        async fn edit_message(&self, _chat_guid: &str, _message_guid: &str, _text: &str, _options: EditOptions) -> TransportResult<Message> {
            unsupported()
        }

        async fn unsend_message(&self, _chat_guid: &str, _message_guid: &str, _part_index: Option<u32>) -> TransportResult<()> {
            Ok(())
        }

        async fn rename_group(&self, _chat_guid: &str, _name: &str) -> TransportResult<()> {
            self.delay().await;
            self.lock().rename_failure.clone().map_or(Ok(()), Err)
        }

        async fn add_participant(&self, _chat_guid: &str, _address: &str) -> TransportResult<()> {
            Ok(())
        }

        async fn remove_participant(&self, _chat_guid: &str, _address: &str) -> TransportResult<()> {
            Ok(())
        }

        async fn leave_group(&self, _chat_guid: &str) -> TransportResult<()> {
            Ok(())
        }

        async fn set_group_icon(&self, _chat_guid: &str, _path: &Path) -> TransportResult<()> {
            Ok(())
        }

        async fn focus_status(&self, address: &str) -> TransportResult<FocusStatus> {
            let mut fake = self.lock();
            fake.focus_calls.push(address.into());
            Ok(fake.focus)
        }

        async fn notify_silenced(&self, _chat_guid: &str, message_guid: &str) -> TransportResult<()> {
            let mut fake = self.lock();
            fake.notify_calls.push(message_guid.into());
            fake.notify_failure.clone().map_or(Ok(()), Err)
        }

        async fn create_facetime_link(&self) -> TransportResult<String> {
            unsupported()
        }

        async fn answer_facetime(&self, _call_uuid: &str) -> TransportResult<String> {
            unsupported()
        }

        async fn leave_facetime(&self, _call_uuid: &str) -> TransportResult<()> {
            Ok(())
        }
    }

    fn options() -> StoreOptions {
        StoreOptions { reconcile_every_ms: Some(0), ..StoreOptions::default() }
    }

    fn store_with(transport: &Arc<FakeTransport>, options: StoreOptions) -> MessagesStore {
        MessagesStore::new(transport.clone(), options, tokio::runtime::Handle::current())
    }

    async fn started(transport: &Arc<FakeTransport>, options: StoreOptions) -> MessagesStore {
        let store = store_with(transport, options);
        store.start().await;
        store
    }

    /// Lets every spawned task and queued transport event run to its next timer.
    async fn settle(store: &MessagesStore) {
        for _ in 0..3 {
            store.sync_events().await;
            for _ in 0..50 {
                tokio::task::yield_now().await;
            }
        }
    }

    fn drain(events: &mut broadcast::Receiver<StoreEvent>) -> Vec<StoreEvent> {
        let mut out = Vec::new();
        while let Ok(event) = events.try_recv() {
            out.push(event);
        }
        out
    }

    fn texts(store: &MessagesStore, chat_guid: &str) -> Vec<String> {
        store.state().messages.get(chat_guid).map(|list| list.iter().map(|item| item.text.clone()).collect()).unwrap_or_default()
    }

    fn unread(store: &MessagesStore, chat_guid: &str) -> bool {
        store.state().chat(chat_guid).is_some_and(|chat| chat.unread)
    }

    async fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(std::time::Instant::now() < deadline, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    mod connecting {
        use super::*;

        #[tokio::test(start_paused = true)]
        async fn keeps_retrying_with_backoff_until_the_server_answers() {
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            transport.lock().fail_connects = 2;
            let store = store_with(&transport, options());
            let running = store.clone();
            let start = tokio::spawn(async move { running.start().await });
            settle(&store).await;
            assert_eq!(store.state().status, ConnectionStatus::Offline);
            assert_eq!(store.state().connection_error.as_deref(), Some("fetch failed"));
            tokio::time::advance(Duration::from_millis(2000)).await;
            settle(&store).await;
            assert_eq!(transport.lock().connect_attempts, 2);
            tokio::time::advance(Duration::from_millis(4000)).await;
            start.await.unwrap();
            assert_eq!(transport.lock().connect_attempts, 3);
            assert_eq!(store.state().status, ConnectionStatus::Online);
            assert_eq!(store.state().connection_error, None);
            assert_eq!(store.state().selected_chat.as_deref(), Some("a"));
            store.stop().await;
        }
    }

    mod sending {
        use super::*;

        #[tokio::test(start_paused = true)]
        async fn queues_sends_while_offline_and_flushes_them_in_order_when_the_connection_is_back() {
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let store = started(&transport, options()).await;
            transport.emit(TransportEvent::Connection { status: ConnectionStatus::Offline, error: Some("transport close".into()) });
            settle(&store).await;
            store.send("a", "first", None);
            store.send("a", "second", None);
            settle(&store).await;
            assert!(transport.lock().send_calls.is_empty());
            assert_eq!(store.pending_sends(), 2);
            assert_eq!(texts(&store, "a"), ["first", "second"]);
            assert!(store.state().messages["a"].iter().all(|item| item.temp_guid.as_deref() == Some(item.guid.as_str())));

            transport.emit(TransportEvent::Connection { status: ConnectionStatus::Online, error: None });
            settle(&store).await;
            assert_eq!(transport.lock().send_calls, ["first", "second"]);
            assert_eq!(store.pending_sends(), 0);
            assert!(store.state().messages["a"].iter().all(|item| item.guid.starts_with("msg-")));
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn sends_queued_offline_survive_a_relaunch() {
            let dir = std::env::temp_dir().join(format!("messages-store-outbox-{}-{}", std::process::id(), fastrand::u64(..)));
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let cache = Arc::new(StateCache::new(&dir));
            let store = started(&transport, StoreOptions { cache: Some(cache.clone()), warm_chats: Some(0), ..options() }).await;
            transport.emit(TransportEvent::Connection { status: ConnectionStatus::Offline, error: Some("transport close".into()) });
            settle(&store).await;
            store.send("a", "while away", None);
            settle(&store).await;
            assert_eq!(store.pending_sends(), 1);
            cache.flush().await;
            store.stop().await;

            let second = FakeTransport::new(vec![chat("a", 1000)]);
            let restarted = started(&second, StoreOptions { cache: Some(Arc::new(StateCache::new(&dir))), warm_chats: Some(0), ..options() }).await;
            settle(&restarted).await;
            assert_eq!(second.lock().send_calls, ["while away"]);
            assert_eq!(restarted.pending_sends(), 0);
            assert!(texts(&restarted, "a").contains(&"while away".to_string()));
            assert!(restarted.state().messages["a"].iter().all(|item| item.guid.starts_with("msg-")));
            restarted.stop().await;
            std::fs::remove_dir_all(&dir).ok();
        }

        #[tokio::test(start_paused = true)]
        async fn folds_the_socket_echo_of_a_send_into_the_optimistic_row_when_the_echo_has_no_temp_guid() {
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let store = started(&transport, options()).await;
            transport.lock().send_failures = VecDeque::from([Failure::Network]);
            store.send("a", "photo caption", None);
            settle(&store).await;
            let echo = message("a", "photo caption", now_ms(), true);
            transport.emit(TransportEvent::Message(echo.clone()));
            settle(&store).await;
            let guids: Vec<String> = store.state().messages["a"].iter().map(|item| item.guid.clone()).collect();
            assert_eq!(guids, [echo.guid.clone()]);
            tokio::time::advance(Duration::from_millis(2000)).await;
            settle(&store).await;
            assert_eq!(transport.lock().send_calls, ["photo caption"]);
            assert_eq!(store.pending_sends(), 0);
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn retries_a_send_the_network_dropped_and_fails_one_the_server_refused() {
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let store = started(&transport, options()).await;
            transport.lock().send_failures = VecDeque::from([Failure::Network]);
            store.send("a", "flaky", None);
            settle(&store).await;
            assert_eq!(transport.lock().send_calls, ["flaky"]);
            assert_eq!(store.state().messages["a"][0].error, None);
            tokio::time::advance(Duration::from_millis(2000)).await;
            settle(&store).await;
            assert_eq!(transport.lock().send_calls, ["flaky", "flaky"]);
            assert!(store.state().messages["a"][0].guid.starts_with("msg-"));

            transport.lock().send_failures = VecDeque::from([Failure::Server]);
            store.send("a", "refused", None);
            settle(&store).await;
            let refused = store.state().messages["a"].iter().find(|item| item.text == "refused").cloned().unwrap();
            assert_eq!(refused.error.as_deref(), Some("the server said no"));
            assert_eq!(store.pending_sends(), 0);

            // Try again drops the failed row and sends the text once more.
            store.retry("a", &refused.guid);
            settle(&store).await;
            assert_eq!(transport.lock().send_calls.last().map(String::as_str), Some("refused"));
            assert!(store.state().messages["a"].iter().filter(|item| item.text == "refused").all(|item| item.error.is_none()));
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn try_again_keeps_the_reply_and_leaves_the_composer_alone() {
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let earlier = message("a", "context", 500, false);
            transport.lock().messages.push(earlier.clone());
            let store = started(&transport, options()).await;
            transport.lock().send_failures = VecDeque::from([Failure::Server]);
            store.set_replying_to("a", Some(&earlier.guid));
            store.send("a", "refused", None);
            settle(&store).await;
            let refused = store.state().messages["a"].iter().find(|item| item.text == "refused").cloned().unwrap();
            assert_eq!(refused.error.as_deref(), Some("the server said no"));
            assert_eq!(refused.reply_to.as_deref(), Some(earlier.guid.as_str()));

            store.set_draft("a", "half typed");
            store.set_replying_to("a", Some(&earlier.guid));
            let mut events = store.events();
            store.retry("a", &refused.guid);
            let queued = store.state().messages["a"].iter().find(|item| item.text == "refused").cloned().unwrap();
            assert_eq!(queued.error, None);
            assert_eq!(queued.reply_to.as_deref(), Some(earlier.guid.as_str()));
            settle(&store).await;
            assert_eq!(transport.lock().reply_calls.last().cloned().flatten().as_deref(), Some(earlier.guid.as_str()));
            assert_eq!(store.state().drafts.get("a").map(String::as_str), Some("half typed"));
            assert_eq!(store.state().replying_to.get("a").map(String::as_str), Some(earlier.guid.as_str()));
            assert!(!drain(&mut events).iter().any(|event| matches!(event, StoreEvent::Draft(_) | StoreEvent::ComposerMode(_))));
            store.stop().await;
        }
    }

    mod reading {
        use super::*;

        #[tokio::test(start_paused = true)]
        async fn pages_the_chats_it_has_not_opened_in_the_background() {
            let transport = FakeTransport::new(vec![chat("a", 2000), chat("b", 1000)]);
            for index in 0..60 {
                transport.lock().messages.push(message("b", &format!("older {index}"), index + 1, false));
            }
            let store = started(&transport, options()).await;
            settle(&store).await;
            assert_eq!(store.state().selected_chat.as_deref(), Some("a"));
            assert_eq!(store.state().messages["b"].len(), 50);
            assert_eq!(store.state().has_older.get("b"), Some(&true));
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn downloads_the_media_of_recent_messages_in_the_background() {
            let transport = FakeTransport::new(vec![chat("a", 2000), chat("b", 1000)]);
            transport.lock().messages.extend([
                media("b", "att-photo", 900, 1024, "image/jpeg"),
                media("b", "att-video", 910, 30 * 1024 * 1024, "video/quicktime"),
                media("b", "att-voice", 920, 20_000, "audio/mp4"),
                // Too big for its kind, and a file that is not media at all: both wait to be asked for.
                media("b", "att-huge", 930, 40 * 1024 * 1024, "image/jpeg"),
                media("b", "att-doc", 940, 1024, "application/pdf"),
            ]);
            let store = started(&transport, options()).await;
            tokio::time::sleep(Duration::from_millis(5000)).await;
            assert_eq!(transport.lock().attachment_calls, ["att-photo", "att-video", "att-voice"]);
            assert_eq!(store.state().messages["b"][0].attachments[0].local_path, Some(PathBuf::from("/cache/att-photo.jpg")));
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn pages_a_chat_the_socket_touched_before_it_was_opened() {
            let transport = FakeTransport::new(vec![chat("a", 2000), chat("b", 1000)]);
            for index in 0..60 {
                transport.lock().messages.push(message("b", &format!("older {index}"), index + 1, false));
            }
            let store = started(&transport, StoreOptions { warm_chats: Some(0), ..options() }).await;
            assert_eq!(store.state().selected_chat.as_deref(), Some("a"));
            let fresh = message("b", "just arrived", 5000, false);
            transport.lock().messages.push(fresh.clone());
            transport.emit(TransportEvent::Message(fresh));
            settle(&store).await;
            assert_eq!(store.state().messages["b"].len(), 1);

            store.select_chat(Some("b")).await;
            assert_eq!(store.state().messages["b"].len(), 51);
            assert_eq!(store.state().messages["b"][50].text, "just arrived");
            assert_eq!(store.state().has_older.get("b"), Some(&true));
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn keeps_chat_and_message_identity_when_a_reconcile_brings_back_the_same_data() {
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            for index in 0..5 {
                transport.lock().messages.push(message("a", &format!("row {index}"), index + 1, false));
            }
            let store = started(&transport, options()).await;
            store.engage("a");
            settle(&store).await;
            let chats_before = store.state().chats.clone();
            let rows_before = store.state().messages["a"].clone();
            let mut events = store.events();
            store.reconcile().await;
            settle(&store).await;
            let state = store.state();
            assert!(chats_before.iter().zip(&state.chats).all(|(a, b)| Arc::ptr_eq(a, b)));
            assert!(rows_before.iter().zip(&state.messages["a"]).all(|(a, b)| Arc::ptr_eq(a, b)));
            let repaints: Vec<StoreEvent> = drain(&mut events)
                .into_iter()
                .filter(|event| matches!(event, StoreEvent::ChatList | StoreEvent::Chat(_) | StoreEvent::Thread(_) | StoreEvent::Message { .. }))
                .collect();
            assert_eq!(repaints, []);
            drop(state);
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn applies_a_page_as_one_publish_rather_than_one_per_row() {
            let transport = FakeTransport::new(vec![chat("a", 2000), chat("b", 1000)]);
            for index in 0..50 {
                transport.lock().messages.push(message("b", &format!("row {index}"), index + 1, false));
            }
            let store = started(&transport, StoreOptions { warm_chats: Some(0), ..options() }).await;
            let mut events = store.events();
            store.select_chat(Some("b")).await;
            assert_eq!(store.state().messages["b"].len(), 50);
            assert!(drain(&mut events).len() < 10);
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn trims_a_conversation_once_it_is_not_the_open_one_leaving_it_pageable() {
            let transport = FakeTransport::new(vec![chat("a", 2000), chat("b", 1000)]);
            for index in 0..260 {
                transport.lock().messages.push(message("b", &format!("row {index}"), index + 1, false));
            }
            let store = started(&transport, StoreOptions { warm_chats: Some(0), ..options() }).await;
            store.select_chat(Some("b")).await;
            store.load_earlier("b").await;
            store.load_earlier("b").await;
            store.load_earlier("b").await;
            // What the open thread paged is what the reader is looking at, so it stays.
            assert_eq!(store.state().messages["b"].len(), 200);

            store.select_chat(Some("a")).await;
            assert_eq!(store.state().messages["b"].len(), 100);
            assert_eq!(store.state().messages["b"][99].text, "row 259");
            assert_eq!(store.state().has_older.get("b"), Some(&true));
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn catches_up_in_pages_of_ten() {
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let store = started(&transport, options()).await;
            transport.lock().search_calls.clear();
            let base = store.state().last_sync_at;
            for index in 0..25 {
                transport.lock().messages.push(message("a", &format!("missed {index}"), base + index + 1, false));
            }
            tokio::time::sleep(Duration::from_millis(30)).await;

            store.reconcile().await;
            assert_eq!(transport.lock().search_calls, [10, 10, 10]);
            assert_eq!(texts(&store, "a").iter().filter(|text| text.starts_with("missed")).count(), 25);
            assert!(store.state().last_sync_at > base + 25);
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn drops_a_chat_deleted_on_the_mac_and_keeps_one_the_socket_added_during_the_pass() {
            let transport = FakeTransport::new(vec![chat("a", 2000), chat("b", 1000)]);
            let store = started(&transport, StoreOptions { warm_chats: Some(0), ..options() }).await;
            {
                let mut fake = transport.lock();
                fake.chats.retain(|chat| chat.guid != "b");
                fake.action_delay_ms = 10;
            }
            let pass = store.reconcile();
            tokio::pin!(pass);
            assert!(futures_util::poll!(pass.as_mut()).is_pending());
            // The list is in flight when a chat started on the phone lands through the socket.
            tokio::time::advance(Duration::from_millis(1)).await;
            transport.emit(TransportEvent::Chat(chat("c", now_ms())));
            pass.await;
            let guids: Vec<String> = store.state().chats.iter().map(|chat| chat.guid.clone()).collect();
            assert_eq!(guids, ["c", "a"]);
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn says_so_after_three_failed_passes_and_clears_it_on_the_next() {
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let store = started(&transport, StoreOptions { warm_chats: Some(0), ..options() }).await;
            transport.lock().list_failure = Some(TransportError::Network("fetch failed".into()));
            let mut events = store.events();
            store.reconcile().await;
            store.reconcile().await;
            assert_eq!(store.state().connection_error, None);
            store.reconcile().await;
            assert_eq!(store.state().status, ConnectionStatus::Online);
            assert_eq!(store.state().connection_error.as_deref(), Some("Could not refresh from the Mac."));
            assert!(drain(&mut events).contains(&StoreEvent::Connection));

            transport.lock().list_failure = None;
            store.reconcile().await;
            assert_eq!(store.state().connection_error, None);
            assert!(drain(&mut events).contains(&StoreEvent::Connection));
            store.stop().await;
        }
    }

    mod granularity {
        use super::*;

        #[tokio::test(start_paused = true)]
        async fn typing_touches_only_its_own_chat() {
            let transport = FakeTransport::new(vec![chat("a", 2000), chat("b", 1000)]);
            let store = started(&transport, StoreOptions { warm_chats: Some(0), ..options() }).await;
            settle(&store).await;
            let mut events = store.events();
            transport.emit(TransportEvent::Typing { chat_guid: "b".into(), typing: true });
            settle(&store).await;
            assert_eq!(drain(&mut events), [StoreEvent::Typing("b".into())]);
            store.set_draft("a", "h");
            assert_eq!(drain(&mut events), [StoreEvent::Draft("a".into())]);
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn a_message_in_one_chat_leaves_the_other_alone() {
            let transport = FakeTransport::new(vec![chat("a", 2000), chat("b", 1000)]);
            let store = started(&transport, StoreOptions { warm_chats: Some(0), ..options() }).await;
            settle(&store).await;
            let mut events = store.events();
            transport.emit(TransportEvent::Message(message("b", "hi", 1500, false)));
            settle(&store).await;
            let seen = drain(&mut events);
            assert!(seen.contains(&StoreEvent::Thread("b".into())));
            assert!(seen.contains(&StoreEvent::Chat("b".into())));
            assert!(!seen.contains(&StoreEvent::ChatList), "b stays below a, so the list keeps its order");
            assert!(!seen.iter().any(|event| matches!(event, StoreEvent::Chat(guid) | StoreEvent::Thread(guid) if guid == "a")));

            // Newer than a: the row moves, which the list has to know about.
            transport.emit(TransportEvent::Message(message("b", "again", 3000, false)));
            settle(&store).await;
            assert!(drain(&mut events).contains(&StoreEvent::ChatList));
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn a_tapback_repaints_one_message() {
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let target = message("a", "hello", 900, false);
            transport.lock().messages.push(target.clone());
            let store = started(&transport, options()).await;
            settle(&store).await;
            let mut events = store.events();
            let reaction = Message {
                guid: "reaction-1".into(),
                reaction: Some(Reaction { target_guid: target.guid.clone(), kind: TapbackKind::Love, emoji: None, removed: false }),
                ..message("a", "", 950, true)
            };
            transport.emit(TransportEvent::Message(reaction));
            settle(&store).await;
            assert_eq!(drain(&mut events), [StoreEvent::Message { chat_guid: "a".into(), message_guid: target.guid.clone() }]);
            store.stop().await;
        }
    }

    mod conversations {
        use super::*;
        use crate::conversations::conversation_chats;

        #[tokio::test(start_paused = true)]
        async fn folds_two_chats_with_the_same_person_into_one_replies_to_the_newer_one_and_reads_both() {
            let mut phone = chat("any;-;+32470000001", 5000);
            let mut email = chat("any;-;papa@example.com", 1000);
            phone.participants = vec![handle("+32470000001")];
            email.participants = vec![handle("papa@example.com")];
            email.unread = true;
            let transport = FakeTransport::new(vec![phone.clone(), email.clone(), chat("c", 100)]);
            transport
                .lock()
                .messages
                .extend([message(&email.guid, "from the email", 900, false), message(&phone.guid, "from the phone", 4900, false)]);
            let store = started(&transport, options()).await;
            transport.emit(TransportEvent::Contacts(vec![Contact {
                id: "papa".into(),
                name: "Papa".into(),
                addresses: vec!["+32 470 00 00 01".into(), "papa@example.com".into()],
                avatar: None,
            }]));
            settle(&store).await;

            assert_eq!(store.state().grouping.merged.get(&phone.guid), Some(&vec![phone.guid.clone(), email.guid.clone()]));
            assert_eq!(store.state().grouping.primary_of.get(&email.guid), Some(&phone.guid));
            store.select_chat(Some(&email.guid)).await;
            assert_eq!(store.state().selected_chat.as_ref(), Some(&phone.guid));
            let merged: Vec<String> = conversation_messages(&store.state(), &phone.guid).iter().map(|item| item.text.clone()).collect();
            assert_eq!(merged, ["from the email", "from the phone"]);
            let rows: Vec<String> = conversation_chats(&store.state()).iter().map(|item| item.guid.clone()).collect();
            assert_eq!(rows, [phone.guid.clone(), "c".to_owned()]);
            store.engage(&email.guid);
            settle(&store).await;
            assert!(!unread(&store, &email.guid));

            store.send(&phone.guid, "hello", None);
            settle(&store).await;
            assert_eq!(transport.lock().messages.last().map(|item| item.chat_guid.clone()), Some(phone.guid.clone()));
            store.stop().await;
        }
    }

    mod attachments {
        use super::*;

        #[tokio::test(start_paused = true)]
        async fn keeps_the_size_read_from_the_file_header_when_the_server_sends_the_message_again() {
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let mut server = media("a", "att-1", 5000, 1000, "image/gif");
            // chat.db reports the pixels the way they are stored, so a portrait photo arrives as a landscape box.
            server.attachments[0].width = Some(1200);
            server.attachments[0].height = Some(800);
            transport.lock().messages = vec![server.clone()];
            let bytes = [b"GIF89a".as_slice(), &[240, 0, 180, 0]].concat();
            let source = format!("data:image/gif;base64,{}", base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes));
            transport.lock().attachment_source = Some(source.clone());
            let store = started(&transport, StoreOptions { warm_chats: Some(0), ..options() }).await;
            store.select_chat(Some("a")).await;
            store.attachment_src("a", &server.guid, "att-1", "clip.gif", Some("image/gif")).await.unwrap();
            let shown = |store: &MessagesStore| store.state().messages["a"][0].attachments[0].clone();
            let first = shown(&store);
            assert_eq!((first.width, first.height, first.measured), (Some(240), Some(180), true));

            store.apply_message(server, true, true);
            let again = shown(&store);
            assert_eq!((again.width, again.height), (Some(240), Some(180)));
            assert_eq!(again.local_path, Some(PathBuf::from(source)));
            store.stop().await;
        }
    }

    mod read_receipts {
        use super::*;

        #[tokio::test(start_paused = true)]
        async fn tells_the_server_when_a_message_lands_in_the_open_thread() {
            let transport = FakeTransport::new(vec![chat("a", 2000), chat("b", 1000)]);
            transport.lock().server_info = private_api();
            let store = started(&transport, StoreOptions { warm_chats: Some(0), ..options() }).await;
            assert_eq!(store.state().selected_chat.as_deref(), Some("a"));

            // Open but untouched: the message stays unread so the phone still rings.
            transport.emit(TransportEvent::Message(message("a", "idle", 2500, false)));
            settle(&store).await;
            assert!(unread(&store, "a"));
            assert!(transport.lock().mark_read_calls.is_empty());

            store.engage("a");
            settle(&store).await;
            assert!(!unread(&store, "a"));
            assert_eq!(transport.lock().mark_read_calls, ["a"]);

            transport.emit(TransportEvent::Message(message("a", "hi", 3000, false)));
            settle(&store).await;
            assert!(!unread(&store, "a"));
            assert_eq!(transport.lock().mark_read_calls, ["a", "a"]);

            // A minute without touching the thread and it stops reading.
            tokio::time::advance(ENGAGED_FOR).await;
            transport.emit(TransportEvent::Message(message("a", "still there?", 3500, false)));
            settle(&store).await;
            assert!(unread(&store, "a"));
            assert_eq!(transport.lock().mark_read_calls, ["a", "a"]);

            // One for a thread that is not open waits for the thread to be used.
            transport.emit(TransportEvent::Message(message("b", "later", 4000, false)));
            settle(&store).await;
            assert!(unread(&store, "b"));
            assert_eq!(transport.lock().mark_read_calls, ["a", "a"]);
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn clears_the_dot_when_the_newest_message_comes_back_read_on_another_device() {
            let transport = FakeTransport::new(vec![chat("a", 2000), chat("b", 1000)]);
            let store = started(&transport, StoreOptions { warm_chats: Some(0), ..options() }).await;
            let incoming = message("b", "hi", 3000, false);
            transport.emit(TransportEvent::Message(incoming.clone()));
            settle(&store).await;
            assert!(unread(&store, "b"));

            transport.emit(TransportEvent::Message(Message { date_read: Some(3500), ..incoming }));
            settle(&store).await;
            assert!(!unread(&store, "b"));
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn a_chat_read_here_stays_read_while_the_mac_still_says_unread_until_something_new_arrives() {
            // No private API: the Mac never records the read, so every list it serves keeps the newest incoming message unread.
            let transport = FakeTransport::new(vec![chat("a", 2000), Chat { unread: true, ..chat("b", 1000) }]);
            transport.lock().messages.push(Message { date_read: None, ..message("b", "hey", 1500, false) });
            let store = started(&transport, StoreOptions { warm_chats: Some(0), ..options() }).await;
            assert!(unread(&store, "b"));

            store.open_chat("b").await;
            settle(&store).await;
            assert!(!unread(&store, "b"));
            store.reconcile().await;
            assert!(!unread(&store, "b"), "the list came back and put the dot back");

            transport.lock().messages.push(message("b", "one more", now_ms() + 1000, false));
            store.select_chat(Some("a")).await;
            store.reconcile().await;
            assert!(unread(&store, "b"), "a newer message is unread again");
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn keeps_a_chat_marked_unread_across_a_re_read_of_the_list() {
            let transport = FakeTransport::new(vec![chat("a", 2000), chat("b", 1000)]);
            let store = started(&transport, StoreOptions { warm_chats: Some(0), ..options() }).await;
            store.mark_unread("b").await;
            store.reconcile().await;
            assert!(unread(&store, "b"));
            store.mark_read("b").await;
            store.reconcile().await;
            assert!(!unread(&store, "b"));
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn turns_them_off_without_telling_the_server_and_back_on_again() {
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            transport.lock().server_info = private_api();
            let store = started(&transport, options()).await;

            store.toggle_read_receipts("a");
            assert_eq!(store.state().chat("a").unwrap().read_receipts, Some(false));
            store.mark_unread("a").await;
            store.mark_read("a").await;
            assert!(!unread(&store, "a"));
            assert!(transport.lock().mark_read_calls.is_empty());

            store.toggle_read_receipts("a");
            store.mark_unread("a").await;
            store.mark_read("a").await;
            assert_eq!(transport.lock().mark_read_calls, ["a"]);
            store.stop().await;
        }
    }

    mod restarting {
        use super::*;

        /// Two chats worth thirty rows each, one of them carrying a photo.
        fn seeded() -> Arc<FakeTransport> {
            let transport = FakeTransport::new(vec![chat("a", 2000), chat("b", 1000)]);
            {
                let mut fake = transport.lock();
                for index in 0..30 {
                    fake.messages.push(message("a", &format!("a {index}"), index + 1, false));
                    fake.messages.push(message("b", &format!("b {index}"), index + 1, false));
                }
                fake.messages.push(media("b", "att-photo", 900, 1024, "image/jpeg"));
            }
            transport
        }

        #[tokio::test(start_paused = true)]
        async fn comes_back_on_the_cache_instead_of_pulling_the_threads_and_their_media_again() {
            let dir = std::env::temp_dir().join(format!("messages-store-cache-{}-{}", std::process::id(), fastrand::u64(..)));
            let first = seeded();
            let cache = Arc::new(StateCache::new(&dir));
            let store = started(&first, StoreOptions { cache: Some(cache.clone()), ..options() }).await;
            tokio::time::sleep(Duration::from_millis(5000)).await;
            assert_eq!(first.lock().attachment_calls, ["att-photo"]);
            assert_eq!(store.state().messages["b"].len(), 31);
            cache.flush().await;
            store.stop().await;

            let second = seeded();
            let restarted = store_with(&second, StoreOptions { cache: Some(Arc::new(StateCache::new(&dir))), ..options() });
            restarted.start().await;
            tokio::time::sleep(Duration::from_millis(5000)).await;

            // Both threads came off disk, so the only page read is the reconcile's
            // safety net over the open one, and no attachment is fetched twice.
            assert_eq!(second.lock().load_calls, ["a"]);
            assert!(second.lock().attachment_calls.is_empty());
            assert_eq!(restarted.state().messages["b"].len(), 31);
            assert_eq!(restarted.state().messages["b"].last().unwrap().attachments[0].local_path, Some(PathBuf::from("/cache/att-photo.jpg")));
            restarted.stop().await;
            std::fs::remove_dir_all(&dir).ok();
        }
    }

    mod chat_actions {
        use super::*;

        #[tokio::test(start_paused = true)]
        async fn drops_a_deleted_conversation_at_once_and_brings_it_back_when_the_server_refuses() {
            let transport = FakeTransport::new(vec![chat("a", 2000), chat("b", 1000)]);
            let store = started(&transport, options()).await;
            {
                let mut fake = transport.lock();
                fake.delete_failure = Some(TransportError::Server { status: 500, message: "chat.db is locked".into() });
                fake.action_delay_ms = 10;
            }
            let pending = store.delete_chat("a");
            tokio::pin!(pending);
            assert!(futures_util::poll!(pending.as_mut()).is_pending());
            let guids = |store: &MessagesStore| store.state().chats.iter().map(|chat| chat.guid.clone()).collect::<Vec<_>>();
            assert_eq!(guids(&store), ["b"]);
            assert_eq!(store.state().selected_chat.as_deref(), Some("b"));
            pending.await;
            assert_eq!(guids(&store), ["a", "b"]);
            assert_eq!(store.state().error.as_deref(), Some("chat.db is locked"));
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn renames_a_group_before_the_server_answers_and_reverts_when_it_fails() {
            let group = Chat { is_group: true, display_name: Some("Old".into()), participants: vec![handle("x"), handle("y")], ..chat("g", 1000) };
            let transport = FakeTransport::new(vec![group]);
            let store = started(&transport, options()).await;
            {
                let mut fake = transport.lock();
                fake.rename_failure = Some(TransportError::Server { status: 400, message: "the server said no".into() });
                fake.action_delay_ms = 10;
            }
            let pending = store.rename_group("g", "New");
            tokio::pin!(pending);
            assert!(futures_util::poll!(pending.as_mut()).is_pending());
            assert_eq!(store.state().chats[0].display_name.as_deref(), Some("New"));
            pending.await;
            assert_eq!(store.state().chats[0].display_name.as_deref(), Some("Old"));
            assert_eq!(store.state().error.as_deref(), Some("the server said no"));
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn selecting_a_conversation_leaves_it_unread_until_it_is_used() {
            let transport = FakeTransport::new(vec![chat("a", 2000), Chat { unread: true, ..chat("b", 1000) }]);
            let store = started(&transport, StoreOptions { warm_chats: Some(0), ..options() }).await;
            store.select_chat(Some("b")).await;
            assert!(unread(&store, "b"));
            store.engage("b");
            settle(&store).await;
            assert!(!unread(&store, "b"));
            store.stop().await;
        }
    }

    mod focus {
        use super::*;

        #[tokio::test(start_paused = true)]
        async fn asks_about_the_open_conversation_keeps_the_answer_and_breaks_through_it() {
            let transport = FakeTransport::new(vec![chat("+15550101", 1000)]);
            let mine = Message { date_delivered: Some(11), delivered_quietly: Some(true), ..message("+15550101", "you up", 10, true) };
            {
                let mut fake = transport.lock();
                fake.server_info = private_api();
                fake.messages = vec![mine.clone()];
                fake.focus = FocusStatus::Silenced;
            }
            let store = started(&transport, StoreOptions { warm_chats: Some(0), ..options() }).await;
            settle(&store).await;

            assert_eq!(transport.lock().focus_calls, ["+15550101"]);
            assert_eq!(conversation_focus(&store.state(), "+15550101"), FocusStatus::Silenced);

            // The TTL holds: reopening the same thread does not ask again.
            store.select_chat(Some("+15550101")).await;
            settle(&store).await;
            assert_eq!(transport.lock().focus_calls.len(), 1);

            store.notify_silenced("+15550101", &mine.guid).await;
            assert_eq!(transport.lock().notify_calls, [mine.guid.clone()]);
            assert_eq!(store.state().messages["+15550101"][0].notified, Some(true));
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn never_asks_when_the_private_api_is_off_and_puts_the_message_back_when_the_notify_fails() {
            let transport = FakeTransport::new(vec![chat("+15550101", 1000)]);
            let mine = Message { date_delivered: Some(11), delivered_quietly: Some(true), ..message("+15550101", "you up", 10, true) };
            {
                let mut fake = transport.lock();
                fake.messages = vec![mine.clone()];
                fake.focus = FocusStatus::Silenced;
            }
            let store = started(&transport, StoreOptions { warm_chats: Some(0), ..options() }).await;
            settle(&store).await;
            assert!(transport.lock().focus_calls.is_empty());
            assert_eq!(conversation_focus(&store.state(), "+15550101"), FocusStatus::Unknown);

            transport.lock().notify_failure = Some(TransportError::Network("helper is gone".into()));
            store.notify_silenced("+15550101", &mine.guid).await;
            assert_eq!(store.state().messages["+15550101"][0].notified, None);
            assert_eq!(store.state().error.as_deref(), Some("helper is gone"));
            store.stop().await;
        }
    }

    mod tapbacks {
        use super::*;

        #[tokio::test(start_paused = true)]
        async fn counts_a_reaction_the_server_sends_twice_once_without_its_handle_as_one() {
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let target = message("a", "hello", 900, false);
            transport.lock().messages.push(target.clone());
            let store = started(&transport, options()).await;
            settle(&store).await;

            let reaction = Message {
                guid: "reaction-1".into(),
                reaction: Some(Reaction { target_guid: target.guid.clone(), kind: TapbackKind::Emoji, emoji: Some("🔥".into()), removed: false }),
                ..message("a", "", 950, false)
            };
            transport.emit(TransportEvent::Message(Message { sender: None, ..reaction.clone() }));
            transport.emit(TransportEvent::Message(Message { sender: Some(handle("a")), ..reaction.clone() }));
            transport.emit(TransportEvent::Message(Message { sender: None, ..reaction }));
            settle(&store).await;

            let tapbacks = store.state().find_message("a", &target.guid).unwrap().tapbacks.clone();
            assert_eq!(
                tapbacks,
                [Tapback { guid: "reaction-1".into(), kind: TapbackKind::Emoji, emoji: Some("🔥".into()), from_me: false, sender: Some(handle("a")) }]
            );
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn keeps_two_people_who_sent_the_same_emoji_apart() {
            let group = Chat { is_group: true, participants: vec![handle("a"), handle("b")], ..chat("g", 1000) };
            let transport = FakeTransport::new(vec![group]);
            let target = message("g", "hello", 900, false);
            transport.lock().messages.push(target.clone());
            let store = started(&transport, options()).await;
            settle(&store).await;

            let detail = Reaction { target_guid: target.guid.clone(), kind: TapbackKind::Love, emoji: None, removed: false };
            let reaction = |guid: &str, date: Millis, sender: Option<&str>, removed: bool| Message {
                guid: guid.into(),
                reaction: Some(Reaction { removed, ..detail.clone() }),
                sender: sender.map(handle),
                ..message("g", "", date, false)
            };
            transport.emit(TransportEvent::Message(reaction("reaction-a", 950, Some("a"), false)));
            transport.emit(TransportEvent::Message(reaction("reaction-b", 960, Some("b"), false)));
            transport.emit(TransportEvent::Message(reaction("reaction-c", 970, None, false)));
            transport.emit(TransportEvent::Message(reaction("reaction-d", 980, Some("b"), true)));
            settle(&store).await;

            let guids: Vec<String> = store.state().find_message("g", &target.guid).unwrap().tapbacks.iter().map(|item| item.guid.clone()).collect();
            assert_eq!(guids, ["reaction-a", "reaction-c"]);
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn folds_a_reaction_that_arrived_before_its_target() {
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let store = started(&transport, options()).await;
            let target = message("a", "late", 900, false);
            let early = Message {
                guid: "reaction-early".into(),
                reaction: Some(Reaction { target_guid: target.guid.clone(), kind: TapbackKind::Like, emoji: None, removed: false }),
                ..message("a", "", 950, false)
            };
            transport.emit(TransportEvent::Message(early));
            transport.emit(TransportEvent::Message(target.clone()));
            settle(&store).await;
            let tapbacks = store.state().find_message("a", &target.guid).unwrap().tapbacks.clone();
            assert_eq!(tapbacks.len(), 1);
            assert_eq!(tapbacks[0].sender, Some(handle("a")));
            store.stop().await;
        }
    }

    mod scheduled {
        use super::*;

        fn schedule(id: &str, send_at: Millis) -> ScheduledMessage {
            ScheduledMessage { id: id.into(), chat_guid: "chat-1".into(), text: "later".into(), send_at }
        }

        #[tokio::test(start_paused = true)]
        async fn start_refreshes_the_scheduled_list_from_the_transport() {
            let transport = FakeTransport::new(vec![chat("chat-1", 1000)]);
            transport.lock().scheduled = vec![schedule("sched-a", now_ms() + 3_600_000)];
            let store = started(&transport, options()).await;
            assert_eq!(store.state().scheduled, transport.lock().scheduled);
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn schedule_send_clears_the_draft_reply_and_edit_state() {
            let transport = FakeTransport::new(vec![chat("chat-1", 1000)]);
            let store = started(&transport, options()).await;
            store.set_draft("chat-1", "see you then");
            store.set_replying_to("chat-1", Some("msg-1"));
            store.schedule_send("chat-1", "see you then", now_ms() + 3_600_000).await;
            let state = store.state();
            assert_eq!(state.drafts.get("chat-1").map(String::as_str), Some(""));
            assert!(!state.replying_to.contains_key("chat-1"));
            assert!(!state.editing.contains_key("chat-1"));
            assert_eq!(state.scheduled.len(), 1);
            assert_eq!(state.scheduled[0].text, "see you then");
            drop(state);
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn schedule_send_ignores_whitespace() {
            let transport = FakeTransport::new(vec![chat("chat-1", 1000)]);
            let store = started(&transport, options()).await;
            store.schedule_send("chat-1", "   ", now_ms() + 3_600_000).await;
            assert!(store.state().scheduled.is_empty());
            store.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn cancel_removes_and_restores_when_the_transport_fails() {
            let transport = FakeTransport::new(vec![chat("chat-1", 1000)]);
            let store = started(&transport, options()).await;
            store.schedule_send("chat-1", "later", now_ms() + 3_600_000).await;
            let id = store.state().scheduled[0].id.clone();
            store.cancel_scheduled(&id).await;
            assert!(store.state().scheduled.is_empty());

            store.schedule_send("chat-1", "later", now_ms() + 3_600_000).await;
            let id = store.state().scheduled[0].id.clone();
            transport.lock().cancel_failure = Some(TransportError::Network("offline".into()));
            store.cancel_scheduled(&id).await;
            assert_eq!(store.state().scheduled.len(), 1);
            assert_eq!(store.state().error.as_deref(), Some("offline"));
            store.stop().await;
        }
    }

    mod gif_favorites {
        use super::*;

        #[tokio::test(start_paused = true)]
        async fn favorites_a_gif_and_lists_it_newest_first_then_unfavorites_it_with_a_tombstone() {
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let store = started(&transport, options()).await;
            store.toggle_gif_favorite(&gif("one"));
            tokio::time::sleep(Duration::from_millis(1)).await;
            store.toggle_gif_favorite(&gif("two"));
            let ids = |store: &MessagesStore| favorite_gifs(&store.state().gif_favorites).into_iter().map(|gif| gif.id).collect::<Vec<_>>();
            assert_eq!(ids(&store), ["two", "one"]);

            store.toggle_gif_favorite(&gif("one"));
            assert_eq!(ids(&store), ["two"]);
            assert!(store.state().gif_favorites["one"].removed);
            store.stop().await;
        }
    }

    /// A stand-in for `@messages/mac-agent`: answers `/prefs`, `/findmy/friends`
    /// and `/findmy/stream` over plain HTTP/1.1 and records what it was sent.
    struct AgentMock {
        config: AgentConfig,
        requests: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
        next_prefs: Arc<Mutex<VecDeque<serde_json::Value>>>,
    }

    impl AgentMock {
        async fn start(prefs: serde_json::Value, friends: serde_json::Value, stream: Option<serde_json::Value>) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let requests: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Arc::default();
            let next_prefs: Arc<Mutex<VecDeque<serde_json::Value>>> = Arc::default();
            let (log, queue) = (requests.clone(), next_prefs.clone());
            tokio::spawn(async move {
                while let Ok((mut socket, _)) = listener.accept().await {
                    let (log, queue, prefs, friends, stream) = (log.clone(), queue.clone(), prefs.clone(), friends.clone(), stream.clone());
                    tokio::spawn(async move {
                        let mut buffer = Vec::new();
                        let mut chunk = [0u8; 4096];
                        let header_end = loop {
                            let Ok(read) = socket.read(&mut chunk).await else { return };
                            if read == 0 {
                                return;
                            }
                            buffer.extend_from_slice(&chunk[..read]);
                            if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                                break end + 4;
                            }
                        };
                        let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
                        let length = head
                            .lines()
                            .find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").map(|value| value.trim().parse::<usize>().unwrap_or(0)))
                            .unwrap_or(0);
                        while buffer.len() < header_end + length {
                            let Ok(read) = socket.read(&mut chunk).await else { return };
                            if read == 0 {
                                break;
                            }
                            buffer.extend_from_slice(&chunk[..read]);
                        }
                        let path = head.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap_or("/").to_owned();
                        let body = serde_json::from_slice(&buffer[header_end..]).unwrap_or(serde_json::Value::Null);
                        log.lock().push((path.clone(), body));
                        if path.ends_with("/findmy/stream") {
                            let Some(snapshot) = stream else {
                                let _ = socket.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                                return;
                            };
                            let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\r\n").await;
                            let _ = socket.write_all(format!("data: {snapshot}\n\n").as_bytes()).await;
                            let _ = socket.flush().await;
                            tokio::time::sleep(Duration::from_secs(30)).await;
                            return;
                        }
                        let answer = if path.ends_with("/prefs") {
                            queue.lock().pop_front().unwrap_or(prefs)
                        } else if path.ends_with("/findmy/friends") {
                            serde_json::json!({ "friends": friends, "updatedAt": 0 })
                        } else {
                            serde_json::json!({})
                        };
                        let text = answer.to_string();
                        let response =
                            format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
                        let _ = socket.write_all(response.as_bytes()).await;
                    });
                }
            });
            Self { config: AgentConfig { url, token: "tok".into() }, requests, next_prefs }
        }

        fn prefs_bodies(&self) -> Vec<serde_json::Value> {
            self.requests.lock().iter().filter(|(path, _)| path.ends_with("/prefs")).map(|(_, body)| body.clone()).collect()
        }

        fn answer_next_prefs(&self, answer: serde_json::Value) {
            self.next_prefs.lock().push_back(answer);
        }
    }

    fn no_pins() -> serde_json::Value {
        serde_json::json!({ "chats": {}, "gifs": {}, "macPinned": [], "macPinnedAt": null })
    }

    async fn with_agent(transport: &Arc<FakeTransport>, agent: &AgentMock) -> MessagesStore {
        let store = started(transport, StoreOptions { warm_chats: Some(0), agent: Some(agent.config.clone()), ..options() }).await;
        // The syncs `start` and the connection spawned have answered before a test queues its own answer.
        tokio::time::sleep(Duration::from_millis(300)).await;
        store
    }

    mod agent {
        use super::*;

        #[tokio::test]
        async fn leaves_a_chat_pinned_in_messages_app_pinned_while_it_is_typed_in_and_still_unpins_on_request() {
            let agent = AgentMock::start(serde_json::json!({ "chats": {}, "gifs": {}, "macPinned": ["a"], "macPinnedAt": 100 }), serde_json::json!([]), None).await;
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let store = with_agent(&transport, &agent).await;
            wait_for("the Mac pin", || store.state().chats[0].pinned).await;

            store.set_draft("a", "on my way");
            tokio::time::sleep(Duration::from_millis(2300)).await;
            assert!(store.state().chats[0].pinned);

            store.toggle_pin("a");
            assert!(!store.state().chats[0].pinned);
            store.stop().await;
        }

        #[tokio::test]
        async fn debounces_a_local_edit_2s_before_syncing_and_clears_the_synced_draft_once_the_message_sends() {
            let agent = AgentMock::start(no_pins(), serde_json::json!([]), None).await;
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let store = with_agent(&transport, &agent).await;
            agent.requests.lock().clear();

            store.set_draft("a", "hey there");
            tokio::time::sleep(Duration::from_millis(1500)).await;
            assert!(agent.prefs_bodies().is_empty());
            wait_for("the draft sync", || agent.prefs_bodies().len() == 1).await;
            assert_eq!(agent.prefs_bodies()[0]["chats"]["a"]["draft"], "hey there");

            store.send("a", "hey there", None);
            wait_for("the cleared draft", || agent.prefs_bodies().last().is_some_and(|body| body["chats"]["a"]["draft"] == "")).await;
            store.stop().await;
        }

        #[tokio::test]
        async fn puts_a_newer_remote_draft_into_the_composer_when_the_local_box_is_empty() {
            let agent = AgentMock::start(no_pins(), serde_json::json!([]), None).await;
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let store = with_agent(&transport, &agent).await;
            agent.answer_next_prefs(serde_json::json!({ "chats": { "a": { "draft": "from my phone", "updatedAt": now_ms() } }, "macPinned": [], "macPinnedAt": null }));
            store.sync_prefs().await;
            assert_eq!(store.state().drafts.get("a").map(String::as_str), Some("from my phone"));
            store.stop().await;
        }

        #[tokio::test]
        async fn never_lets_a_stale_remote_draft_that_arrives_late_overwrite_text_typed_more_recently() {
            let agent = AgentMock::start(no_pins(), serde_json::json!([]), None).await;
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let store = with_agent(&transport, &agent).await;
            let edited_at = now_ms();
            store.set_draft("a", "local text");
            agent.answer_next_prefs(
                serde_json::json!({ "chats": { "a": { "draft": "their text", "updatedAt": edited_at - 1000 } }, "macPinned": [], "macPinnedAt": null }),
            );
            store.sync_prefs().await;
            assert_eq!(store.state().drafts.get("a").map(String::as_str), Some("local text"));
            store.stop().await;
        }

        #[tokio::test]
        async fn syncs_gif_favorites_through_the_agent_and_merges_a_newer_remote_entry_in() {
            let agent = AgentMock::start(no_pins(), serde_json::json!([]), None).await;
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let store = with_agent(&transport, &agent).await;
            store.toggle_gif_favorite(&gif("one"));
            wait_for("the favorite sync", || agent.prefs_bodies().last().is_some_and(|body| body["gifs"]["one"]["gif"]["id"] == "one")).await;
            tokio::time::sleep(Duration::from_millis(100)).await;

            agent.answer_next_prefs(serde_json::json!({
                "chats": {}, "macPinned": [], "macPinnedAt": null,
                "gifs": { "two": { "gif": serde_json::to_value(gif("two")).unwrap(), "updatedAt": now_ms() + 1000 } },
            }));
            store.sync_prefs().await;
            let ids: Vec<String> = favorite_gifs(&store.state().gif_favorites).into_iter().map(|gif| gif.id).collect();
            assert_eq!(ids, ["two", "one"]);
            store.stop().await;
        }

        #[tokio::test]
        async fn never_lets_a_stale_remote_favorite_overwrite_a_newer_local_unfavorite() {
            let agent = AgentMock::start(no_pins(), serde_json::json!([]), None).await;
            let transport = FakeTransport::new(vec![chat("a", 1000)]);
            let store = with_agent(&transport, &agent).await;
            store.toggle_gif_favorite(&gif("one"));
            store.toggle_gif_favorite(&gif("one"));
            tokio::time::sleep(Duration::from_millis(200)).await;
            let local = store.state().gif_favorites["one"].updated_at;

            agent.answer_next_prefs(serde_json::json!({
                "chats": {}, "macPinned": [], "macPinnedAt": null,
                "gifs": { "one": { "gif": serde_json::to_value(gif("one")).unwrap(), "updatedAt": local - 500 } },
            }));
            store.sync_prefs().await;
            let entry = store.state().gif_favorites["one"].clone();
            assert!(entry.removed);
            assert_eq!(entry.updated_at, local);
            store.stop().await;
        }

        fn friend(latitude: f64) -> serde_json::Value {
            serde_json::json!({ "id": "f1", "addresses": ["+34600111222"], "latitude": latitude, "longitude": -15.4, "timestamp": 1, "isSharing": true })
        }

        #[tokio::test]
        async fn takes_a_pushed_location_while_the_details_panel_is_open_without_waiting_for_the_poll() {
            let agent = AgentMock::start(no_pins(), serde_json::json!([]), Some(serde_json::json!({ "friends": [friend(28.1)], "devices": null, "updatedAt": 5 }))).await;
            let transport = FakeTransport::new(vec![chat("+34600111222", 1000)]);
            let store = with_agent(&transport, &agent).await;

            store.set_details_open(true);
            wait_for("the pushed location", || store.state().locations.get("600111222").is_some_and(|friend| friend.latitude == 28.1)).await;
            assert_eq!(store.state().find_my, FindMyState::Ok);
            store.set_details_open(false);
            store.stop().await;
        }

        #[tokio::test]
        async fn leaves_the_last_known_locations_alone_when_a_snapshot_carries_no_friends() {
            let agent =
                AgentMock::start(no_pins(), serde_json::json!([friend(28.9)]), Some(serde_json::json!({ "friends": null, "devices": [], "updatedAt": 5 }))).await;
            let transport = FakeTransport::new(vec![chat("+34600111222", 1000)]);
            let store = with_agent(&transport, &agent).await;
            wait_for("the polled location", || store.state().locations.get("600111222").is_some_and(|friend| friend.latitude == 28.9)).await;

            store.set_details_open(true);
            tokio::time::sleep(Duration::from_millis(300)).await;
            assert_eq!(store.state().locations["600111222"].latitude, 28.9);
            store.set_details_open(false);
            store.stop().await;
        }
    }

    mod demo {
        use super::*;
        use crate::conversations::conversation_chats;
        use crate::demo::DemoTransport;

        #[tokio::test(start_paused = true)]
        async fn runs_on_the_fixtures_and_answers_a_send() {
            let store = MessagesStore::new(Arc::new(DemoTransport::new()), StoreOptions::default(), tokio::runtime::Handle::current());
            store.start().await;
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(store.state().status, ConnectionStatus::Online);
            assert_eq!(store.state().selected_chat.as_deref(), Some("iMessage;-;+14155550134"));
            assert_eq!(conversation_chats(&store.state()).len(), 9);
            store.send("iMessage;-;+14155550134", "on my way", None);
            tokio::time::sleep(Duration::from_millis(4000)).await;
            let rows = texts(&store, "iMessage;-;+14155550134");
            let sent = rows.iter().position(|text| text == "on my way").unwrap();
            assert_eq!(rows[sent + 1], "ha, deal");
            let state = store.state();
            let mine = state.messages["iMessage;-;+14155550134"].iter().find(|message| message.text == "on my way").unwrap();
            assert!(mine.date_read.is_some() && !mine.guid.starts_with("temp-"));
            drop(state);
            store.stop().await;
        }
    }
}
