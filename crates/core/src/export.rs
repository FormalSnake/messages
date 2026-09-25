//! Port of packages/core/src/export.ts: a conversation as Markdown in ~/Downloads.

use std::path::PathBuf;
use std::sync::Arc;

use crate::model::{Chat, Handle, Message, Millis};

/// A heading with the participants, a `## <day>` whenever the date changes, one line per message.
pub fn format_conversation_markdown(chat: &Chat, handles: &[Handle], messages: &[Arc<Message>]) -> String {
    let _ = (chat, handles, messages);
    unimplemented!()
}

/// `~/Downloads/<title with /\\:*?"<>| replaced> <YYYY-MM-DD>.md`.
pub fn export_file_path(chat: &Chat, now: Millis) -> PathBuf {
    let _ = (chat, now);
    unimplemented!()
}

pub async fn write_conversation_export(chat: &Chat, handles: &[Handle], messages: &[Arc<Message>]) -> anyhow::Result<PathBuf> {
    let _ = (chat, handles, messages);
    unimplemented!()
}
