//! chat.db keeps one chat per address, so a person texted on two numbers shows
//! up twice. Those fold into one conversation: the most recently active chat is
//! the primary the sidebar lists and sends go to; the others lend their messages.

use std::collections::HashMap;
use std::sync::Arc;

use crate::findmy::normalize_address;
use crate::model::{Chat, Contact, FocusStatus, Handle, Message};
use crate::store::AppState;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Grouping {
    /// Member guid to primary guid, only for chats in a merged conversation.
    pub primary_of: HashMap<String, String>,
    /// Primary guid to every member guid, itself first.
    pub merged: HashMap<String, Vec<String>>,
}

/// Normalized address to the id of the contact that owns it.
pub(crate) fn contact_owners(contacts: &[Contact]) -> HashMap<String, String> {
    let mut owner = HashMap::new();
    for contact in contacts {
        for address in &contact.addresses {
            owner.insert(normalize_address(address), contact.id.clone());
        }
    }
    owner
}

/// Groups one-to-one chats by contact id, else by normalized address. Alphanumeric sender ids stay apart.
pub fn group_chats(chats: &[Arc<Chat>], contacts: &[Contact]) -> Grouping {
    group_chats_with(chats, &contact_owners(contacts))
}

pub(crate) fn group_chats_with(chats: &[Arc<Chat>], owner: &HashMap<String, String>) -> Grouping {
    let mut by_key: HashMap<String, Vec<&Chat>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for chat in chats {
        if chat.is_group || chat.participants.len() != 1 {
            continue;
        }
        let raw = &chat.participants[0].address;
        let address = normalize_address(raw);
        // Alphanumeric sender ids ("Wise") normalise to nothing; they stay apart.
        let key = if address.is_empty() {
            format!("id:{}", raw.to_lowercase())
        } else {
            owner.get(&address).cloned().unwrap_or_else(|| format!("address:{address}"))
        };
        match by_key.get_mut(&key) {
            Some(list) => list.push(chat),
            None => {
                order.push(key.clone());
                by_key.insert(key, vec![chat]);
            }
        }
    }
    let mut grouping = Grouping::default();
    for key in order {
        let mut members = by_key.remove(&key).unwrap_or_default();
        if members.len() < 2 {
            continue;
        }
        members.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
        let primary = members[0].guid.clone();
        for chat in &members {
            grouping.primary_of.insert(chat.guid.clone(), primary.clone());
        }
        grouping.merged.insert(primary, members.iter().map(|chat| chat.guid.clone()).collect());
    }
    grouping
}

pub fn conversation_guid<'a>(grouping: &'a Grouping, guid: &'a str) -> &'a str {
    grouping.primary_of.get(guid).map(String::as_str).unwrap_or(guid)
}

pub fn conversation_members(grouping: &Grouping, guid: &str) -> Vec<String> {
    match grouping.merged.get(conversation_guid(grouping, guid)) {
        Some(members) => members.clone(),
        None => vec![guid.to_owned()],
    }
}

/// A stable key for a conversation: the same whichever member is primary today.
pub fn conversation_key(grouping: &Grouping, guid: &str) -> String {
    conversation_members(grouping, guid).into_iter().min().unwrap_or_else(|| guid.to_owned())
}

/// The rows the sidebar shows: every chat not folded into another, in `state.chats` order.
pub fn conversation_chats(state: &AppState) -> Vec<Arc<Chat>> {
    state
        .chats
        .iter()
        .filter(|chat| conversation_guid(&state.grouping, &chat.guid) == chat.guid)
        .cloned()
        .collect()
}

/// Every loaded message across the members, oldest first.
pub fn conversation_messages(state: &AppState, guid: &str) -> Vec<Arc<Message>> {
    let members = conversation_members(&state.grouping, guid);
    if members.len() == 1 {
        return state.messages.get(&members[0]).cloned().unwrap_or_default();
    }
    let mut all: Vec<Arc<Message>> = members.iter().filter_map(|member| state.messages.get(member)).flatten().cloned().collect();
    all.sort_by_key(|message| message.date);
    all
}

pub fn conversation_unread(state: &AppState, guid: &str) -> bool {
    let members = conversation_members(&state.grouping, guid);
    members.iter().any(|member| state.chat(member).is_some_and(|chat| chat.unread))
}

pub fn conversation_typing(state: &AppState, guid: &str) -> bool {
    conversation_members(&state.grouping, guid).iter().any(|member| state.typing.contains(member))
}

/// True while any member has not reported `has_older == false`.
pub fn conversation_has_older(state: &AppState, guid: &str) -> bool {
    conversation_members(&state.grouping, guid).iter().any(|member| state.has_older.get(member) != Some(&false))
}

pub fn conversation_loading(state: &AppState, guid: &str) -> bool {
    conversation_members(&state.grouping, guid).iter().any(|member| state.loading.contains(member))
}

/// The addresses the conversation reaches the person on, primary first, deduplicated.
pub fn conversation_handles(state: &AppState, guid: &str) -> Vec<Handle> {
    let mut handles: Vec<Handle> = Vec::new();
    for member in conversation_members(&state.grouping, guid) {
        let Some(chat) = state.chat(&member) else { continue };
        for handle in &chat.participants {
            if !handles.iter().any(|item| item.address == handle.address) {
                handles.push(handle.clone());
            }
        }
    }
    handles
}

/// The key a Focus answer is kept under: one person, however many numbers they have.
pub fn focus_key(address: &str) -> String {
    let normalized = normalize_address(address);
    if normalized.is_empty() { address.to_lowercase() } else { normalized }
}

/// The Focus of the person on the other end. A group, or anyone not asked about, is Unknown.
pub fn conversation_focus(state: &AppState, guid: &str) -> FocusStatus {
    let Some(chat) = state.chat(conversation_guid(&state.grouping, guid)) else {
        return FocusStatus::Unknown;
    };
    if chat.is_group {
        return FocusStatus::Unknown;
    }
    let handles = conversation_handles(state, guid);
    let Some(first) = handles.first() else {
        return FocusStatus::Unknown;
    };
    state.focus.get(&focus_key(&first.address)).copied().unwrap_or(FocusStatus::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Service;

    fn chat(guid: &str, address: &str, last_activity: i64) -> Arc<Chat> {
        Arc::new(Chat {
            guid: guid.into(),
            identifier: address.into(),
            participants: vec![Handle { address: address.into(), service: Service::IMessage, name: None, avatar: None }],
            last_activity,
            ..Chat::default()
        })
    }

    #[test]
    fn folds_chats_that_share_a_contact_and_keeps_the_newest_primary() {
        let chats = vec![chat("phone", "+32470000001", 5000), chat("mail", "papa@example.com", 1000), chat("other", "+15550000", 10)];
        let contacts = vec![Contact { id: "papa".into(), name: "Papa".into(), addresses: vec!["+32 470 00 00 01".into(), "papa@example.com".into()], avatar: None }];
        let grouping = group_chats(&chats, &contacts);
        assert_eq!(grouping.merged.get("phone"), Some(&vec!["phone".to_owned(), "mail".to_owned()]));
        assert_eq!(conversation_guid(&grouping, "mail"), "phone");
        assert_eq!(conversation_guid(&grouping, "other"), "other");
        assert_eq!(conversation_members(&grouping, "other"), vec!["other".to_owned()]);
        assert_eq!(conversation_key(&grouping, "phone"), "mail");
    }

    #[test]
    fn folds_the_same_address_without_a_contact_and_keeps_sender_ids_apart() {
        let chats = vec![chat("sms", "+15550101", 10), chat("imsg", "+1 555 0101", 20), chat("wise", "Wise", 5), chat("wise2", "Wise", 4)];
        let grouping = group_chats(&chats, &[]);
        assert_eq!(conversation_guid(&grouping, "sms"), "imsg");
        // Alphanumeric sender ids normalise to nothing and match only on their raw id.
        assert_eq!(conversation_guid(&grouping, "wise2"), "wise");
    }

    #[test]
    fn focus_key_falls_back_to_the_lowercased_address() {
        assert_eq!(focus_key("Wise"), "wise");
        assert_eq!(focus_key("+34 600 111 222"), "600111222");
    }
}
