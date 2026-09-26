//! Port of `apps/desktop/src/ui/sidebar.tsx`: search, pinned strip, the
//! conversation list and the connection footer.
//!
//! Each chat row is its own `SidebarRow` entity, created once and cached by
//! guid in `rows`; a render pass here only ever calls `set_state` on the
//! handful of rows whose placement, selection or keyboard cursor actually
//! changed; unread, typing and Focus repaint the row itself through its own
//! `Chat`/`Typing`/`Focus` watches, never through here. `Sidebar` itself only
//! watches `ChatList`, `Selection` and `Connection`, so a message arriving in
//! some other chat never touches this view.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::conversations::conversation_chats;
use messages_core::search::{SearchContext, parse_search_query, resolve_search_query};
use messages_core::transport::{ConnectionStatus, TransportKind};
use messages_core::{Chat, Message, chat_title};

use crate::app::{AppRoot, root};
use crate::menus::{MenuItem, MenuRequest, shortcut};
use crate::bridge::{Bridge, StoreHandle, Topic};
use crate::icons::IconName;
use crate::primitives::IconButton;
use crate::search_results::{search_result_row, section_header};
use crate::sidebar_row::{ROW_INSET, SidebarRow};
use crate::theme::{SIDEBAR_WIDTH, SIDEBAR_WIDTH_COMPACT, TITLEBAR_HEIGHT, Theme, radius, spacing, traffic_light_clearance, type_scale};

fn current_store(cx: &App) -> Option<messages_core::MessagesStore> {
    cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone())
}

/// One row the sidebar's body shows, in paint order (sidebar.tsx's `SidebarItem`).
enum Row {
    Chat(Arc<Chat>),
    ResultsHeader,
    Result(Message, Arc<Chat>),
    Note { title: SharedString, body: SharedString },
}

const OPERATOR_TIPS: [&str; 4] = ["from:name or from:me", "has:photo, has:video, has:file or has:link", "before:2024-01-01, after:2024-01-01", "in:chat name"];

pub struct Sidebar {
    app: WeakEntity<AppRoot>,
    search_state: Entity<InputState>,
    results: Vec<Message>,
    search_epoch: u64,
    cursor: Option<String>,
    selected: Option<String>,
    pinned_set: HashSet<String>,
    /// Pinned then unpinned guids, the keyboard cursor's travel order.
    order: Vec<String>,
    rows: HashMap<String, Entity<SidebarRow>>,
    list_scroll: UniformListScrollHandle,
    /// Precomputed by the last render; `cx.processor` reads it back to build
    /// only the rows in the visible range.
    items: Vec<Row>,
    on_select: Rc<dyn Fn(&str, &mut Window, &mut App)>,
    on_arrow: Rc<dyn Fn(&str, i32, &mut Window, &mut App)>,
    /// Set by the list area's capture pass for a right-click: the menu that
    /// was up before any row saw the click.
    menu_before_click: Option<Option<EntityId>>,
    _subscriptions: Vec<Subscription>,
}

impl Sidebar {
    pub fn new(window: &mut Window, app: WeakEntity<AppRoot>, cx: &mut Context<Self>) -> Self {
        let search_state = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let weak = cx.entity().downgrade();

        let on_select: Rc<dyn Fn(&str, &mut Window, &mut App)> = {
            let weak = weak.clone();
            Rc::new(move |guid: &str, window: &mut Window, cx: &mut App| {
                let guid = guid.to_owned();
                let _ = weak.update(cx, |this, cx| this.select(&guid, window, cx));
            })
        };
        let on_arrow: Rc<dyn Fn(&str, i32, &mut Window, &mut App)> = {
            let weak = weak.clone();
            Rc::new(move |guid: &str, delta: i32, window: &mut Window, cx: &mut App| {
                let guid = guid.to_owned();
                let _ = weak.update(cx, |this, cx| this.on_arrow(&guid, delta, window, cx));
            })
        };

        let sub = cx.subscribe_in(&search_state, window, |this: &mut Self, _state, event: &InputEvent, window, cx| match event {
            InputEvent::Change => this.on_query_changed(window, cx),
            InputEvent::PressEnter { .. } => this.submit_search(window, cx),
            _ => {}
        });

        Bridge::watch(cx, Topic::ChatList, weak.clone().into());
        Bridge::watch(cx, Topic::Selection, weak.clone().into());
        Bridge::watch(cx, Topic::Connection, weak.into());

        Self {
            app,
            search_state,
            results: Vec::new(),
            search_epoch: 0,
            cursor: None,
            selected: None,
            pinned_set: HashSet::new(),
            order: Vec::new(),
            rows: HashMap::new(),
            list_scroll: UniformListScrollHandle::new(),
            items: Vec::new(),
            on_select,
            on_arrow,
            menu_before_click: None,
            _subscriptions: vec![sub],
        }
    }

    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.search_state.focus_handle(cx), cx);
    }

    fn query(&self, cx: &App) -> String {
        self.search_state.read(cx).value().trim().to_string()
    }

    fn on_query_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
        self.schedule_search(window, cx);
    }

    fn submit_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.cursor.clone().or_else(|| self.order.first().cloned());
        if let Some(guid) = target {
            self.select(&guid, window, cx);
        }
    }

    fn schedule_search(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.search_epoch += 1;
        let epoch = self.search_epoch;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(250)).await;
            let _ = this.update(cx, |this, cx| {
                if this.search_epoch == epoch {
                    this.run_search(epoch, cx);
                }
            });
        })
        .detach();
    }

    fn run_search(&mut self, epoch: u64, cx: &mut Context<Self>) {
        let Some(store) = current_store(cx) else {
            return;
        };
        let query = self.query(cx);
        let parsed = parse_search_query(&query);
        let free_text = parsed.text.trim().to_lowercase();
        let has_filter =
            parsed.from_me || !parsed.senders.is_empty() || parsed.attachments.is_some() || parsed.links || parsed.before.is_some() || parsed.after.is_some() || !parsed.chat_names.is_empty();
        if free_text.len() < 2 && !has_filter {
            if !self.results.is_empty() {
                self.results.clear();
                cx.notify();
            }
            return;
        }
        let (query_text, mut filters) = {
            let state = store.state();
            resolve_search_query(&parsed, &SearchContext { contacts: &state.contacts, chats: &state.chats })
        };
        filters.limit = Some(20);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let transport_store = store.clone();
        store.spawn(async move {
            let result = transport_store.transport().search_messages(&query_text, &filters).await;
            let _ = tx.send(result);
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(messages)) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    if this.search_epoch == epoch {
                        this.results = messages;
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// Gets or creates the row entity for `guid`, and pushes its current
    /// placement/selection/cursor into it. Cheap when the row already exists:
    /// `set_state` only notifies when something actually changed.
    fn row_for(&mut self, guid: &str, pinned_cell: bool, window: &mut Window, cx: &mut Context<Self>) -> Entity<SidebarRow> {
        let selected = self.selected.as_deref() == Some(guid);
        let cursored = self.cursor.as_deref() == Some(guid);
        let app = self.app.clone();
        let on_select = self.on_select.clone();
        let on_arrow = self.on_arrow.clone();
        let entity = self.rows.entry(guid.to_owned()).or_insert_with(|| {
            cx.new(|cx| SidebarRow::new(guid.to_owned(), app, on_select, on_arrow, window, cx))
        }).clone();
        entity.update(cx, |row, cx| row.set_state(pinned_cell, selected, cursored, cx));
        entity
    }

    fn select(&mut self, guid: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.cursor = Some(guid.to_owned());
        if let Some(row) = self.rows.get(guid).cloned() {
            window.focus(&row.read(cx).focus_handle(), cx);
        }
        let Some(store) = current_store(cx) else { return };
        let guid = guid.to_owned();
        let for_task = store.clone();
        store.spawn(async move { for_task.select_chat(Some(&guid)).await });
    }

    /// Moves the keyboard cursor to `next_guid`: un-cursors the row that had
    /// it, cursors and focuses the new one, and scrolls it into view when it
    /// lives in the virtualized list rather than the pinned strip.
    fn move_cursor_to(&mut self, next_guid: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.cursor.as_deref() == Some(next_guid.as_str()) {
            return;
        }
        let previous = self.cursor.replace(next_guid.clone());
        if let Some(previous) = previous {
            if let Some(row) = self.rows.get(&previous).cloned() {
                let pinned = self.pinned_set.contains(&previous);
                let selected = self.selected.as_deref() == Some(previous.as_str());
                row.update(cx, |row, cx| row.set_state(pinned, selected, false, cx));
            }
        }
        if let Some(row) = self.rows.get(&next_guid).cloned() {
            let pinned = self.pinned_set.contains(&next_guid);
            let selected = self.selected.as_deref() == Some(next_guid.as_str());
            row.update(cx, |row, cx| row.set_state(pinned, selected, true, cx));
            window.focus(&row.read(cx).focus_handle(), cx);
        }
        if !self.pinned_set.contains(&next_guid) {
            if let Some(rest_index) = self.items.iter().filter(|item| matches!(item, Row::Chat(_))).position(|item| matches!(item, Row::Chat(chat) if chat.guid == next_guid)) {
                self.list_scroll.scroll_to_item(rest_index, ScrollStrategy::Nearest);
            }
        }
    }

    fn on_arrow(&mut self, guid: &str, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.order.iter().position(|item| item == guid) else { return };
        let len = self.order.len() as i32;
        let next_index = (index as i32 + delta).clamp(0, len - 1) as usize;
        let Some(next_guid) = self.order.get(next_index).cloned() else { return };
        self.move_cursor_to(next_guid, window, cx);
    }

    fn focus_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(first) = self.order.first().cloned() {
            self.move_cursor_to(first, window, cx);
        }
    }
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::trace::render("Sidebar");
        let palette = Theme::get(cx);
        let store = current_store(cx);
        let query = self.query(cx);
        let has_query = !query.is_empty();
        let parsed = parse_search_query(&query);
        let free_text = parsed.text.trim().to_lowercase();
        let has_filter =
            parsed.from_me || !parsed.senders.is_empty() || parsed.attachments.is_some() || parsed.links || parsed.before.is_some() || parsed.after.is_some() || !parsed.chat_names.is_empty();

        let (people, host, status, pending, selected_chat, chats_empty) = match &store {
            Some(store) => {
                let state = store.state();
                let people = conversation_chats(&state);
                let host = if let Some(server) = &state.server {
                    if store.transport().kind() == TransportKind::Demo {
                        "Demo data".to_owned()
                    } else {
                        format!("macOS {}", server.macos_version.clone().unwrap_or_default()).trim().to_owned()
                    }
                } else {
                    String::new()
                };
                let empty = state.chats.is_empty();
                (people, host, state.status, store.pending_sends(), state.selected_chat.clone(), empty)
            }
            None => (Vec::new(), String::new(), ConnectionStatus::Connecting, 0, None, true),
        };
        self.selected = selected_chat;

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
        let pinned: Vec<Arc<Chat>> = if has_query { Vec::new() } else { visible.iter().filter(|chat| chat.pinned).cloned().collect() };
        let rest: Vec<Arc<Chat>> = if has_query { visible.clone() } else { visible.into_iter().filter(|chat| !chat.pinned).collect() };
        self.pinned_set = pinned.iter().map(|chat| chat.guid.clone()).collect();
        self.order = pinned.iter().chain(rest.iter()).map(|chat| chat.guid.clone()).collect();
        // Rows nobody placed this pass are gone from the sidebar (deleted,
        // filtered out); dropping them stops the topic watches for good.
        self.rows.retain(|guid, _| self.order.contains(guid));

        let show_tips = has_query && !has_filter && rest.is_empty() && pinned.is_empty() && self.results.is_empty() && query.chars().next().is_some_and(|c| c.is_ascii_alphabetic());

        let mut items: Vec<Row> = rest.iter().cloned().map(Row::Chat).collect();
        if has_query && !self.results.is_empty() {
            let chat_by_guid: HashMap<String, Arc<Chat>> = store
                .as_ref()
                .map(|store| {
                    let state = store.state();
                    state.chats.iter().map(|chat| (chat.guid.clone(), chat.clone())).collect()
                })
                .unwrap_or_default();
            items.push(Row::ResultsHeader);
            for message in &self.results {
                let guid = store.as_ref().map(|store| messages_core::conversations::conversation_guid(&store.state().grouping, &message.chat_guid).to_owned()).unwrap_or_else(|| message.chat_guid.clone());
                if let Some(chat) = chat_by_guid.get(&guid) {
                    items.push(Row::Result(message.clone(), chat.clone()));
                }
            }
        }
        if has_query && rest.is_empty() && pinned.is_empty() && self.results.is_empty() {
            items.push(Row::Note { title: format!("No results for \u{201c}{query}\u{201d}").into(), body: "Try a name, number or a word from a message.".into() });
        }
        if !has_query && chats_empty && matches!(status, ConnectionStatus::Online) {
            items.push(Row::Note { title: "No conversations yet".into(), body: "Start one with the compose button.".into() });
        }
        self.items = items;

        let close_search = {
            let search_state = self.search_state.clone();
            move |window: &mut Window, cx: &mut App| {
                search_state.update(cx, |state, cx| state.set_value("", window, cx));
            }
        };

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(spacing::X2)
            .h(TITLEBAR_HEIGHT)
            .pl(spacing::X3 + traffic_light_clearance())
            .pr(spacing::X2)
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(spacing::X1)
                    .flex_grow(1.)
                    .min_w(px(0.))
                    .h(px(28.))
                    .px(spacing::X2)
                    .rounded(radius::CONTROL)
                    .occlude()
                    .bg(palette.canvas)
                    .border_1()
                    .border_color(palette.separator)
                    .on_action(cx.listener({
                        let close_search = close_search.clone();
                        move |this, _: &crate::app::Dismiss, window, cx| {
                            if this.query(cx).is_empty() {
                                cx.propagate();
                            } else {
                                close_search(window, cx);
                            }
                        }
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key == "down" {
                            this.focus_first(window, cx);
                        }
                    }))
                    .child(crate::icons::Icon::new(IconName::Search).size(px(13.)).color(palette.tertiary))
                    .child(Input::new(&self.search_state).bordered(false))
                    .when(has_query, |el| el.child(IconButton::new("clear-search", IconName::Close, "Clear search").size(px(12.)).hit(px(20.)).on_click(move |_, window, cx| close_search(window, cx)))),
            )
            .child(IconButton::new("new-message", IconName::Compose, "New message").size(px(17.)).on_click(|_, window, cx| {
                window.dispatch_action(Box::new(crate::app::NewChat), cx);
            }));

        let tips = show_tips.then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .pl(ROW_INSET)
                .pr(ROW_INSET)
                .pt(spacing::X1)
                .pb(spacing::X2)
                .flex_shrink_0()
                .child(div().text_size(type_scale::MICRO.font_size).line_height(type_scale::MICRO.line_height).font_weight(FontWeight::SEMIBOLD).text_color(palette.tertiary).child("Search tips"))
                .children(OPERATOR_TIPS.iter().map(|tip| div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).child(*tip)))
        });

        let pinned_strip = (!pinned.is_empty()).then(|| {
            let cells = pinned
                .iter()
                .map(|chat| {
                    let guid = chat.guid.clone();
                    div().debug_selector(move || format!("pinned-{guid}")).child(self.row_for(&chat.guid, true, window, cx))
                })
                .collect::<Vec<_>>();
            div().flex().flex_col().px(spacing::X2).flex_shrink_0().child(div().flex().flex_row().flex_wrap().pt(spacing::X1).pb(spacing::X2).children(cells)).when(!rest.is_empty() && !has_query, |el| {
                el.child(div().h(px(1.)).bg(palette.sidebar_border).mb(spacing::X2).mx(ROW_INSET).flex_shrink_0())
            })
        });

        let body: AnyElement = if has_query {
            let mut column = div().id("sidebar-search-body").flex().flex_col().flex_grow(1.).min_h(px(0.)).overflow_y_scroll().pb(spacing::X2);
            for index in 0..self.items.len() {
                column = column.child(render_row(self, index, window, cx));
            }
            column.into_any_element()
        } else {
            let count = self.items.len();
            let list = uniform_list("sidebar-list", count, cx.processor(|this, range: std::ops::Range<usize>, window, cx| {
                range.map(|index| render_row(this, index, window, cx)).collect::<Vec<_>>()
            }))
            .track_scroll(&self.list_scroll)
            .flex_grow(1.)
            .min_h(px(0.))
            .w_full()
            .pb(spacing::X2);
            // Right-click on the list's empty space offers "New message"
            // (sidebar.tsx:436-443). A row opens its own menu on the same
            // mouse-up, so the capture pass notes which menu was up and the
            // bubble pass only acts when no row replaced it.
            div()
                .id("sidebar-list-area")
                .debug_selector(|| "sidebar-list-area".into())
                .flex()
                .flex_col()
                .flex_grow(1.)
                .min_h(px(0.))
                .w_full()
                .capture_any_mouse_up(cx.listener(|this, event: &MouseUpEvent, _, cx| {
                    if event.button == MouseButton::Right {
                        this.menu_before_click = Some(root(cx).and_then(|app| app.read(cx).menu_id()));
                    }
                }))
                .on_mouse_up(MouseButton::Right, cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    let Some(before) = this.menu_before_click.take() else { return };
                    let Some(app) = root(cx) else { return };
                    if app.read(cx).menu_id() != before {
                        return;
                    }
                    let item = MenuItem::item("New message", |window, cx| window.dispatch_action(Box::new(crate::app::NewChat), cx))
                        .icon(IconName::Compose)
                        .shortcut(shortcut("N", false, false));
                    AppRoot::open_menu(&app, MenuRequest::at(event.position, vec![item]), window, cx);
                }))
                .child(list)
                .into_any_element()
        };

        let (status_color, status_label) = {
            let queued = if pending > 0 { format!(" \u{b7} {}", messages_core::format::pluralize(pending, "message waiting", Some("messages waiting"))) } else { String::new() };
            match status {
                ConnectionStatus::Online => (palette.online, format!("{host}{queued}")),
                ConnectionStatus::Connecting => (palette.warning, format!("Connecting\u{2026}{queued}")),
                ConnectionStatus::Offline => (palette.offline, format!("Offline, retrying\u{2026}{queued}")),
            }
        };

        let footer = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(spacing::X1)
            .h(px(38.))
            .pl(spacing::X3)
            .pr(spacing::X2)
            .flex_shrink_0()
            .border_t_1()
            .border_color(palette.sidebar_border)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(spacing::X2)
                    .min_w(px(0.))
                    .flex_grow(1.)
                    .child(div().w(px(8.)).h(px(8.)).rounded(px(4.)).flex_shrink_0().bg(status_color))
                    .child(div().text_size(type_scale::MICRO.font_size).line_height(type_scale::MICRO.line_height).text_color(palette.secondary).text_ellipsis().min_w(px(0.)).child(status_label)),
            )
            .child(IconButton::new("settings", IconName::Settings, format!("Server settings ({})", crate::sidebar_row::shortcut(",", false))).on_click(|_, window, cx| {
                window.dispatch_action(Box::new(crate::app::OpenSettings), cx);
            }));

        div()
            .id("sidebar")
            .flex()
            .flex_col()
            .w(if window.viewport_size().width < px(900.) { SIDEBAR_WIDTH_COMPACT } else { SIDEBAR_WIDTH })
            .h_full()
            .flex_shrink_0()
            .bg(palette.sidebar)
            .child(header)
            .when_some(tips, |el, tips| el.child(tips))
            .when_some(pinned_strip, |el, strip| el.child(strip))
            .child(body)
            .child(footer)
    }
}

fn render_row(sidebar: &mut Sidebar, index: usize, window: &mut Window, cx: &mut Context<Sidebar>) -> AnyElement {
    let Some(item) = sidebar.items.get(index) else { return div().into_any_element() };
    match item {
        Row::Chat(chat) => {
            let guid = chat.guid.clone();
            let selector = format!("chat-{guid}");
            div().px(spacing::X2).debug_selector(move || selector).child(sidebar.row_for(&guid, false, window, cx)).into_any_element()
        }
        Row::ResultsHeader => {
            let palette = Theme::get(cx);
            div().px(spacing::X2).child(section_header(palette)).into_any_element()
        }
        Row::Result(message, chat) => {
            let palette = Theme::get(cx);
            let chat_guid = chat.guid.clone();
            let message_guid = message.guid.clone();
            let on_select = sidebar.on_select.clone();
            let id = ElementId::Name(format!("result-{}", message.guid).into());
            let row = search_result_row(id, message, chat, palette, move |window, cx| {
                on_select(&chat_guid, window, cx);
                crate::thread::request_jump(&chat_guid, &message_guid, cx);
            });
            div().px(spacing::X2).child(row).into_any_element()
        }
        Row::Note { title, body } => {
            let palette = Theme::get(cx);
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(spacing::X1)
                .pt(spacing::X10)
                .px(spacing::X4)
                .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).font_weight(FontWeight::SEMIBOLD).text_color(palette.text).text_align(TextAlign::Center).child(title.clone()))
                .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).text_align(TextAlign::Center).child(body.clone()))
                .into_any_element()
        }
    }
}
