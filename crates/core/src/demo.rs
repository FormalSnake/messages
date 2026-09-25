//! Port of packages/core/src/demo.ts: fixtures and a transport that answers
//! from them, for `MESSAGES_DEMO=1` and the UI tests. Sends get a canned reply.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::model::{Chat, Contact, FocusStatus, Message, Millis, ScheduledMessage, ServerInfo, Service, TapbackKind};
use crate::transport::*;

pub struct DemoTransport {
    _private: (),
}

impl DemoTransport {
    pub fn new() -> Self {
        unimplemented!()
    }
}

#[async_trait]
#[allow(unused_variables)]
impl Transport for DemoTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Demo
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
