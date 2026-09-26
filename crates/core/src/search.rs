//! The sidebar query language (`from:`, `has:`, `before:`, `after:`, `in:`).

use std::collections::HashSet;

use chrono::NaiveDate;

use crate::model::{chat_title, Chat, Contact, Millis};
use crate::transport::{AttachmentFilter, SearchFilters};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedSearchQuery {
    pub text: String,
    pub from_me: bool,
    /// Raw `from:` values, not yet resolved to addresses.
    pub senders: Vec<String>,
    pub attachments: Option<AttachmentFilter>,
    pub links: bool,
    /// Local midnight of the date, inclusive.
    pub after: Option<Millis>,
    /// Local midnight of the date, exclusive.
    pub before: Option<Millis>,
    /// Raw `in:` values, not yet resolved to chat guids.
    pub chat_names: Vec<String>,
}

fn parse_local_date(value: &str) -> Option<Millis> {
    // Exactly YYYY-MM-DD; chrono alone would take `2026-9-3`.
    if value.len() != 10 || !value.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() }) {
        return None;
    }
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
    Some(crate::format::local_datetime(date.and_hms_opt(0, 0, 0)?).timestamp_millis())
}

/// Splits on whitespace, keeping `key:value` together and letting the value be quoted for a multi-word name.
fn tokenize(raw: &str) -> Vec<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r#"([a-zA-Z]+):"([^"]*)"|(\S+)"#).unwrap());
    let mut tokens = Vec::new();
    for caps in re.captures_iter(raw) {
        if let (Some(key), Some(value)) = (caps.get(1), caps.get(2)) {
            tokens.push(format!("{}:{}", key.as_str(), value.as_str()));
        } else if let Some(bare) = caps.get(3) {
            tokens.push(bare.as_str().to_owned());
        }
    }
    tokens
}

/// An operator whose value fails to parse falls back to plain text.
pub fn parse_search_query(raw: &str) -> ParsedSearchQuery {
    let mut parsed = ParsedSearchQuery::default();
    let mut words: Vec<String> = Vec::new();
    for token in tokenize(raw) {
        let colon = token.find(':');
        let (key, value) = match colon {
            Some(idx) if idx > 0 => (token[..idx].to_lowercase(), token[idx + 1..].to_owned()),
            _ => (String::new(), String::new()),
        };
        if key == "from" && value.to_lowercase() == "me" {
            parsed.from_me = true;
        } else if key == "from" && !value.is_empty() {
            parsed.senders.push(value);
        } else if key == "has" && (value == "photo" || value == "image") {
            parsed.attachments = Some(AttachmentFilter::Image);
        } else if key == "has" && value == "video" {
            parsed.attachments = Some(AttachmentFilter::Video);
        } else if key == "has" && value == "file" {
            parsed.attachments = Some(AttachmentFilter::File);
        } else if key == "has" && value == "link" {
            parsed.links = true;
        } else if key == "before" && parse_local_date(&value).is_some() {
            parsed.before = parse_local_date(&value);
        } else if key == "after" && parse_local_date(&value).is_some() {
            parsed.after = parse_local_date(&value);
        } else if key == "in" && !value.is_empty() {
            parsed.chat_names.push(value);
        } else {
            words.push(token);
        }
    }
    parsed.text = words.join(" ");
    parsed
}

pub struct SearchContext<'a> {
    pub contacts: &'a [Contact],
    pub chats: &'a [std::sync::Arc<Chat>],
}

/// A contact whose name contains the token, every one of its addresses. Falls back to the token itself, in case it is already an address.
fn resolve_sender(token: &str, context: &SearchContext<'_>) -> Vec<String> {
    let needle = token.to_lowercase();
    let contact_matches: Vec<&Contact> = context.contacts.iter().filter(|c| c.name.to_lowercase().contains(&needle)).collect();
    if !contact_matches.is_empty() {
        return contact_matches.into_iter().flat_map(|c| c.addresses.clone()).collect();
    }
    let mut handle_addresses: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for chat in context.chats {
        for handle in &chat.participants {
            if handle.name.as_deref().is_some_and(|name| name.to_lowercase().contains(&needle)) && seen.insert(handle.address.clone()) {
                handle_addresses.push(handle.address.clone());
            }
        }
    }
    if !handle_addresses.is_empty() {
        handle_addresses
    } else {
        vec![token.to_owned()]
    }
}

fn resolve_chat_name(token: &str, context: &SearchContext<'_>) -> Vec<String> {
    let needle = token.to_lowercase();
    context.chats.iter().filter(|chat| chat_title(chat).to_lowercase().contains(&needle)).map(|chat| chat.guid.clone()).collect()
}

/// Resolves `from:` and `in:`. An `in:` with no match yields Some(empty) chat guids, which returns nothing.
pub fn resolve_search_query(parsed: &ParsedSearchQuery, context: &SearchContext<'_>) -> (String, SearchFilters) {
    let mut filters = SearchFilters::default();
    if parsed.from_me {
        filters.from_me = true;
    }
    if !parsed.senders.is_empty() {
        filters.senders = parsed.senders.iter().flat_map(|token| resolve_sender(token, context)).collect();
    }
    if parsed.attachments.is_some() {
        filters.attachments = parsed.attachments;
    }
    if parsed.links {
        filters.links = true;
    }
    // SearchFilters.after/before are inclusive on both ends, matching the
    // server's own date query. after:DATE ("on or after that day") maps
    // straight onto it; before:DATE ("before that day") excludes the day
    // itself, so its boundary moves back one millisecond.
    if let Some(after) = parsed.after {
        filters.after = Some(after);
    }
    if let Some(before) = parsed.before {
        filters.before = Some(before - 1);
    }
    if !parsed.chat_names.is_empty() {
        filters.chat_guids = Some(parsed.chat_names.iter().flat_map(|token| resolve_chat_name(token, context)).collect());
    }
    (parsed.text.clone(), filters)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Handle, Service};
    use std::sync::Arc;

    fn contact(name: &str, addresses: &[&str]) -> Contact {
        Contact { id: name.to_owned(), name: name.to_owned(), addresses: addresses.iter().map(|s| s.to_string()).collect(), avatar: None }
    }

    fn handle(address: &str, name: Option<&str>) -> Handle {
        Handle { address: address.to_owned(), service: Service::IMessage, name: name.map(str::to_owned), avatar: None }
    }

    fn chat(guid: &str, participants: Vec<Handle>) -> Arc<Chat> {
        Arc::new(Chat {
            guid: guid.to_owned(),
            identifier: guid.to_owned(),
            group_id: None,
            service: Service::IMessage,
            is_group: false,
            display_name: None,
            icon: None,
            participants,
            pinned: false,
            muted: false,
            read_receipts: None,
            archived: false,
            unread: false,
            last_message: None,
            last_activity: 0,
        })
    }

    #[test]
    fn plain_text_with_no_operators() {
        let parsed = parse_search_query("coffee tomorrow");
        assert_eq!(parsed.text, "coffee tomorrow");
        assert!(!parsed.from_me);
        assert!(parsed.senders.is_empty());
    }

    #[test]
    fn from_me_sets_from_me_and_leaves_no_sender() {
        let parsed = parse_search_query("from:me lunch");
        assert!(parsed.from_me);
        assert!(parsed.senders.is_empty());
        assert_eq!(parsed.text, "lunch");
    }

    #[test]
    fn from_name_collects_the_raw_token_unresolved() {
        let parsed = parse_search_query("from:Alex bike");
        assert_eq!(parsed.senders, vec!["Alex"]);
        assert_eq!(parsed.text, "bike");
    }

    #[test]
    fn a_quoted_from_value_keeps_its_spaces_as_one_token() {
        let parsed = parse_search_query(r#"from:"Priya Natarajan" deck"#);
        assert_eq!(parsed.senders, vec!["Priya Natarajan"]);
        assert_eq!(parsed.text, "deck");
    }

    #[test]
    fn has_photo_video_file_link() {
        assert_eq!(parse_search_query("has:photo").attachments, Some(AttachmentFilter::Image));
        assert_eq!(parse_search_query("has:video").attachments, Some(AttachmentFilter::Video));
        assert_eq!(parse_search_query("has:file").attachments, Some(AttachmentFilter::File));
        assert!(parse_search_query("has:link").links);
    }

    #[test]
    fn an_unknown_has_value_falls_back_to_plain_text() {
        let parsed = parse_search_query("has:sticker");
        assert_eq!(parsed.attachments, None);
        assert_eq!(parsed.text, "has:sticker");
    }

    #[test]
    fn before_and_after_parse_a_local_date_to_midnight() {
        let parsed = parse_search_query("after:2024-01-15 before:2024-02-01");
        assert_eq!(parsed.after, parse_local_date("2024-01-15"));
        assert_eq!(parsed.before, parse_local_date("2024-02-01"));
    }

    #[test]
    fn a_malformed_date_falls_back_to_plain_text() {
        let parsed = parse_search_query("before:not-a-date");
        assert_eq!(parsed.before, None);
        assert_eq!(parsed.text, "before:not-a-date");
    }

    #[test]
    fn in_chat_name_collects_the_raw_token() {
        let parsed = parse_search_query("in:Family lunch");
        assert_eq!(parsed.chat_names, vec!["Family"]);
        assert_eq!(parsed.text, "lunch");
    }

    #[test]
    fn every_operator_combines_with_free_text() {
        let parsed = parse_search_query("from:me has:photo after:2024-01-01 in:Family the hike photos");
        assert_eq!(parsed.text, "the hike photos");
        assert!(parsed.from_me);
        assert!(parsed.senders.is_empty());
        assert_eq!(parsed.attachments, Some(AttachmentFilter::Image));
        assert!(!parsed.links);
        assert_eq!(parsed.after, parse_local_date("2024-01-01"));
        assert_eq!(parsed.chat_names, vec!["Family"]);
    }

    fn context() -> (Vec<Contact>, Vec<Arc<Chat>>) {
        let contacts = vec![contact("Alex Rivera", &["+14155550134", "alex@example.com"]), contact("Priya Natarajan", &["priya@example.com"])];
        let chats = vec![
            chat("iMessage;-;+14155550188", vec![handle("+14155550188", Some("Jordan Lee"))]),
            chat("iMessage;+;chat1", vec![handle("+14155550101", Some("Mom"))]),
        ];
        (contacts, chats)
    }

    #[test]
    fn a_from_name_resolves_to_every_address_of_the_matching_contact() {
        let (contacts, chats) = context();
        let ctx = SearchContext { contacts: &contacts, chats: &chats };
        let (_, filters) = resolve_search_query(&parse_search_query("from:Alex"), &ctx);
        assert_eq!(filters.senders, vec!["+14155550134", "alex@example.com"]);
    }

    #[test]
    fn a_from_name_with_no_contact_match_falls_back_to_a_chat_participant() {
        let (contacts, chats) = context();
        let ctx = SearchContext { contacts: &contacts, chats: &chats };
        let (_, filters) = resolve_search_query(&parse_search_query("from:Jordan"), &ctx);
        assert_eq!(filters.senders, vec!["+14155550188"]);
    }

    #[test]
    fn a_from_value_with_no_match_anywhere_is_passed_through_as_a_literal_address() {
        let (contacts, chats) = context();
        let ctx = SearchContext { contacts: &contacts, chats: &chats };
        let (_, filters) = resolve_search_query(&parse_search_query("from:+14155559999"), &ctx);
        assert_eq!(filters.senders, vec!["+14155559999"]);
    }

    #[test]
    fn from_me_sets_from_me_with_no_senders_filter() {
        let (contacts, chats) = context();
        let ctx = SearchContext { contacts: &contacts, chats: &chats };
        let (_, filters) = resolve_search_query(&parse_search_query("from:me"), &ctx);
        assert!(filters.from_me);
        assert!(filters.senders.is_empty());
    }

    #[test]
    fn in_chat_name_resolves_to_matching_chat_guids() {
        let (contacts, chats) = context();
        let ctx = SearchContext { contacts: &contacts, chats: &chats };
        let (text, filters) = resolve_search_query(&parse_search_query("in:Family sunday"), &ctx);
        assert_eq!(filters.chat_guids, Some(vec![]));
        assert_eq!(text, "sunday");
    }

    #[test]
    fn in_mom_resolves_to_the_chat_whose_participant_is_named_that() {
        let (contacts, chats) = context();
        let ctx = SearchContext { contacts: &contacts, chats: &chats };
        let (_, filters) = resolve_search_query(&parse_search_query("in:Mom"), &ctx);
        assert_eq!(filters.chat_guids, Some(vec!["iMessage;+;chat1".to_owned()]));
    }

    #[test]
    fn before_shifts_back_one_millisecond_so_the_named_day_is_excluded() {
        let (contacts, chats) = context();
        let ctx = SearchContext { contacts: &contacts, chats: &chats };
        let midnight = parse_local_date("2024-02-01").unwrap();
        let (_, filters) = resolve_search_query(&parse_search_query("before:2024-02-01"), &ctx);
        assert_eq!(filters.before, Some(midnight - 1));
    }

    #[test]
    fn no_operators_leaves_an_empty_filters_object() {
        let (contacts, chats) = context();
        let ctx = SearchContext { contacts: &contacts, chats: &chats };
        let (text, filters) = resolve_search_query(&parse_search_query("just words"), &ctx);
        assert_eq!(filters, SearchFilters::default());
        assert_eq!(text, "just words");
    }
}
