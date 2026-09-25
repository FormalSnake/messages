//! Port of packages/core/src/search.ts: the sidebar query language
//! (`from:`, `has:`, `before:`, `after:`, `in:`).

use crate::model::{Chat, Contact, Millis};
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

/// An operator whose value fails to parse falls back to plain text.
pub fn parse_search_query(raw: &str) -> ParsedSearchQuery {
    let _ = raw;
    unimplemented!()
}

pub struct SearchContext<'a> {
    pub contacts: &'a [Contact],
    pub chats: &'a [std::sync::Arc<Chat>],
}

/// Resolves `from:` and `in:`. An `in:` with no match yields Some(empty) chat guids, which returns nothing.
pub fn resolve_search_query(parsed: &ParsedSearchQuery, context: &SearchContext<'_>) -> (String, SearchFilters) {
    let _ = (parsed, context);
    unimplemented!()
}
