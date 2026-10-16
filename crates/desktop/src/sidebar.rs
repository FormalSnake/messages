//! Search, the pinned strip, the conversation list and the connection footer. The list is nd's
//! virtual list (an `NSTableView` on macOS): rows are keyed by conversation and versioned by what
//! they show, so a message in one chat re-renders that chat's row and nothing else.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use messages_core::conversations::{conversation_chats, conversation_focus, conversation_typing, conversation_unread};
use messages_core::format::format_list_date;
use messages_core::search::{SearchContext, parse_search_query, resolve_search_query};
use messages_core::transport::{ConnectionStatus, TransportKind};
use messages_core::{Chat, FocusStatus, GroupEvent, Message, chat_title, handle_name};
use nd::prelude::*;

use crate::actions::*;
use crate::bridge::{self, Bridge, Topic};
use crate::ui;

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(250);
/// Messages' sidebar row: a 40 pt avatar on an 80 pt row, the unread dot in a 20 pt gutter.
const AVATAR_ROW: f32 = 40.;
const ROW_HEIGHT: f32 = 80.;
const PINNED_AVATAR: f32 = 52.;
const PINNED_CELL: f32 = 92.;
/// Long enough that two lines of a row clip it, short enough to keep a novel out of the label.
const PREVIEW_CHARS: usize = 300;
const SEARCH_FAILED: &str = "Search failed. Try again.";
const OPERATOR_TIPS: [&str; 4] = [
    "from:name or from:me",
    "has:photo, has:video, has:file or has:link",
    "before:2024-01-01, after:2024-01-01",
    "in:chat name",
];

/// One row of the list.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Item {
    Chat(String),
    ResultsHeader,
    /// A search hit, by message guid.
    Result(String),
    Note,
}

pub struct Sidebar {
    list: ListState<Item>,
    query: SharedString,
    results: Vec<Message>,
    search_error: Option<String>,
    search_epoch: u64,
    /// What the note row says when the list shows one.
    note: Option<(SharedString, Option<SharedString>)>,
    selected: Option<String>,
}

fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

/// The sidebar's line for a chat's last message.
pub fn preview_text(message: Option<&Message>, chat: &Chat) -> String {
    let Some(message) = message else {
        return if chat.is_group { "New group".to_owned() } else { "New conversation".to_owned() };
    };
    let who = if message.from_me {
        "You".to_owned()
    } else if let Some(sender) = &message.sender {
        first_name(handle_name(sender)).to_owned()
    } else {
        String::new()
    };
    if message.date_retracted.is_some() {
        return format!("{who} unsent a message");
    }
    if let Some(event) = &message.group_event {
        return match event {
            GroupEvent::Rename { title } => format!("{who} named the conversation \u{201c}{title}\u{201d}"),
            GroupEvent::Join { who: joined } => {
                format!("{who} added {}", joined.as_ref().map(|h| handle_name(h)).unwrap_or("someone"))
            }
            GroupEvent::Leave { who: left } => {
                format!("{} left the conversation", left.as_ref().map(|h| handle_name(h)).unwrap_or(who.as_str()))
            }
            GroupEvent::Photo => format!("{who} changed the group photo"),
        };
    }
    let mut body = message.text.split_whitespace().collect::<Vec<_>>().join(" ");
    if body.is_empty()
        && let Some(first) = message.attachments.first()
    {
        body = if message.is_audio {
            "Audio message".to_owned()
        } else if first.mime.starts_with("image/") {
            "Photo".to_owned()
        } else if first.mime.starts_with("video/") {
            "Video".to_owned()
        } else {
            first.name.clone()
        };
    }
    if chat.is_group && !message.from_me && !who.is_empty() {
        return format!("{who}: {body}");
    }
    body
}

/// A conversation's context menu, shared by its row, its pinned cell and the header.
pub fn chat_menu(menu: ContextMenu, chat: &Chat, open: bool) -> ContextMenu {
    let guid = chat.guid.clone();
    let menu = if open { menu.item("Open Conversation", OpenChat(guid.clone())).separator() } else { menu };
    let menu = menu
        .item(if chat.pinned { "Unpin" } else { "Pin" }, TogglePin(guid.clone()))
        .item(if chat.muted { "Show Alerts" } else { "Hide Alerts" }, ToggleMute(guid.clone()))
        .item(
            if chat.read_receipts == Some(false) { "Send Read Receipts" } else { "Read Without Receipts" },
            ToggleReceipts(guid.clone()),
        );
    let menu = if chat.unread {
        menu.item("Mark as Read", MarkChatRead(guid.clone()))
    } else {
        menu.item("Mark as Unread", MarkChatUnread(guid.clone()))
    };
    menu.item("Show Details", ShowDetails(guid.clone()))
        .item("Export Conversation\u{2026}", ExportChat(guid.clone()))
        .separator()
        .item_destructive("Delete Conversation", DeleteChat(guid))
}

/// The version a chat row renders at: anything it shows that can change.
fn row_version(chat: &Arc<Chat>, typing: bool, unread: bool, silenced: bool, contacts: usize, selected: bool) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (Arc::as_ptr(chat) as usize, typing, unread, silenced, contacts, selected).hash(&mut hasher);
    hasher.finish()
}

impl Sidebar {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.weak_entity();
        for topic in [Topic::ChatList, Topic::Selection, Topic::Connection, Topic::Contacts, Topic::Focus] {
            Bridge::watch(cx, topic, &weak);
        }
        Self {
            list: ListState::new(cx).estimated_row_height(px(64.)),
            query: SharedString::default(),
            results: Vec::new(),
            search_error: None,
            search_epoch: 0,
            note: None,
            selected: None,
        }
    }

    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        ui::focus_id(window, cx, "sidebar-search");
    }

    fn set_query(&mut self, query: SharedString, cx: &mut Context<Self>) {
        if self.query == query {
            return;
        }
        self.query = query;
        self.search_epoch += 1;
        let epoch = self.search_epoch;
        let weak = cx.weak_entity();
        crate::rt::after(SEARCH_DEBOUNCE, move |cx| {
            weak.update(cx, |this, cx| {
                if this.search_epoch == epoch {
                    this.run_search(epoch, cx);
                }
            });
        });
        cx.notify();
    }

    fn run_search(&mut self, epoch: u64, cx: &mut Context<Self>) {
        let Some(store) = bridge::store(cx) else { return };
        let query = self.query.trim().to_owned();
        let parsed = parse_search_query(&query);
        if parsed.text.trim().len() < 2 && !has_filter(&parsed) {
            if !self.results.is_empty() || self.search_error.is_some() {
                self.results.clear();
                self.search_error = None;
                cx.notify();
            }
            return;
        }
        let (text, mut filters) = {
            let state = store.state();
            resolve_search_query(&parsed, &SearchContext { contacts: &state.contacts, chats: &state.chats })
        };
        filters.limit = Some(20);
        let search = store.clone();
        crate::rt::spawn_for(
            cx.weak_entity(),
            async move { search.transport().search_messages(&text, &filters).await },
            move |this, outcome, cx| {
                if this.search_epoch != epoch {
                    return;
                }
                match outcome {
                    Ok(messages) => {
                        this.results = messages;
                        this.search_error = None;
                    }
                    Err(_) => {
                        this.results.clear();
                        this.search_error = Some(SEARCH_FAILED.to_owned());
                    }
                }
                cx.notify();
            },
        );
    }

    fn open(guid: &str, window: &mut Window, cx: &mut App) {
        crate::ui::dispatch(window, cx, OpenChat(guid.to_owned()));
    }

    fn row(&mut self, item: Item, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match item {
            Item::Chat(guid) => {
                let selected = self.selected.as_deref() == Some(guid.as_str());
                chat_row(&guid, selected, cx)
            }
            Item::ResultsHeader => {
                label("Messages").text_style(TextStyle::Caption1).secondary().ml_3().mt_3().into_any_element()
            }
            Item::Result(message) => self.result_row(&message, cx),
            Item::Note => {
                let (title, body) = self.note.clone().unwrap_or_default();
                v_flex()
                    .items_center()
                    .gap_1()
                    .pt_8()
                    .px_4()
                    .child(label(title).text_style(TextStyle::Headline))
                    .children(body.map(|body| label(body).secondary().wraps()))
                    .into_any_element()
            }
        }
    }

    fn result_row(&self, message_guid: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some(message) = self.results.iter().find(|m| m.guid == message_guid).cloned() else {
            return div().into_any_element();
        };
        let Some(store) = bridge::store(cx) else { return div().into_any_element() };
        let chat = {
            let state = store.state();
            let guid = messages_core::conversations::conversation_guid(&state.grouping, &message.chat_guid).to_owned();
            state.chat(&guid).cloned()
        };
        let Some(chat) = chat else { return div().into_any_element() };
        let theme = cx.theme().clone();
        let (chat_guid, target) = (chat.guid.clone(), message.guid.clone());
        v_flex()
            .id(format!("result-{message_guid}"))
            .gap_0p5()
            .px_3()
            .py_2()
            .on_tap(move |_, window, cx| {
                Self::open(&chat_guid, window, cx);
                crate::thread::request_jump(&chat_guid, &target, cx);
            })
            .child(
                h_flex()
                    .gap_1()
                    .child(text(chat_title(&chat)).font_weight(FontWeight::SEMIBOLD).flex_1().min_w_0())
                    .child(
                        text(format_list_date(message.date, ui::now_ms()))
                            .text_size(theme.type_scale.caption)
                            .text_color(theme.text.secondary),
                    ),
            )
            .child(
                label(ui::clamp_preview(&preview_text(Some(&message), &chat), PREVIEW_CHARS))
                    .lines(2)
                    .text_size(theme.type_scale.caption)
                    .text_color(theme.text.secondary),
            )
            .into_any_element()
    }

    fn pinned_strip(&self, pinned: &[Arc<Chat>], cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let store = bridge::store(cx);
        let cells = pinned.iter().map(|chat| {
            let unread = store.as_ref().is_some_and(|s| conversation_unread(&s.state(), &chat.guid));
            let selected = self.selected.as_deref() == Some(chat.guid.as_str());
            let title = chat_title(chat);
            let name = if chat.is_group { title } else { first_name(&title).to_owned() };
            let guid = chat.guid.clone();
            let menu_chat = chat.clone();
            v_flex()
                .id(format!("pinned-{}", chat.guid))
                .w(px(PINNED_CELL))
                .items_center()
                .gap_1()
                .py_2()
                .on_tap(move |_, window, cx| Self::open(&guid, window, cx))
                .context_menu(move |menu, _, _| chat_menu(menu, &menu_chat, true))
                .child(
                    div()
                        .size(px(PINNED_AVATAR + 8.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .border_2()
                        .border_color(if selected { theme.accent } else { theme.surface.sidebar })
                        .child(ui::chat_avatar(chat, PINNED_AVATAR))
                        .when(unread, |cell| {
                            cell.child(
                                ui::dot(theme.accent, 14.)
                                    .absolute()
                                    .top(px(2.))
                                    .right(px(2.))
                                    .border_2()
                                    .border_color(theme.surface.sidebar),
                            )
                        }),
                )
                .child(text(name).text_size(theme.type_scale.caption).text_color(if selected {
                    theme.text.primary
                } else {
                    theme.text.secondary
                }))
        });
        div().id("pinned").flex().flex_row().flex_wrap().px_2().pb_2().children(cells).into_any_element()
    }

    fn footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let (status, error, host, pending) = match bridge::store(cx) {
            Some(store) => {
                let state = store.state();
                let host = match &state.server {
                    Some(_) if store.transport().kind() == TransportKind::Demo => "Demo data".to_owned(),
                    Some(server) => {
                        format!("macOS {}", server.macos_version.clone().unwrap_or_default()).trim().to_owned()
                    }
                    None => String::new(),
                };
                (state.status, state.connection_error.clone(), host, store.pending_sends())
            }
            None => (ConnectionStatus::Connecting, None, String::new(), 0),
        };
        let queued = if pending > 0 {
            format!(
                " \u{b7} {}",
                messages_core::format::pluralize(pending, "message waiting", Some("messages waiting"))
            )
        } else {
            String::new()
        };
        let (color, text_label) = match status {
            ConnectionStatus::Online => match error {
                Some(error) => (theme.warning, format!("{error}{queued}")),
                None => (theme.success, format!("{host}{queued}")),
            },
            ConnectionStatus::Connecting => (theme.warning, format!("Connecting\u{2026}{queued}")),
            ConnectionStatus::Offline => (theme.danger, format!("Offline, retrying\u{2026}{queued}")),
        };
        h_flex()
            .id("sidebar-footer")
            .gap_2()
            .h(px(38.))
            .pl_3()
            .pr_2()
            .flex_none()
            .child(ui::dot(color, 8.))
            .child(
                text(text_label)
                    .id("connection-status")
                    .text_size(theme.type_scale.caption)
                    .text_color(theme.text.secondary)
                    .flex_1()
                    .min_w_0(),
            )
            .child(
                icon_button("settings", Icon::Settings)
                    .tooltip(format!("Settings ({})", ui::shortcut(",", false)))
                    .on_click(|_, window, cx| crate::ui::dispatch(window, cx, OpenSettings)),
            )
            .into_any_element()
    }
}

fn has_filter(parsed: &messages_core::search::ParsedSearchQuery) -> bool {
    parsed.from_me
        || !parsed.senders.is_empty()
        || parsed.attachments.is_some()
        || parsed.links
        || parsed.before.is_some()
        || parsed.after.is_some()
        || !parsed.chat_names.is_empty()
}

/// One conversation in the list: unread dot, avatar, title with the muted bell and Focus moon,
/// date, and two lines of preview (or "Typing…" in the accent).
fn chat_row(guid: &str, selected: bool, cx: &mut App) -> AnyElement {
    let Some(store) = bridge::store(cx) else { return div().into_any_element() };
    let (chat, unread, typing, silenced) = {
        let state = store.state();
        let Some(chat) = state.chat(guid).cloned() else { return div().into_any_element() };
        (
            chat,
            conversation_unread(&state, guid),
            conversation_typing(&state, guid),
            conversation_focus(&state, guid) == FocusStatus::Silenced,
        )
    };
    let theme = cx.theme().clone();
    let preview = if typing {
        "Typing\u{2026}".to_owned()
    } else {
        ui::clamp_preview(&preview_text(chat.last_message.as_ref(), &chat), PREVIEW_CHARS)
    };
    let date = if chat.last_activity > 0 { format_list_date(chat.last_activity, ui::now_ms()) } else { String::new() };
    let menu_chat = chat.clone();
    // The table's selection highlight sits under a selected row; secondary text loses its
    // contrast there, so the row's own lines take the primary colour.
    let secondary = if selected { theme.text.primary } else { theme.text.secondary };
    h_flex()
        .id(format!("chat-{}", chat.identifier))
        .gap(px(10.))
        .h(px(ROW_HEIGHT))
        .pr_3()
        .context_menu(move |menu, _, _| chat_menu(menu, &menu_chat, false))
        .child(
            div()
                .w(px(20.))
                .flex()
                .justify_center()
                .flex_none()
                .when(unread, |col| col.child(ui::dot(theme.accent, 8.).id(format!("unread-{}", chat.identifier)))),
        )
        .child(ui::chat_avatar(&chat, AVATAR_ROW))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .justify_center()
                .gap_0p5()
                .relative()
                // The hairline under each row, from the text's edge, as Messages draws them.
                .when(!selected, |col| col.child(separator().absolute().bottom_0().left_0().right_0()))
                .child(
                    h_flex()
                        .gap_1()
                        // One line, cut with an ellipsis, as Messages' names are.
                        .child(label(chat_title(&chat)).font_weight(FontWeight::BOLD).flex_1().min_w_0())
                        .when(silenced, |row| {
                            row.child(icon(Icon::Moon, 11.).text_color(secondary).label("Focus on"))
                        })
                        .when(chat.muted, |row| {
                            row.child(icon(Icon::BellOff, 11.).text_color(secondary).label("Alerts hidden"))
                        })
                        .child(label(date).text_size(theme.type_scale.footnote).text_color(secondary).flex_none()),
                )
                .child(label(preview).lines(2).text_size(theme.type_scale.footnote).text_color(if typing && !selected {
                    theme.accent
                } else {
                    secondary
                })),
        )
        .into_any_element()
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = bridge::store(cx);
        let query = self.query.trim().to_owned();
        let has_query = !query.is_empty();
        let parsed = parse_search_query(&query);
        let free_text = parsed.text.trim().to_lowercase();

        let (people, status, empty, selected, contacts) = match &store {
            Some(store) => {
                let state = store.state();
                (
                    conversation_chats(&state),
                    state.status,
                    state.chats.is_empty(),
                    state.selected_chat.clone(),
                    Arc::as_ptr(&state.contacts) as usize,
                )
            }
            None => (Vec::new(), ConnectionStatus::Connecting, true, None, 0),
        };
        for chat in &people {
            {
                let weak = cx.weak_entity();
                Bridge::watch(cx, Topic::Chat(chat.guid.clone()), &weak);
            }
            {
                let weak = cx.weak_entity();
                Bridge::watch(cx, Topic::Typing(chat.guid.clone()), &weak);
            }
        }
        let visible: Vec<Arc<Chat>> = if free_text.is_empty() {
            people
        } else {
            people
                .into_iter()
                .filter(|chat| {
                    let mut haystack = format!("{} {}", chat_title(chat), chat.identifier);
                    for participant in &chat.participants {
                        haystack.push(' ');
                        haystack.push_str(&participant.address);
                        if let Some(name) = &participant.name {
                            haystack.push(' ');
                            haystack.push_str(name);
                        }
                    }
                    haystack.to_lowercase().contains(&free_text)
                })
                .collect()
        };
        let pinned: Vec<Arc<Chat>> =
            if has_query { Vec::new() } else { visible.iter().filter(|chat| chat.pinned).cloned().collect() };
        let rest: Vec<Arc<Chat>> =
            if has_query { visible } else { visible.into_iter().filter(|chat| !chat.pinned).collect() };

        let mut rows: Vec<(Item, u64)> = Vec::with_capacity(rest.len() + 4);
        if let Some(store) = &store {
            let state = store.state();
            for chat in &rest {
                let version = row_version(
                    chat,
                    conversation_typing(&state, &chat.guid),
                    conversation_unread(&state, &chat.guid),
                    conversation_focus(&state, &chat.guid) == FocusStatus::Silenced,
                    contacts,
                    selected.as_deref() == Some(chat.guid.as_str()),
                );
                rows.push((Item::Chat(chat.guid.clone()), version));
            }
        }
        if has_query && !self.results.is_empty() {
            rows.push((Item::ResultsHeader, 0));
            let mut seen = HashMap::new();
            for (i, message) in self.results.iter().enumerate() {
                if seen.insert(message.guid.clone(), i).is_none() {
                    rows.push((Item::Result(message.guid.clone()), self.search_epoch));
                }
            }
        }
        self.note = if has_query && rest.is_empty() && self.results.is_empty() {
            Some(match &self.search_error {
                Some(error) => (error.clone().into(), None),
                None => (
                    format!("No Results for \u{201c}{query}\u{201d}").into(),
                    Some("Try a name, number or a word from a message.".into()),
                ),
            })
        } else if !has_query && empty && status == ConnectionStatus::Online {
            Some(("No Conversations Yet".into(), Some("Start one with the compose button.".into())))
        } else {
            None
        };
        if let Some((title, _)) = &self.note {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            title.hash(&mut hasher);
            rows.push((Item::Note, hasher.finish()));
        }
        self.list.set_items_versioned(rows, cx);
        if self.selected != selected {
            self.selected = selected.clone();
            match selected {
                Some(guid) => self.list.set_selected(vec![Item::Chat(guid)], cx),
                None => self.list.set_selected(Vec::new(), cx),
            }
        }

        let theme = cx.theme().clone();
        let show_tips = has_query
            && !has_filter(&parsed)
            && rest.is_empty()
            && pinned.is_empty()
            && self.results.is_empty()
            && query.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
        let tips = show_tips.then(|| {
            v_flex()
                .gap_0p5()
                .px_3()
                .pb_2()
                .child(label("Search Tips").text_style(TextStyle::Caption1).secondary())
                .children(
                    OPERATOR_TIPS
                        .iter()
                        .map(|tip| text(*tip).text_size(theme.type_scale.caption).text_color(theme.text.secondary)),
                )
        });
        v_flex()
            .id("sidebar")
            .size_full()
            .context_menu(|menu, _, _| menu.item("New Message", NewChat))
            .child(
                div().px_2().pb_2().flex_none().child(
                    text_field("sidebar-search")
                        .search()
                        .text(self.query.clone())
                        .placeholder("Search")
                        .label("Search")
                        .on_change(cx.listener(|this, e: &TextChanged, _, cx| this.set_query(e.text.clone(), cx)))
                        .on_submit(cx.listener(|this, _: &TextSubmitted, window, cx| {
                            let first = this.list.items().into_iter().find_map(|item| match item {
                                Item::Chat(guid) => Some(guid),
                                _ => None,
                            });
                            if let Some(guid) = first {
                                Self::open(&guid, window, cx);
                            }
                        })),
                ),
            )
            .children(tips)
            .when(!pinned.is_empty(), |col| col.child(self.pinned_strip(&pinned, cx)))
            .child(
                list("chats", &self.list, cx.processor(Self::row))
                    .inset()
                    .estimated_row_height(px(ROW_HEIGHT))
                    .selection(Selection::single())
                    .on_select(cx.listener(|_, e: &SelectEvent<Item>, window, cx| match &e.key {
                        Item::Chat(guid) => Self::open(guid, window, cx),
                        Item::Result(_) | Item::ResultsHeader | Item::Note => {}
                    }))
                    .flex_1()
                    .min_h_0(),
            )
            .child(self.footer(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_multi_line_message_previews_as_one_line() {
        let chat = Chat { guid: "c".into(), ..Chat::default() };
        let message = Message { text: "one\ntwo\n\nthree".into(), ..Message::default() };
        assert_eq!(preview_text(Some(&message), &chat), "one two three");
        assert_eq!(preview_text(None, &chat), "New conversation");
    }
}
