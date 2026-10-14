//! Cmd/Ctrl+K: jump to a conversation. nd's command palette (the Spotlight-style panel on
//! macOS) over the conversations, ranked by the core's fuzzy search, eight at most.

use messages_core::conversations::conversation_chats;
use messages_core::fuzzy::search_chats;
use messages_core::{MessagesStore, chat_title};
use nd::prelude::*;

use crate::actions::OpenChat;
use crate::root::Root;

const LIMIT: usize = 8;

#[derive(Default)]
pub struct Switcher {
    pub open: bool,
    query: SharedString,
}

impl Switcher {
    pub fn open(&mut self) {
        self.open = true;
        self.query = SharedString::default();
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    fn items(&self, store: Option<&MessagesStore>) -> Vec<PaletteItem> {
        let Some(store) = store else { return Vec::new() };
        let state = store.state();
        let chats = conversation_chats(&state);
        let ranked = if self.query.trim().is_empty() {
            chats.into_iter().take(LIMIT).collect()
        } else {
            search_chats(&chats, self.query.trim(), LIMIT)
        };
        ranked
            .iter()
            .map(|chat| {
                let preview = chat
                    .last_message
                    .as_ref()
                    .map(|m| crate::ui::clamp_preview(&crate::sidebar::preview_text(Some(m), chat), 60))
                    .unwrap_or_default();
                palette_item(chat.guid.clone(), chat_title(chat)).subtitle(preview).icon(Icon::User)
            })
            .collect()
    }

    pub fn element(&self, store: Option<&MessagesStore>, cx: &mut Context<Root>) -> AnyElement {
        command_palette("switcher")
            .open(self.open)
            .placeholder("Jump to a conversation")
            .query(self.query.clone())
            .items(self.items(store))
            .on_query_change(cx.listener(|this, e: &PaletteQueryChanged, _, cx| {
                this.switcher_mut().query = e.text.clone();
                cx.notify();
            }))
            .on_activate(cx.listener(|this, e: &PaletteActivated, window, cx| {
                this.switcher_mut().close();
                crate::ui::dispatch(window, cx, OpenChat(e.id.to_string()));
                cx.notify();
            }))
            .on_cancel(cx.listener(|this, _: &PaletteCancelled, _, cx| {
                this.switcher_mut().close();
                cx.notify();
            }))
            .into_any_element()
    }
}
