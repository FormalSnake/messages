//! Port of packages/core/src/fuzzy.ts: the Ctrl+K switcher's ranking.

use std::sync::Arc;

use crate::model::Chat;

/// Subsequence match; None when it does not match. Word starts and runs score higher, shorter text wins ties.
pub fn fuzzy_score(query: &str, text: &str) -> Option<f64> {
    let _ = (query, text);
    unimplemented!()
}

/// Title, participant names and addresses, best field wins. Empty query returns the first `limit`.
pub fn search_chats(chats: &[Arc<Chat>], query: &str, limit: usize) -> Vec<Arc<Chat>> {
    let _ = (chats, query, limit);
    unimplemented!()
}
