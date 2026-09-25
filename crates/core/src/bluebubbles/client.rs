//! Port of packages/core/src/bluebubbles/client.ts.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::model::{Chat, Contact, FocusStatus, Message, Millis, ScheduledMessage, ServerInfo, Service, TapbackKind};
use crate::transport::*;

#[derive(Clone, Debug)]
pub struct BlueBubblesOptions {
    pub url: String,
    pub password: String,
    pub attachments_dir: PathBuf,
}

/// REST timeouts: 30 s, uploads and downloads 5 min. Downloads stream to
/// `<path>.part` then rename. Group icons: at most six in flight, 3 s each, 404s
/// remembered for the session. Contact avatars: four decoded at a time.
pub struct BlueBubblesTransport {
    _private: (),
}

impl BlueBubblesTransport {
    pub fn new(options: BlueBubblesOptions, http: reqwest::Client) -> Self {
        let _ = (options, http);
        unimplemented!()
    }
}

/// One `POST /message/query` where clause.
#[derive(Clone, Debug, PartialEq)]
pub struct WhereClause {
    pub statement: String,
    pub args: Option<Value>,
}

/// Search filters to where clauses. Dates stay in the top-level `after`/`before` fields (Cocoa epoch in chat.db).
pub fn build_search_where(filters: &SearchFilters) -> Vec<WhereClause> {
    let _ = filters;
    unimplemented!()
}

#[async_trait]
#[allow(unused_variables)]
impl Transport for BlueBubblesTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::BlueBubbles
    }

    async fn connect(&self) -> TransportResult<ServerInfo> {
        unimplemented!()
    }

    async fn disconnect(&self) {
        unimplemented!()
    }

    fn subscribe(&self) -> mpsc::UnboundedReceiver<TransportEvent> {
        unimplemented!()
    }

    fn seed_contacts(&self, contacts: &[Contact]) {
        unimplemented!()
    }

    async fn list_chats(&self, options: ListChatsOptions) -> TransportResult<Page<Chat>> {
        unimplemented!()
    }

    async fn get_chat(&self, chat_guid: &str) -> TransportResult<Chat> {
        unimplemented!()
    }

    async fn load_messages(&self, chat_guid: &str, options: LoadMessagesOptions) -> TransportResult<Page<Message>> {
        unimplemented!()
    }

    async fn search_messages(&self, query: &str, filters: &SearchFilters) -> TransportResult<Vec<Message>> {
        unimplemented!()
    }

    async fn list_contacts(&self) -> TransportResult<Vec<Contact>> {
        unimplemented!()
    }

    async fn send_text(&self, chat_guid: &str, text: &str, options: SendTextOptions) -> TransportResult<Message> {
        unimplemented!()
    }

    async fn send_attachment(&self, chat_guid: &str, path: &Path, options: SendAttachmentOptions) -> TransportResult<Message> {
        unimplemented!()
    }

    async fn attachment_path(&self, attachment_guid: &str, options: AttachmentPathOptions) -> TransportResult<PathBuf> {
        unimplemented!()
    }

    async fn create_chat(&self, addresses: &[String], first_message: &str, service: Option<Service>) -> TransportResult<Chat> {
        unimplemented!()
    }

    async fn mark_read(&self, chat_guid: &str) -> TransportResult<()> {
        unimplemented!()
    }

    async fn delete_chat(&self, chat_guid: &str) -> TransportResult<()> {
        unimplemented!()
    }

    async fn schedule_text(&self, chat_guid: &str, text: &str, send_at: Millis) -> TransportResult<ScheduledMessage> {
        unimplemented!()
    }

    async fn list_scheduled(&self) -> TransportResult<Vec<ScheduledMessage>> {
        unimplemented!()
    }

    async fn cancel_scheduled(&self, id: &str) -> TransportResult<()> {
        unimplemented!()
    }

    async fn react(&self, chat_guid: &str, message_guid: &str, kind: TapbackKind, options: ReactOptions) -> TransportResult<()> {
        unimplemented!()
    }

    async fn set_typing(&self, chat_guid: &str, typing: bool) -> TransportResult<()> {
        unimplemented!()
    }

    async fn mark_unread(&self, chat_guid: &str) -> TransportResult<()> {
        unimplemented!()
    }

    async fn edit_message(&self, chat_guid: &str, message_guid: &str, text: &str, options: EditOptions) -> TransportResult<Message> {
        unimplemented!()
    }

    async fn unsend_message(&self, chat_guid: &str, message_guid: &str, part_index: Option<u32>) -> TransportResult<()> {
        unimplemented!()
    }

    async fn rename_group(&self, chat_guid: &str, name: &str) -> TransportResult<()> {
        unimplemented!()
    }

    async fn add_participant(&self, chat_guid: &str, address: &str) -> TransportResult<()> {
        unimplemented!()
    }

    async fn remove_participant(&self, chat_guid: &str, address: &str) -> TransportResult<()> {
        unimplemented!()
    }

    async fn leave_group(&self, chat_guid: &str) -> TransportResult<()> {
        unimplemented!()
    }

    async fn set_group_icon(&self, chat_guid: &str, path: &Path) -> TransportResult<()> {
        unimplemented!()
    }

    async fn focus_status(&self, address: &str) -> TransportResult<FocusStatus> {
        unimplemented!()
    }

    async fn notify_silenced(&self, chat_guid: &str, message_guid: &str) -> TransportResult<()> {
        unimplemented!()
    }

    async fn create_facetime_link(&self) -> TransportResult<String> {
        unimplemented!()
    }

    async fn answer_facetime(&self, call_uuid: &str) -> TransportResult<String> {
        unimplemented!()
    }

    async fn leave_facetime(&self, call_uuid: &str) -> TransportResult<()> {
        unimplemented!()
    }
}
