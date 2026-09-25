//! Port of packages/core/src/notify.ts. Linux: notify-send with an "open"
//! action (`--app-name=Messages --category=im.received --action=open=Open`).
//! macOS: osascript. Windows: a PowerShell toast. Failures are logged, never returned.

use std::path::PathBuf;

use crate::model::{Chat, Message};

#[derive(Clone, Debug, Default)]
pub struct NotifyOptions {
    /// The message a tapback landed on, when it is loaded.
    pub target: Option<Message>,
    /// Icon for the Linux notification daemon.
    pub icon: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyAction {
    Open,
}

/// The notification text, or None for a tapback removal. Pure, for tests.
pub fn notification_body(chat: &Chat, message: &Message, target: Option<&Message>) -> Option<String> {
    let _ = (chat, message, target);
    unimplemented!()
}

/// Resolves to Some(Open) when the person clicked the action.
pub async fn notify_incoming(chat: &Chat, message: &Message, options: NotifyOptions) -> Option<NotifyAction> {
    let _ = (chat, message, options);
    unimplemented!()
}
