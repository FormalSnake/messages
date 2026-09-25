//! Port of packages/core/src/fuzzy.ts: the Ctrl+K switcher's ranking.

use std::sync::Arc;

use crate::model::{chat_title, handle_name, Chat};

const WORD_START_BONUS: f64 = 8.0;
const CONSECUTIVE_BONUS: f64 = 6.0;
/// Keeps a long haystack from beating a short one on an otherwise equal match.
const LENGTH_PENALTY: f64 = 0.15;

/// Subsequence match; None when it does not match. Word starts and runs score higher, shorter text wins ties.
pub fn fuzzy_score(query: &str, text: &str) -> Option<f64> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Some(0.0);
    }
    let haystack: Vec<char> = text.to_lowercase().chars().collect();
    let mut cursor: usize = 0;
    let mut score: f64 = 0.0;
    let mut run: i64 = 0;
    for ch in needle.chars() {
        let found = haystack[cursor..].iter().position(|&c| c == ch).map(|i| i + cursor)?;
        let at_word_start = found == 0 || matches!(haystack[found - 1], ' ' | '-' | '@' | '.');
        run = if found == cursor { run + 1 } else { 1 };
        score += 1.0 + (run as f64) * CONSECUTIVE_BONUS + if at_word_start { WORD_START_BONUS } else { 0.0 };
        cursor = found + 1;
    }
    Some(score - (haystack.len() as f64) * LENGTH_PENALTY)
}

/// The best score for `query` across several fields naming the same thing, or None if none match.
fn best_score(query: &str, candidates: &[&str]) -> Option<f64> {
    let mut best: Option<f64> = None;
    for candidate in candidates {
        if candidate.is_empty() {
            continue;
        }
        if let Some(score) = fuzzy_score(query, candidate) {
            best = Some(best.map_or(score, |b| b.max(score)));
        }
    }
    best
}

/// Title, participant names and addresses, best field wins. Empty query returns the first `limit`.
pub fn search_chats(chats: &[Arc<Chat>], query: &str, limit: usize) -> Vec<Arc<Chat>> {
    let needle = query.trim();
    if needle.is_empty() {
        return chats.iter().take(limit).cloned().collect();
    }
    let mut scored: Vec<(Arc<Chat>, f64)> = Vec::new();
    for chat in chats {
        let title = chat_title(chat);
        let mut candidates: Vec<&str> = vec![&title];
        for participant in &chat.participants {
            candidates.push(handle_name(participant));
            candidates.push(&participant.address);
        }
        if let Some(score) = best_score(needle, &candidates) {
            scored.push((chat.clone(), score));
        }
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(limit).map(|(chat, _)| chat).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Handle, Service};

    fn chat(guid: &str, display_name: Option<&str>, participants: Vec<Handle>) -> Arc<Chat> {
        Arc::new(Chat {
            guid: guid.to_owned(),
            identifier: guid.to_owned(),
            group_id: None,
            service: Service::IMessage,
            is_group: false,
            display_name: display_name.map(str::to_owned),
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

    fn handle(address: &str, name: Option<&str>) -> Handle {
        Handle { address: address.to_owned(), service: Service::IMessage, name: name.map(str::to_owned), avatar: None }
    }

    #[test]
    fn matches_a_subsequence_regardless_of_case() {
        assert!(fuzzy_score("mrn", "Maureen").is_some());
    }

    #[test]
    fn rejects_characters_out_of_order() {
        assert!(fuzzy_score("nam", "Maureen").is_none());
    }

    #[test]
    fn rejects_a_character_missing_entirely() {
        assert!(fuzzy_score("zz", "Maureen").is_none());
    }

    #[test]
    fn empty_query_matches_everything_with_a_zero_score() {
        assert_eq!(fuzzy_score("", "Maureen"), Some(0.0));
    }

    #[test]
    fn scores_a_match_at_a_word_start_higher_than_one_mid_word() {
        let word_start = fuzzy_score("m", "Cape May").unwrap();
        let mid_word = fuzzy_score("m", "Cape Cod Marina").unwrap();
        assert!(word_start > mid_word);
    }

    #[test]
    fn prefers_a_shorter_title_for_an_equally_good_match() {
        let short = fuzzy_score("ann", "Ann").unwrap();
        let long = fuzzy_score("ann", "Ann Marie Fitzgerald-Robinson").unwrap();
        assert!(short > long);
    }

    #[test]
    fn a_longer_consecutive_run_scores_higher_than_the_same_letters_scattered() {
        let consecutive = fuzzy_score("ann", "Ann Smith").unwrap();
        let scattered = fuzzy_score("ann", "Anna Nolan Nunez").unwrap();
        assert!(consecutive > scattered);
    }

    fn chats() -> Vec<Arc<Chat>> {
        vec![
            chat("1", Some("Weekend trip"), vec![]),
            chat("2", None, vec![handle("+15551234567", Some("Maureen Doyle"))]),
            chat("3", None, vec![handle("sam@example.com", None)]),
        ]
    }

    #[test]
    fn returns_every_chat_most_recently_active_first_for_an_empty_query() {
        let guids: Vec<String> = search_chats(&chats(), "", 8).into_iter().map(|c| c.guid.clone()).collect();
        assert_eq!(guids, vec!["1", "2", "3"]);
    }

    #[test]
    fn matches_a_chat_by_its_display_title() {
        let guids: Vec<String> = search_chats(&chats(), "weekend", 8).into_iter().map(|c| c.guid.clone()).collect();
        assert_eq!(guids, vec!["1"]);
    }

    #[test]
    fn matches_a_chat_by_a_participant_name() {
        let guids: Vec<String> = search_chats(&chats(), "maur", 8).into_iter().map(|c| c.guid.clone()).collect();
        assert_eq!(guids, vec!["2"]);
    }

    #[test]
    fn matches_a_chat_by_a_participant_address() {
        let guids: Vec<String> = search_chats(&chats(), "sam@example", 8).into_iter().map(|c| c.guid.clone()).collect();
        assert_eq!(guids, vec!["3"]);
    }

    #[test]
    fn drops_chats_that_match_nothing() {
        assert!(search_chats(&chats(), "zzzzz", 8).is_empty());
    }

    #[test]
    fn caps_results_at_the_given_limit() {
        let many: Vec<Arc<Chat>> = (0..12).map(|i| chat(&format!("m{i}"), Some("Team standup"), vec![])).collect();
        assert_eq!(search_chats(&many, "team", 8).len(), 8);
    }
}
