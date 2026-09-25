//! Port of packages/core/src/transport.ts.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::model::{Chat, Contact, FocusStatus, Handle, Message, Millis, ScheduledMessage, ServerInfo, Service, TapbackKind};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ConnectionStatus {
    #[default]
    Connecting,
    Online,
    Offline,
}

/// `Server` is a definite answer (a 4xx or 5xx envelope) and is never retried.
/// `Network` is anything else a request throws, a dropped socket or a timeout, and is.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    #[error("{message}")]
    Server { status: u16, message: String },
    #[error("{0}")]
    Network(String),
}

impl TransportError {
    /// Port of `isRetryable`.
    pub fn is_retryable(&self) -> bool {
        matches!(self, TransportError::Network(_))
    }
}

pub type TransportResult<T> = Result<T, TransportError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceTimeStatus {
    Incoming,
    Ended,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TransportEvent {
    Connection { status: ConnectionStatus, error: Option<String> },
    Server(ServerInfo),
    /// A new or updated message. Reactions arrive here too, with `reaction` set.
    Message(Message),
    Chat(Chat),
    ChatRemoved { chat_guid: String },
    /// The address book, once the transport has fetched it.
    Contacts(Vec<Contact>),
    Typing { chat_guid: String, typing: bool },
    Read { chat_guid: String, read: bool },
    /// `can_answer` is false on the legacy `incoming-facetime` path, which only names the caller.
    FaceTime { call_uuid: String, status: FaceTimeStatus, from: Option<Handle>, can_answer: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub has_more: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListChatsOptions {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadMessagesOptions {
    pub limit: u32,
    pub before: Option<Millis>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SendTextOptions {
    pub reply_to: Option<String>,
    pub effect: Option<String>,
    pub subject: Option<String>,
    pub temp_guid: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SendAttachmentOptions {
    pub name: Option<String>,
    pub is_audio: bool,
    pub temp_guid: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AttachmentFilter {
    Image,
    Video,
    File,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchFilters {
    pub chat_guid: Option<String>,
    /// From an `in:` filter. Some(empty) (no chat matched) returns nothing, unlike None.
    pub chat_guids: Option<Vec<String>>,
    pub limit: Option<u32>,
    /// Exclusive lower bound. The reconcile sweep uses this with an empty query and no `before`.
    pub after: Option<Millis>,
    /// Inclusive upper bound.
    pub before: Option<Millis>,
    pub from_me: bool,
    /// Addresses, already resolved from a `from:` filter's name or address.
    pub senders: Vec<String>,
    pub attachments: Option<AttachmentFilter>,
    pub links: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttachmentPathOptions {
    pub name: Option<String>,
    pub mime: Option<String>,
    pub sticker: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReactOptions {
    pub emoji: Option<String>,
    pub remove: bool,
    pub part_index: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditOptions {
    pub part_index: Option<u32>,
    pub backwards_compat_text: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportKind {
    BlueBubbles,
    Demo,
}

#[async_trait]
pub trait Transport: Send + Sync + 'static {
    fn kind(&self) -> TransportKind;

    /// Resolves once the server answered and the event stream is open. Fails on the first error; the store retries.
    async fn connect(&self) -> TransportResult<ServerInfo>;
    async fn disconnect(&self);
    /// Every event from now on. The store is the only subscriber in practice; each call gets its own receiver.
    fn subscribe(&self) -> mpsc::UnboundedReceiver<TransportEvent>;
    /// Last known address book, so names resolve before the server's own list arrives.
    fn seed_contacts(&self, contacts: &[Contact]);

    async fn list_chats(&self, options: ListChatsOptions) -> TransportResult<Page<Chat>>;
    async fn get_chat(&self, chat_guid: &str) -> TransportResult<Chat>;
    async fn load_messages(&self, chat_guid: &str, options: LoadMessagesOptions) -> TransportResult<Page<Message>>;
    /// Empty `query` with `after` lists everything created since then, oldest first; the store reconciles with it.
    async fn search_messages(&self, query: &str, filters: &SearchFilters) -> TransportResult<Vec<Message>>;
    async fn list_contacts(&self) -> TransportResult<Vec<Contact>>;

    async fn send_text(&self, chat_guid: &str, text: &str, options: SendTextOptions) -> TransportResult<Message>;
    async fn send_attachment(&self, chat_guid: &str, path: &Path, options: SendAttachmentOptions) -> TransportResult<Message>;
    /// Downloads into the attachment cache when needed and returns the shared local path.
    async fn attachment_path(&self, attachment_guid: &str, options: AttachmentPathOptions) -> TransportResult<PathBuf>;

    async fn create_chat(&self, addresses: &[String], first_message: &str, service: Option<Service>) -> TransportResult<Chat>;
    async fn mark_read(&self, chat_guid: &str) -> TransportResult<()>;
    async fn delete_chat(&self, chat_guid: &str) -> TransportResult<()>;

    /// The server holds the message and sends it once `send_at` passes; the client does no waiting.
    async fn schedule_text(&self, chat_guid: &str, text: &str, send_at: Millis) -> TransportResult<ScheduledMessage>;
    async fn list_scheduled(&self) -> TransportResult<Vec<ScheduledMessage>>;
    async fn cancel_scheduled(&self, id: &str) -> TransportResult<()>;

    // Everything below needs the private API (SIP disabled, helper connected).
    async fn react(&self, chat_guid: &str, message_guid: &str, kind: TapbackKind, options: ReactOptions) -> TransportResult<()>;
    async fn set_typing(&self, chat_guid: &str, typing: bool) -> TransportResult<()>;
    async fn mark_unread(&self, chat_guid: &str) -> TransportResult<()>;
    async fn edit_message(&self, chat_guid: &str, message_guid: &str, text: &str, options: EditOptions) -> TransportResult<Message>;
    async fn unsend_message(&self, chat_guid: &str, message_guid: &str, part_index: Option<u32>) -> TransportResult<()>;
    async fn rename_group(&self, chat_guid: &str, name: &str) -> TransportResult<()>;
    async fn add_participant(&self, chat_guid: &str, address: &str) -> TransportResult<()>;
    async fn remove_participant(&self, chat_guid: &str, address: &str) -> TransportResult<()>;
    async fn leave_group(&self, chat_guid: &str) -> TransportResult<()>;
    async fn set_group_icon(&self, chat_guid: &str, path: &Path) -> TransportResult<()>;
    /// Whether a Focus is silencing the person at `address`. Only shared with people they allow.
    async fn focus_status(&self, address: &str) -> TransportResult<FocusStatus>;
    /// Breaks a Focus for one message I sent: the "Notify Anyway" button.
    async fn notify_silenced(&self, chat_guid: &str, message_guid: &str) -> TransportResult<()>;
    /// Creates a FaceTime Link on the Mac and returns it.
    async fn create_facetime_link(&self) -> TransportResult<String>;
    /// Answers a ringing call on the Mac and returns a link that joins it from a browser.
    async fn answer_facetime(&self, call_uuid: &str) -> TransportResult<String>;
    async fn leave_facetime(&self, call_uuid: &str) -> TransportResult<()>;
}
