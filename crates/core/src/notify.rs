//! Linux: notify-send with an "open" action
//! (`--app-name=Messages --category=im.received --action=open=Open`).
//! macOS: osascript. Windows: a PowerShell toast. Failures are logged, never returned.

use std::path::PathBuf;
use std::process::Stdio;

use tokio::process::Command;

use crate::model::{chat_title, handle_name, tapback_glyph, Chat, Message};

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

fn summary(message: &Message) -> String {
    if message.date_retracted.is_some() {
        return "Unsent a message".to_owned();
    }
    if !message.attachments.is_empty() {
        let first = message.attachments.iter().find(|item| !item.hidden).unwrap_or(&message.attachments[0]);
        if first.mime.starts_with("image/") {
            return if !message.text.is_empty() {
                message.text.clone()
            } else if first.is_sticker {
                "Sent a sticker".to_owned()
            } else {
                "Sent a photo".to_owned()
            };
        }
        if first.mime.starts_with("video/") {
            return if !message.text.is_empty() { message.text.clone() } else { "Sent a video".to_owned() };
        }
        if message.is_audio {
            return "Sent an audio message".to_owned();
        }
        return if !message.text.is_empty() { message.text.clone() } else { format!("Sent {}", first.name) };
    }
    message.text.clone()
}

/// The notification text, or None for a tapback removal. Pure, for tests.
pub fn notification_body(chat: &Chat, message: &Message, target: Option<&Message>) -> Option<String> {
    let sender = message.sender.as_ref().map(|s| handle_name(s).to_owned()).unwrap_or_else(|| chat_title(chat));
    if let Some(reaction) = &message.reaction {
        if reaction.removed {
            return None;
        }
        let quoted = target.map_or_else(
            || "a message".to_owned(),
            |target| {
                let trimmed = target.text.trim();
                if !trimmed.is_empty() {
                    let clipped: String = trimmed.chars().take(80).collect();
                    format!("\u{201C}{clipped}\u{201D}")
                } else if !target.attachments.is_empty() {
                    "an attachment".to_owned()
                } else {
                    "a message".to_owned()
                }
            },
        );
        return Some(format!("{sender} reacted {} to {quoted}", tapback_glyph(reaction.kind, reaction.emoji.as_deref())));
    }
    let text = summary(message);
    Some(if chat.is_group && message.sender.is_some() { format!("{sender}: {text}") } else { text })
}

/// A toast needs an AppUserModelID Windows knows about, or it is dropped
/// without an error. The registry key is enough (no Start Menu shortcut
/// needed), so the script writes it before every toast.
#[cfg(windows)]
const WINDOWS_TOAST: &str = r#"$key = 'HKCU:\Software\Classes\AppUserModelId\Messages'
if (-not (Test-Path $key)) { New-Item $key -Force | Out-Null; Set-ItemProperty $key DisplayName 'Messages' }
[void][Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime]
[void][Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime]
$xml = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02)
$lines = $xml.GetElementsByTagName('text')
[void]$lines.Item(0).AppendChild($xml.CreateTextNode($env:MESSAGES_TITLE))
[void]$lines.Item(1).AppendChild($xml.CreateTextNode($env:MESSAGES_TEXT))
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Messages').Show([Windows.UI.Notifications.ToastNotification]::new($xml))"#;

/// Desktop notification: notify-send on Linux (with an Open action when the
/// daemon supports it), osascript on macOS, a toast on Windows. Resolves to
/// Some(Open) when the person clicked the action.
pub async fn notify_incoming(chat: &Chat, message: &Message, options: NotifyOptions) -> Option<NotifyAction> {
    let title = chat_title(chat);
    let text = notification_body(chat, message, options.target.as_ref())?;

    #[cfg(target_os = "macos")]
    {
        let script = format!("display notification {} with title {}", applescript_json(&text), applescript_json(&title));
        if let Err(error) = Command::new("osascript").arg("-e").arg(&script).stdout(Stdio::null()).stderr(Stdio::null()).status().await {
            tracing::error!("notify: {error}");
        }
        None
    }

    #[cfg(windows)]
    {
        let env: std::collections::HashMap<&str, String> = [("MESSAGES_TITLE", title), ("MESSAGES_TEXT", text)].into_iter().collect();
        if crate::windows::powershell(WINDOWS_TOAST, &env).await.is_none() {
            tracing::error!("notify: toast failed");
        }
        None
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut cmd = Command::new("notify-send");
        cmd.arg("--app-name=Messages").arg("--category=im.received").arg("--action=open=Open");
        if let Some(icon) = &options.icon {
            cmd.arg(format!("--icon={}", icon.display()));
        }
        cmd.arg(&title).arg(&text).stdout(Stdio::piped()).stderr(Stdio::null());
        match cmd.output().await {
            Ok(output) => {
                let clicked = String::from_utf8_lossy(&output.stdout).trim() == "open";
                if clicked { Some(NotifyAction::Open) } else { None }
            }
            Err(error) => {
                tracing::error!("notify: {error}");
                None
            }
        }
    }
}

/// A JSON string literal doubles as a valid AppleScript string literal for anything this app sends it.
#[cfg(target_os = "macos")]
fn applescript_json(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Handle, Reaction, Service, TapbackKind};

    fn chat(is_group: bool) -> Chat {
        Chat {
            guid: "c1".to_owned(),
            identifier: "c1".to_owned(),
            group_id: None,
            service: Service::IMessage,
            is_group,
            display_name: None,
            icon: None,
            participants: vec![],
            pinned: false,
            muted: false,
            read_receipts: None,
            archived: false,
            unread: false,
            last_message: None,
            last_activity: 0,
        }
    }

    fn sender() -> Handle {
        Handle { address: "+15551234567".to_owned(), service: Service::IMessage, name: Some("Alex".to_owned()), avatar: None }
    }

    fn message(text: &str) -> Message {
        Message { guid: "m1".to_owned(), chat_guid: "c1".to_owned(), text: text.to_owned(), sender: Some(sender()), service: Service::IMessage, ..Default::default() }
    }

    #[test]
    fn plain_text_in_a_one_to_one_chat() {
        assert_eq!(notification_body(&chat(false), &message("hey"), None), Some("hey".to_owned()));
    }

    #[test]
    fn prefixes_the_sender_in_a_group() {
        assert_eq!(notification_body(&chat(true), &message("hey"), None), Some("Alex: hey".to_owned()));
    }

    #[test]
    fn a_removed_tapback_produces_no_notification() {
        let mut m = message("");
        m.reaction = Some(Reaction { target_guid: "t1".to_owned(), kind: TapbackKind::Love, emoji: None, removed: true });
        assert_eq!(notification_body(&chat(false), &m, None), None);
    }

    #[test]
    fn a_tapback_quotes_the_target_message() {
        let mut m = message("");
        m.reaction = Some(Reaction { target_guid: "t1".to_owned(), kind: TapbackKind::Love, emoji: None, removed: false });
        let target = message("Still on for Friday?");
        let body = notification_body(&chat(false), &m, Some(&target)).unwrap();
        assert!(body.contains("Alex reacted"));
        assert!(body.contains("Still on for Friday?"));
    }

    #[test]
    fn an_unsent_message_is_summarized() {
        let mut m = message("oops");
        m.date_retracted = Some(1);
        assert_eq!(notification_body(&chat(false), &m, None), Some("Unsent a message".to_owned()));
    }
}
