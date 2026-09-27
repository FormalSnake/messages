//! Turns a conversation's messages into list rows: date separators, runs,
//! quotes, receipts, event captions. Pure, so every string a row shows is
//! formatted here once per change rather than in render.

use std::sync::Arc;

use gpui_kit::{Corners, Pixels};
use messages_core::format::{format_separator, format_time, needs_separator};
use messages_core::{DeliveryState, GroupEvent, Message, Service, Tapback, delivery_state, handle_name, tapback_glyph};

use crate::theme::radius;

/// Same author within this long joins the previous bubble's run.
pub const RUN_GAP_MS: i64 = 60_000;
pub const EDIT_WINDOW_MS: i64 = 15 * 60_000;
pub const UNSEND_WINDOW_MS: i64 = 2 * 60_000;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    Single,
    First,
    Middle,
    Last,
}

impl Position {
    pub fn starts_run(self) -> bool {
        matches!(self, Position::First | Position::Single)
    }

    pub fn ends_run(self) -> bool {
        matches!(self, Position::Last | Position::Single)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub text: String,
    pub failed: bool,
}

#[derive(Clone, Debug)]
pub struct MessageRowData {
    pub key: String,
    pub message: Arc<Message>,
    pub position: Position,
    pub show_sender: bool,
    pub receipt: Option<Receipt>,
    /// Offer "Notify Anyway" under this row.
    pub notify: bool,
    pub show_quote: bool,
}

impl PartialEq for MessageRowData {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && Arc::ptr_eq(&self.message, &other.message)
            && self.position == other.position
            && self.show_sender == other.show_sender
            && self.receipt == other.receipt
            && self.notify == other.notify
            && self.show_quote == other.show_quote
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Separator { key: String, label: String },
    Event { key: String, text: String },
    Message(MessageRowData),
    Typing,
    Loading,
}

fn same_author(a: &Message, b: &Message) -> bool {
    if a.from_me != b.from_me {
        return false;
    }
    a.from_me || a.sender.as_ref().map(|s| &s.address) == b.sender.as_ref().map(|s| &s.address)
}

/// The caption a group event or an unsent message shows instead of a bubble.
pub fn event_text(message: &Message) -> Option<String> {
    let who = if message.from_me { "You" } else { message.sender.as_ref().map(handle_name).unwrap_or("Someone") };
    if message.date_retracted.is_some() {
        return Some(format!("{who} unsent a message."));
    }
    Some(match message.group_event.as_ref()? {
        GroupEvent::Rename { title } => format!("{who} named the conversation \u{201C}{title}\u{201D}."),
        GroupEvent::Join { who: added } => format!("{who} added {}.", added.as_ref().map(handle_name).unwrap_or("someone")),
        GroupEvent::Leave { who: left } => format!("{} left the conversation.", left.as_ref().map(handle_name).unwrap_or(who)),
        GroupEvent::Photo => format!("{who} changed the group photo."),
    })
}

fn is_event(message: &Message) -> bool {
    message.date_retracted.is_some() || message.group_event.is_some()
}

pub struct BuildOptions<'a> {
    pub is_group: bool,
    pub typing: bool,
    pub loading: bool,
    pub online: bool,
    /// Name of the person whose Focus is on, when the private API says one is.
    pub silenced_by: Option<&'a str>,
    pub now: i64,
}

pub fn build_rows(messages: &[Arc<Message>], options: &BuildOptions) -> Vec<Row> {
    let mut rows = Vec::with_capacity(messages.len() + 8);
    if options.loading {
        rows.push(Row::Loading);
    }
    let mut last_mine: Option<usize> = None;
    let mut last_read: Option<usize> = None;
    for (index, message) in messages.iter().enumerate() {
        if message.from_me && message.error.is_none() && !is_event(message) {
            last_mine = Some(index);
            if message.date_read.is_some() {
                last_read = Some(index);
            }
        }
    }
    // A run of replies to the same message shares one quote, on its first row.
    let mut quoted = Vec::with_capacity(messages.len());
    let mut last_shown: Option<&Message> = None;
    for message in messages {
        if is_event(message) {
            quoted.push(false);
            continue;
        }
        quoted.push(message.reply_to.is_some() && last_shown.map(|m| &m.reply_to) != Some(&message.reply_to));
        last_shown = Some(message);
    }

    let mut previous_date: Option<i64> = None;
    for (index, message) in messages.iter().enumerate() {
        let key = message.temp_guid.clone().unwrap_or_else(|| message.guid.clone());
        if needs_separator(previous_date, message.date) {
            rows.push(Row::Separator { key: format!("sep-{key}"), label: format_separator(message.date, options.now) });
        }
        previous_date = Some(message.date);
        if let Some(text) = event_text(message) {
            rows.push(Row::Event { key, text });
            continue;
        }
        let show_quote = quoted[index];
        let joins_previous = index > 0 && {
            let previous = &messages[index - 1];
            same_author(previous, message) && !is_event(previous) && message.date - previous.date < RUN_GAP_MS && !needs_separator(Some(previous.date), message.date) && !show_quote
        };
        let joins_next = messages.get(index + 1).is_some_and(|next| {
            same_author(message, next) && !is_event(next) && next.date - message.date < RUN_GAP_MS && !needs_separator(Some(message.date), next.date) && !quoted[index + 1]
        });
        let position = match (joins_previous, joins_next) {
            (true, true) => Position::Middle,
            (true, false) => Position::Last,
            (false, true) => Position::First,
            (false, false) => Position::Single,
        };
        let mut receipt = None;
        let mut notify = false;
        if message.from_me {
            let state = delivery_state(message);
            let text = if state == DeliveryState::Failed {
                Some("Not delivered".to_owned())
            } else if state == DeliveryState::Sending {
                Some(if options.online { "Sending\u{2026}" } else { "Waiting for connection\u{2026}" }.to_owned())
            } else if Some(index) == last_read && message.date_read.is_some() {
                Some(format!("Read {}", format_time(message.date_read.unwrap_or_default())))
            } else if Some(index) == last_mine && last_read.is_none_or(|read| read < index) {
                // A Focus shows up two ways: on the message once it landed without
                // a sound, and on the person while theirs is on. Either way the last
                // thing I sent is the one that can break through.
                if message.notified == Some(true) {
                    Some("Notified".to_owned())
                } else if message.delivered_quietly == Some(true) {
                    notify = true;
                    Some("Delivered Quietly".to_owned())
                } else if let (Some(name), DeliveryState::Delivered) = (options.silenced_by, state) {
                    notify = true;
                    Some(format!("{name} has notifications silenced"))
                } else if state == DeliveryState::Delivered {
                    Some("Delivered".to_owned())
                } else if message.service == Service::IMessage {
                    Some("Sent".to_owned())
                } else {
                    Some("Sent as text message".to_owned())
                }
            } else {
                None
            };
            receipt = text.map(|text| Receipt { text, failed: state == DeliveryState::Failed });
        }
        rows.push(Row::Message(MessageRowData {
            key,
            message: message.clone(),
            position,
            show_sender: options.is_group && !message.from_me && position.starts_run(),
            receipt,
            notify,
            show_quote,
        }));
    }
    if options.typing {
        rows.push(Row::Typing);
    }
    rows
}

/// Run-aware corners: tight where the bubble joins its neighbour, full at the run's ends, mirrored for mine.
pub fn bubble_radius(from_me: bool, position: Position) -> Corners<Pixels> {
    let big = radius::BUBBLE;
    let small = radius::BUBBLE_TIGHT;
    let top = if matches!(position, Position::Middle | Position::Last) { small } else { big };
    let bottom = if matches!(position, Position::Middle | Position::First) { small } else { big };
    if from_me {
        Corners { top_left: big, bottom_left: big, top_right: top, bottom_right: bottom }
    } else {
        Corners { top_right: big, bottom_right: big, top_left: top, bottom_left: bottom }
    }
}

pub fn effect_name(id: &str) -> String {
    let known = match id {
        "com.apple.MobileSMS.expressivesend.impact" => "Slam",
        "com.apple.MobileSMS.expressivesend.loud" => "Loud",
        "com.apple.MobileSMS.expressivesend.gentle" => "Gentle",
        "com.apple.MobileSMS.expressivesend.invisibleink" => "Invisible Ink",
        "com.apple.messages.effect.CKEchoEffect" => "Echo",
        "com.apple.messages.effect.CKSpotlightEffect" => "Spotlight",
        "com.apple.messages.effect.CKHappyBirthdayEffect" => "Balloons",
        "com.apple.messages.effect.CKConfettiEffect" => "Confetti",
        "com.apple.messages.effect.CKHeartEffect" => "Love",
        "com.apple.messages.effect.CKLasersEffect" => "Lasers",
        "com.apple.messages.effect.CKFireworksEffect" => "Fireworks",
        "com.apple.messages.effect.CKShootingStarEffect" => "Shooting Star",
        "com.apple.messages.effect.CKSparklesEffect" => "Celebration",
        _ => return id.rsplit('.').next().unwrap_or(id).to_owned(),
    };
    known.to_owned()
}

#[derive(Clone, Debug, PartialEq)]
pub struct TapbackGroup {
    pub glyph: String,
    pub count: usize,
    pub mine: bool,
    /// Who sent it, in arrival order.
    pub who: Vec<String>,
}

/// At most three pills, one per glyph, in first-seen order.
pub fn tapback_groups(tapbacks: &[Tapback]) -> Vec<TapbackGroup> {
    let mut groups: Vec<TapbackGroup> = Vec::new();
    for tapback in tapbacks {
        let glyph = tapback_glyph(tapback.kind, tapback.emoji.as_deref()).to_owned();
        let who = if tapback.from_me { "You".to_owned() } else { tapback.sender.as_ref().map(|s| handle_name(s).to_owned()).unwrap_or_else(|| "Unknown sender".to_owned()) };
        match groups.iter_mut().find(|group| group.glyph == glyph) {
            Some(group) => {
                group.count += 1;
                group.mine |= tapback.from_me;
                group.who.push(who);
            }
            None => groups.push(TapbackGroup { glyph, count: 1, mine: tapback.from_me, who: vec![who] }),
        }
    }
    groups.truncate(3);
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    fn message(text: &str, date: i64, edit: impl FnOnce(&mut Message)) -> Arc<Message> {
        let seq = SEQ.fetch_add(1, Ordering::SeqCst);
        let mut message = Message { guid: format!("m{seq}"), chat_guid: "c".into(), text: text.into(), date, service: Service::IMessage, ..Message::default() };
        edit(&mut message);
        Arc::new(message)
    }

    fn options(silenced_by: Option<&str>) -> BuildOptions<'_> {
        BuildOptions { is_group: false, typing: false, loading: false, online: true, silenced_by, now: 0 }
    }

    fn message_rows(messages: &[Arc<Message>], silenced_by: Option<&str>) -> Vec<MessageRowData> {
        build_rows(messages, &options(silenced_by))
            .into_iter()
            .filter_map(|row| match row {
                Row::Message(data) => Some(data),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn heads_a_run_of_replies_to_the_same_message_with_one_quote_and_groups_the_bubbles() {
        let original = message("wat zie je in hem", 0, |m| m.from_me = true);
        let later = message("en?", 500, |m| m.from_me = true);
        let mut all = vec![original.clone(), later];
        for date in [1000, 2000, 3000] {
            all.push(message("reply", date, |m| m.reply_to = Some(original.guid.clone())));
        }
        let rows = message_rows(&all, None);
        assert_eq!(rows.iter().map(|r| r.show_quote).collect::<Vec<_>>(), vec![false, false, true, false, false]);
        assert_eq!(rows[2..].iter().map(|r| r.position).collect::<Vec<_>>(), vec![Position::First, Position::Middle, Position::Last]);
    }

    #[test]
    fn starts_a_new_run_where_a_quote_appears() {
        let old = message("old", 0, |_| {});
        let filler = message("filler", 100_000, |_| {});
        let plain = message("plain", 200_000, |_| {});
        let reply = message("about the old one", 201_000, |m| m.reply_to = Some(old.guid.clone()));
        let rows = message_rows(&[old, filler, plain, reply], None);
        assert_eq!(rows.iter().map(|r| r.show_quote).collect::<Vec<_>>(), vec![false, false, false, true]);
        assert_eq!(rows[2..].iter().map(|r| r.position).collect::<Vec<_>>(), vec![Position::Single, Position::Single]);
    }

    #[test]
    fn runs_break_after_a_minute_and_on_a_new_author() {
        let a = message("a", 0, |_| {});
        let b = message("b", 30_000, |_| {});
        let c = message("c", 95_000, |_| {});
        let d = message("d", 96_000, |m| m.from_me = true);
        let rows = message_rows(&[a, b, c, d], None);
        assert_eq!(rows.iter().map(|r| r.position).collect::<Vec<_>>(), vec![Position::First, Position::Last, Position::Single, Position::Single]);
    }

    #[test]
    fn separators_follow_an_hour_gap_and_events_become_captions() {
        let a = message("a", 0, |_| {});
        let b = message("b", 2 * 60 * 60 * 1000, |_| {});
        let c = message("", 2 * 60 * 60 * 1000 + 1, |m| m.date_retracted = Some(1));
        let rows = build_rows(&[a, b, c], &options(None));
        let kinds: Vec<&str> = rows
            .iter()
            .map(|row| match row {
                Row::Separator { .. } => "sep",
                Row::Event { .. } => "event",
                Row::Message(_) => "msg",
                Row::Typing => "typing",
                Row::Loading => "loading",
            })
            .collect();
        assert_eq!(kinds, vec!["sep", "msg", "sep", "msg", "event"]);
        match &rows[4] {
            Row::Event { text, .. } => assert_eq!(text, "Someone unsent a message."),
            _ => unreachable!(),
        }
    }

    #[test]
    fn a_failed_send_reads_not_delivered_and_a_pending_one_waits_offline() {
        let failed = message("x", 0, |m| {
            m.from_me = true;
            m.error = Some("Not delivered (error 22)".into());
        });
        let pending = message("y", 10, |m| {
            m.from_me = true;
            m.temp_guid = Some(m.guid.clone());
        });
        let rows = build_rows(&[failed, pending], &BuildOptions { online: false, ..options(None) });
        let receipts: Vec<Option<Receipt>> = rows
            .into_iter()
            .filter_map(|row| match row {
                Row::Message(data) => Some(data.receipt),
                _ => None,
            })
            .collect();
        assert_eq!(receipts[0], Some(Receipt { text: "Not delivered".into(), failed: true }));
        assert_eq!(receipts[1], Some(Receipt { text: "Waiting for connection\u{2026}".into(), failed: false }));
    }

    #[test]
    fn only_my_last_message_carries_sent_or_delivered() {
        let first = message("a", 0, |m| {
            m.from_me = true;
            m.date_delivered = Some(1);
        });
        let second = message("b", 10, |m| m.from_me = true);
        let sms = message("c", 20, |m| {
            m.from_me = true;
            m.service = Service::Sms;
        });
        let rows = message_rows(&[first, second.clone()], None);
        assert_eq!(rows[0].receipt, None);
        assert_eq!(rows[1].receipt.as_ref().map(|r| r.text.as_str()), Some("Sent"));
        let rows = message_rows(&[second, sms], None);
        assert_eq!(rows[1].receipt.as_ref().map(|r| r.text.as_str()), Some("Sent as text message"));
    }

    #[test]
    fn names_a_focus_under_the_last_message_i_sent_and_offers_to_break_through() {
        let sent = message("you up", 0, |m| {
            m.from_me = true;
            m.date_delivered = Some(1);
        });
        let rows = message_rows(&[sent], Some("Ben Okafor"));
        assert_eq!(rows[0].receipt.as_ref().map(|r| r.text.as_str()), Some("Ben Okafor has notifications silenced"));
        assert!(rows[0].notify);
    }

    #[test]
    fn prefers_what_the_message_itself_came_back_with() {
        let sent = message("you up", 0, |m| {
            m.from_me = true;
            m.date_delivered = Some(1);
            m.delivered_quietly = Some(true);
        });
        let rows = message_rows(&[sent], Some("Ben Okafor"));
        assert_eq!(rows[0].receipt.as_ref().map(|r| r.text.as_str()), Some("Delivered Quietly"));
        assert!(rows[0].notify);
    }

    #[test]
    fn stops_offering_once_broken_through_or_read() {
        let notified = message("you up", 0, |m| {
            m.from_me = true;
            m.date_delivered = Some(1);
            m.delivered_quietly = Some(true);
            m.notified = Some(true);
        });
        let rows = message_rows(&[notified], Some("Ben Okafor"));
        assert_eq!(rows[0].receipt.as_ref().map(|r| r.text.as_str()), Some("Notified"));
        assert!(!rows[0].notify);
        let read = message("you up", 0, |m| {
            m.from_me = true;
            m.date_delivered = Some(1);
            m.date_read = Some(2);
        });
        let rows = message_rows(&[read], Some("Ben Okafor"));
        assert!(rows[0].receipt.as_ref().is_some_and(|r| r.text.starts_with("Read ")));
        assert!(!rows[0].notify);
    }

    #[test]
    fn groups_show_the_sender_on_a_runs_first_bubble_only() {
        let alex = messages_core::Handle { address: "+1".into(), service: Service::IMessage, name: Some("Alex".into()), avatar: None };
        let a = message("a", 0, |m| m.sender = Some(alex.clone()));
        let b = message("b", 1000, |m| m.sender = Some(alex.clone()));
        let rows: Vec<bool> = build_rows(&[a, b], &BuildOptions { is_group: true, ..options(None) })
            .into_iter()
            .filter_map(|row| match row {
                Row::Message(data) => Some(data.show_sender),
                _ => None,
            })
            .collect();
        assert_eq!(rows, vec![true, false]);
    }

    #[test]
    fn tapbacks_group_by_glyph_and_cap_at_three() {
        use messages_core::TapbackKind;
        let tapback = |kind, from_me| Tapback { guid: String::new(), kind, emoji: None, from_me, sender: None };
        let groups = tapback_groups(&[
            tapback(TapbackKind::Love, false),
            tapback(TapbackKind::Love, true),
            tapback(TapbackKind::Like, false),
            tapback(TapbackKind::Laugh, false),
            tapback(TapbackKind::Question, false),
        ]);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].count, 2);
        assert!(groups[0].mine);
        assert_eq!(groups[0].who, vec!["Unknown sender".to_owned(), "You".to_owned()]);
    }

    #[test]
    fn bubble_corners_tighten_where_a_run_joins_and_mirror_for_mine() {
        let (big, small) = (radius::BUBBLE, radius::BUBBLE_TIGHT);
        let received = |position| {
            let c = bubble_radius(false, position);
            (c.top_left, c.bottom_left, c.top_right, c.bottom_right)
        };
        assert_eq!(received(Position::Single), (big, big, big, big));
        assert_eq!(received(Position::First), (big, small, big, big));
        assert_eq!(received(Position::Middle), (small, small, big, big));
        assert_eq!(received(Position::Last), (small, big, big, big));
        let mine = bubble_radius(true, Position::Last);
        assert_eq!((mine.top_right, mine.bottom_right, mine.top_left, mine.bottom_left), (small, big, big, big));
        let mine = bubble_radius(true, Position::First);
        assert_eq!((mine.top_right, mine.bottom_right), (big, small));
        assert!(Position::Last.ends_run() && Position::Single.ends_run());
        assert!(!Position::First.ends_run() && !Position::Middle.ends_run());
    }

    #[test]
    fn effect_names_fall_back_to_the_last_segment() {
        assert_eq!(effect_name("com.apple.MobileSMS.expressivesend.impact"), "Slam");
        assert_eq!(effect_name("com.apple.messages.effect.CKNewThing"), "CKNewThing");
    }
}
