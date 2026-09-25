//! Port of packages/core/src/conversations.ts.
//!
//! chat.db keeps one chat per address, so a person texted on two numbers shows
//! up twice. Those fold into one conversation: the most recently active chat is
//! the primary the sidebar lists and sends go to; the others lend their messages.

use std::collections::HashMap;
use std::sync::Arc;

use crate::model::{Chat, Contact, FocusStatus, Handle, Message};
use crate::store::AppState;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Grouping {
    /// Member guid to primary guid, only for chats in a merged conversation.
    pub primary_of: HashMap<String, String>,
    /// Primary guid to every member guid, itself first.
    pub merged: HashMap<String, Vec<String>>,
}

/// Groups one-to-one chats by contact id, else by normalized address. Alphanumeric sender ids stay apart.
pub fn group_chats(chats: &[Arc<Chat>], contacts: &[Contact]) -> Grouping {
    let _ = (chats, contacts);
    unimplemented!()
}

pub fn conversation_guid<'a>(grouping: &'a Grouping, guid: &'a str) -> &'a str {
    let _ = (grouping, guid);
    unimplemented!()
}

pub fn conversation_members(grouping: &Grouping, guid: &str) -> Vec<String> {
    let _ = (grouping, guid);
    unimplemented!()
}

/// A stable key for a conversation: the same whichever member is primary today.
pub fn conversation_key(grouping: &Grouping, guid: &str) -> String {
    let _ = (grouping, guid);
    unimplemented!()
}

/// The rows the sidebar shows: every chat not folded into another, in `state.chats` order.
pub fn conversation_chats(state: &AppState) -> Vec<Arc<Chat>> {
    let _ = state;
    unimplemented!()
}

/// Every loaded message across the members, oldest first.
pub fn conversation_messages(state: &AppState, guid: &str) -> Vec<Arc<Message>> {
    let _ = (state, guid);
    unimplemented!()
}

pub fn conversation_unread(state: &AppState, guid: &str) -> bool {
    let _ = (state, guid);
    unimplemented!()
}

pub fn conversation_typing(state: &AppState, guid: &str) -> bool {
    let _ = (state, guid);
    unimplemented!()
}

/// True while any member has not reported `has_older == false`.
pub fn conversation_has_older(state: &AppState, guid: &str) -> bool {
    let _ = (state, guid);
    unimplemented!()
}

pub fn conversation_loading(state: &AppState, guid: &str) -> bool {
    let _ = (state, guid);
    unimplemented!()
}

/// The addresses the conversation reaches the person on, primary first, deduplicated.
pub fn conversation_handles(state: &AppState, guid: &str) -> Vec<Handle> {
    let _ = (state, guid);
    unimplemented!()
}

/// The key a Focus answer is kept under: one person, however many numbers they have.
pub fn focus_key(address: &str) -> String {
    let _ = address;
    unimplemented!()
}

/// The Focus of the person on the other end. A group, or anyone not asked about, is Unknown.
pub fn conversation_focus(state: &AppState, guid: &str) -> FocusStatus {
    let _ = (state, guid);
    unimplemented!()
}
