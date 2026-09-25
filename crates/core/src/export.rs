//! Port of packages/core/src/export.ts: a conversation as Markdown in ~/Downloads.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Local};

use crate::format::hhmm;
use crate::model::{chat_title, handle_name, tapback_glyph, Chat, GroupEvent, Handle, Message, Millis};

fn day_heading(ms: Millis) -> String {
    DateTime::from_timestamp_millis(ms).unwrap_or_default().with_timezone(&Local).format("%A, %B %-d, %Y").to_string()
}

fn group_event_text(event: &GroupEvent) -> String {
    match event {
        GroupEvent::Rename { title } => format!("named the conversation \"{title}\""),
        GroupEvent::Join { who } => format!("added {}", who.as_ref().map_or("someone".to_owned(), |h| handle_name(h).to_owned())),
        GroupEvent::Leave { who } => match who {
            Some(who) => format!("removed {}", handle_name(who)),
            None => "left the conversation".to_owned(),
        },
        GroupEvent::Photo => "changed the group photo".to_owned(),
    }
}

fn attachments_text(message: &Message) -> String {
    message.attachments.iter().filter(|item| !item.hidden).map(|item| format!("[{}]", item.name)).collect::<Vec<_>>().join(" ")
}

fn tapback_text(tapbacks: &[crate::model::Tapback]) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let mut order: Vec<&str> = Vec::new();
    for tapback in tapbacks {
        let glyph = tapback_glyph(tapback.kind, tapback.emoji.as_deref());
        if !counts.contains_key(glyph) {
            order.push(glyph);
        }
        *counts.entry(glyph).or_insert(0) += 1;
    }
    if counts.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = order.iter().map(|glyph| format!("{glyph} {}", counts[glyph])).collect();
    format!("({})", parts.join(", "))
}

fn message_line(message: &Message) -> String {
    let who = if message.from_me {
        "You".to_owned()
    } else if let Some(sender) = &message.sender {
        handle_name(sender).to_owned()
    } else {
        "Unknown".to_owned()
    };
    let body = if message.date_retracted.is_some() {
        "unsent a message".to_owned()
    } else if let Some(event) = &message.group_event {
        group_event_text(event)
    } else {
        message.text.clone()
    };
    let parts: Vec<String> = [body, attachments_text(message), tapback_text(&message.tapbacks)].into_iter().filter(|p| !p.is_empty()).collect();
    format!("**{who}** {}  {}", hhmm(message.date), parts.join(" ")).trim_end().to_owned()
}

/// A heading with the participants, a `## <day>` whenever the date changes, one line per message.
pub fn format_conversation_markdown(chat: &Chat, handles: &[Handle], messages: &[Arc<Message>]) -> String {
    let participants = if chat.is_group {
        handles.iter().map(|h| handle_name(h)).collect::<Vec<_>>().join(", ")
    } else {
        handles.first().map(|h| handle_name(h).to_owned()).unwrap_or_else(|| chat.identifier.clone())
    };
    let mut lines: Vec<String> = vec![format!("# {participants}"), String::new()];
    let mut last_day = String::new();
    for message in messages {
        let day = day_heading(message.date);
        if day != last_day {
            lines.push(format!("## {day}"));
            lines.push(String::new());
            last_day = day;
        }
        lines.push(message_line(message));
    }
    format!("{}\n", lines.join("\n"))
}

fn sanitize_filename(name: &str) -> String {
    name.chars().map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c }).collect::<String>().trim().to_owned()
}

/// `~/Downloads/<title with /\\:*?"<>| replaced> <YYYY-MM-DD>.md`.
pub fn export_file_path(chat: &Chat, now: Millis) -> PathBuf {
    let date = DateTime::from_timestamp_millis(now).unwrap_or_default().with_timezone(&Local).format("%Y-%m-%d").to_string();
    let home = dirs::home_dir().unwrap_or_default();
    home.join("Downloads").join(format!("{} {date}.md", sanitize_filename(&chat_title(chat))))
}

pub async fn write_conversation_export(chat: &Chat, handles: &[Handle], messages: &[Arc<Message>]) -> anyhow::Result<PathBuf> {
    let now = chrono::Utc::now().timestamp_millis();
    let file_path = export_file_path(chat, now);
    if let Some(parent) = file_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let markdown = format_conversation_markdown(chat, handles, messages);
    tokio::fs::write(&file_path, markdown).await?;
    Ok(file_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Attachment, Service, Tapback, TapbackKind};

    fn alex() -> Handle {
        Handle { address: "+14155550134".to_owned(), service: Service::IMessage, name: Some("Alex Rivera".to_owned()), avatar: None }
    }

    fn ben() -> Handle {
        Handle { address: "+14155550170".to_owned(), service: Service::IMessage, name: Some("Ben Okafor".to_owned()), avatar: None }
    }

    fn chat(is_group: bool, display_name: Option<&str>) -> Chat {
        Chat {
            guid: "c1".to_owned(),
            identifier: "+14155550134".to_owned(),
            group_id: None,
            service: Service::IMessage,
            is_group,
            display_name: display_name.map(str::to_owned),
            icon: None,
            participants: vec![alex()],
            pinned: false,
            muted: false,
            read_receipts: None,
            archived: false,
            unread: false,
            last_message: None,
            last_activity: 0,
        }
    }

    fn day1() -> Millis {
        Local.with_ymd_and_hms(2026, 1, 5, 9, 15, 0).single().unwrap().timestamp_millis()
    }

    fn day2() -> Millis {
        Local.with_ymd_and_hms(2026, 1, 6, 18, 5, 0).single().unwrap().timestamp_millis()
    }

    fn message(text: &str) -> Message {
        Message {
            guid: "m1".to_owned(),
            chat_guid: "c1".to_owned(),
            text: text.to_owned(),
            from_me: false,
            sender: Some(alex()),
            date: day1(),
            service: Service::IMessage,
            ..Default::default()
        }
    }

    use chrono::TimeZone;

    #[test]
    fn heads_the_file_with_the_participants() {
        let markdown = format_conversation_markdown(&chat(false, None), &[alex()], &[]);
        assert!(markdown.starts_with("# Alex Rivera\n"));
    }

    #[test]
    fn lists_every_participant_for_a_group() {
        let markdown = format_conversation_markdown(&chat(true, None), &[alex(), ben()], &[]);
        assert!(markdown.starts_with("# Alex Rivera, Ben Okafor\n"));
    }

    #[test]
    fn formats_a_line_as_sender_hhmm_text() {
        let markdown = format_conversation_markdown(&chat(false, None), &[alex()], &[Arc::new(message("coffee at 4?"))]);
        assert!(markdown.contains("**Alex Rivera** 09:15  coffee at 4?"));
    }

    #[test]
    fn labels_the_users_own_messages_you() {
        let mut m = message("sure");
        m.from_me = true;
        m.sender = None;
        let markdown = format_conversation_markdown(&chat(false, None), &[alex()], &[Arc::new(m)]);
        assert!(markdown.contains("**You** 09:15  sure"));
    }

    #[test]
    fn inserts_a_day_separator_when_the_date_changes_once_per_day() {
        let mut m1 = message("a");
        m1.date = day1();
        let mut m2 = message("b");
        m2.guid = "m2".to_owned();
        m2.date = day1() + 60_000;
        let mut m3 = message("c");
        m3.guid = "m3".to_owned();
        m3.date = day2();
        let markdown = format_conversation_markdown(&chat(false, None), &[alex()], &[Arc::new(m1), Arc::new(m2), Arc::new(m3)]);
        assert_eq!(markdown.lines().filter(|l| l.starts_with("## ")).count(), 2);
    }

    #[test]
    fn renders_attachments_as_a_trailing_name() {
        let mut m = message("");
        m.attachments = vec![Attachment {
            guid: "a1".to_owned(),
            name: "IMG_1.jpg".to_owned(),
            mime: "image/jpeg".to_owned(),
            bytes: 100,
            width: None,
            height: None,
            measured: false,
            is_sticker: false,
            local_path: None,
            hidden: false,
            duration_ms: None,
        }];
        let markdown = format_conversation_markdown(&chat(false, None), &[alex()], &[Arc::new(m)]);
        assert!(markdown.contains("**Alex Rivera** 09:15  [IMG_1.jpg]"));
    }

    #[test]
    fn drops_hidden_attachments() {
        let mut m = message("a link");
        m.attachments = vec![Attachment {
            guid: "a1".to_owned(),
            name: "preview.jpg".to_owned(),
            mime: "image/jpeg".to_owned(),
            bytes: 100,
            width: None,
            height: None,
            measured: false,
            is_sticker: false,
            local_path: None,
            hidden: true,
            duration_ms: None,
        }];
        let markdown = format_conversation_markdown(&chat(false, None), &[alex()], &[Arc::new(m)]);
        assert!(!markdown.contains("preview.jpg"));
    }

    #[test]
    fn renders_tapbacks_as_a_trailing_count_list() {
        let mut m = message("shipped");
        m.tapbacks = vec![
            Tapback { guid: "t1".to_owned(), kind: TapbackKind::Love, emoji: None, from_me: true, sender: None },
            Tapback { guid: "t2".to_owned(), kind: TapbackKind::Love, emoji: None, from_me: false, sender: Some(ben()) },
            Tapback { guid: "t3".to_owned(), kind: TapbackKind::Like, emoji: None, from_me: false, sender: Some(ben()) },
        ];
        let markdown = format_conversation_markdown(&chat(false, None), &[alex()], &[Arc::new(m)]);
        assert!(markdown.contains("**Alex Rivera** 09:15  shipped (\u{2764}\u{FE0F} 2, \u{1F44D} 1)"));
    }

    #[test]
    fn describes_a_retracted_message_instead_of_showing_its_text() {
        let mut m = message("oops");
        m.date_retracted = Some(day1());
        let markdown = format_conversation_markdown(&chat(false, None), &[alex()], &[Arc::new(m)]);
        assert!(markdown.contains("**Alex Rivera** 09:15  unsent a message"));
    }

    #[test]
    fn describes_a_group_event_instead_of_its_empty_text() {
        let mut m = message("");
        m.group_event = Some(GroupEvent::Rename { title: "Family".to_owned() });
        let markdown = format_conversation_markdown(&chat(true, None), &[alex(), ben()], &[Arc::new(m)]);
        assert!(markdown.contains("**Alex Rivera** 09:15  named the conversation \"Family\""));
    }

    #[test]
    fn lands_in_downloads_named_after_the_title_and_the_export_date() {
        let now = Local.with_ymd_and_hms(2026, 3, 4, 12, 0, 0).single().unwrap().timestamp_millis();
        let path = export_file_path(&chat(false, Some("Family Trip")), now);
        assert!(path.to_string_lossy().ends_with("/Downloads/Family Trip 2026-03-04.md"));
    }

    #[test]
    fn replaces_characters_a_filesystem_would_reject() {
        let now = Local.with_ymd_and_hms(2026, 3, 4, 12, 0, 0).single().unwrap().timestamp_millis();
        let path = export_file_path(&chat(false, Some("Q1/Q2 planning")), now);
        assert!(path.to_string_lossy().ends_with("/Downloads/Q1-Q2 planning 2026-03-04.md"));
    }
}
