//! Port of the conversation header in `apps/desktop/src/ui/header.tsx`.
//! `app.rs` mounts `ConversationHeader` above `Thread`.

use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::assistant::{CanaryLlmClient, MAX_SUMMARY_MESSAGES, SummarizeMessage};
use messages_core::conversations::{conversation_focus, conversation_guid, conversation_handles, conversation_messages};
use messages_core::findmy::{FriendLocation, contact_addresses, match_friend};
use messages_core::format::format_address;
use messages_core::{FocusStatus, Message, Service, chat_title, handle_name};

use crate::app::{AppRoot, ToggleInfo, root};
use crate::bridge::{Bridge, Topic, config, store};
use crate::details::service_label;
use crate::icons::{Icon, IconName};
use crate::menus::{MenuRequest, shortcut};
use crate::motion::{self, DURATION_FAST, Presence};
use crate::primitives::{IconButton, avatar, overlay_shadows};
use crate::theme::{TITLEBAR_HEIGHT, Theme, radius, spacing, type_scale};

/// Caps how far back "Catch me up" looks when I have not sent anything in this thread.
const CATCH_UP_FALLBACK: usize = 50;

/// Everything since my last message in the thread, oldest first, or the last
/// 50 if there is nothing after it (header.tsx:34-42).
fn messages_to_catch_up_on(messages: &[Arc<Message>]) -> Vec<Arc<Message>> {
    let visible: Vec<Arc<Message>> = messages.iter().filter(|message| message.date_retracted.is_none()).cloned().collect();
    let since = match visible.iter().rposition(|message| message.from_me) {
        Some(index) => visible[index + 1..].to_vec(),
        None => Vec::new(),
    };
    if !since.is_empty() { since } else { visible[visible.len().saturating_sub(CATCH_UP_FALLBACK)..].to_vec() }
}

/// Only text, sender name and time leave the machine; attachment bytes never do.
fn to_summarize(messages: &[Arc<Message>]) -> Vec<SummarizeMessage> {
    messages
        .iter()
        .take(MAX_SUMMARY_MESSAGES)
        .map(|message| SummarizeMessage {
            sender: if message.from_me { "Me".to_owned() } else { message.sender.as_ref().map(|sender| handle_name(sender).to_owned()).unwrap_or_else(|| "Someone".to_owned()) },
            text: if !message.text.trim().is_empty() {
                message.text.trim().to_owned()
            } else if !message.attachments.is_empty() {
                "[attachment]".to_owned()
            } else {
                String::new()
            },
            date: message.date,
            from_me: message.from_me,
        })
        .collect()
}

#[derive(Clone)]
enum Card {
    Loading,
    Error(String),
    Summary(String),
}

pub struct ConversationHeader {
    current_chat: Option<String>,
    card: Option<Card>,
    card_shown: Presence<Card>,
    /// Bumped per request and per chat switch, so a late answer is dropped.
    request: u64,
    info_open: bool,
    /// Where "Catch me up" was clicked; the card hangs its top-right corner there.
    card_x: Pixels,
}

impl ConversationHeader {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade();
        for topic in [Topic::Selection, Topic::Focus, Topic::Locations, Topic::Contacts, Topic::Connection, Topic::ChatList] {
            Bridge::watch(cx, topic, weak.clone().into());
        }
        Self { current_chat: None, card: None, card_shown: Presence::new(DURATION_FAST), request: 0, info_open: false, card_x: px(0.) }
    }

    /// Switching threads makes any answer in flight stale; drop it rather
    /// than show it on the wrong chat.
    fn sync_selection(&mut self, cx: &mut Context<Self>) {
        let Some(store) = store(cx) else { return };
        let primary = {
            let state = store.state();
            state.selected_chat.as_deref().map(|guid| conversation_guid(&state.grouping, guid).to_owned())
        };
        if primary == self.current_chat {
            return;
        }
        if let Some(guid) = &primary {
            let weak = cx.entity().downgrade();
            Bridge::watch(cx, Topic::Chat(guid.clone()), weak.into());
        }
        self.current_chat = primary;
        self.request += 1;
        self.card = None;
    }

    fn catch_me_up(&mut self, x: Pixels, cx: &mut Context<Self>) {
        self.card_x = x;
        if self.card.is_some() {
            self.close_card(cx);
            return;
        }
        let Some(store) = store(cx) else { return };
        let Some(chat) = self.current_chat.clone() else { return };
        let Some(settings) = config(cx).and_then(|config| config.canaryllm.clone()) else { return };
        let payload = to_summarize(&messages_to_catch_up_on(&conversation_messages(&store.state(), &chat)));

        self.request += 1;
        let request = self.request;
        self.card = Some(Card::Loading);
        cx.notify();

        let (tx, rx) = tokio::sync::oneshot::channel();
        store.spawn(async move {
            let client = CanaryLlmClient::new(settings.api_key, settings.model, None, reqwest::Client::new());
            let _ = tx.send(client.summarize(&payload).await.map_err(|error| error.to_string()));
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                if this.request != request {
                    return;
                }
                this.card = Some(match result {
                    Ok(text) => Card::Summary(text),
                    Err(error) => Card::Error(error),
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn close_card(&mut self, cx: &mut Context<Self>) {
        self.request += 1;
        self.card = None;
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn title(&self, cx: &App) -> Option<String> {
        let store = store(cx)?;
        let state = store.state();
        self.current_chat.as_deref().and_then(|guid| state.chat(guid)).map(|chat| chat_title(chat))
    }

    fn open_chat_menu(&self, position: Point<Pixels>, window: &mut Window, cx: &mut App) {
        let (Some(store), Some(app), Some(guid)) = (store(cx), root(cx), self.current_chat.clone()) else { return };
        let Some(chat) = store.state().chat(&guid).cloned() else { return };
        let items = crate::sidebar_row::chat_menu(&chat, &store, app.downgrade(), false);
        AppRoot::open_menu(&app, MenuRequest::at(position, items), window, cx);
    }
}

fn card_view(card: &Card, cx: &mut Context<ConversationHeader>) -> Stateful<Div> {
    let palette = Theme::get(cx);
    let body = match card {
        Card::Loading => div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).child("Summarizing…"),
        Card::Error(error) => div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.danger).child(error.clone()),
        Card::Summary(summary) => div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).text_color(palette.text).child(summary.clone()),
    };
    div()
        .id("catch-up-card")
        .occlude()
        .flex()
        .flex_col()
        .w(px(300.))
        .p(spacing::X3)
        .gap(spacing::X2)
        .rounded(radius::CARD)
        .bg(palette.overlay)
        .border_1()
        .border_color(palette.overlay_border)
        .shadow(overlay_shadows())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(spacing::X2)
                .child(Icon::new(IconName::Sparkles).size(px(14.)).color(palette.accent))
                .child(div().flex_grow(1.).text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).font_weight(FontWeight::SEMIBOLD).text_color(palette.text).child("Catch me up"))
                .child(IconButton::new("close-summary", IconName::Close, "Close").size(px(12.)).hit(px(22.)).on_click(cx.listener(|this, _, _, cx| this.close_card(cx)))),
        )
        .child(body)
}

/// The line under the title (header.tsx:151-158).
fn subtitle(chat: &messages_core::Chat, handles: &[messages_core::Handle], sharing: bool) -> String {
    let base = if chat.is_group {
        chat.participants.iter().map(handle_name).collect::<Vec<_>>().join(", ")
    } else if handles.first().is_some_and(|first| first.name.is_some()) {
        let addresses: Vec<String> = handles.iter().map(|handle| format_address(&handle.address)).collect();
        format!("{} · {}", addresses.join(" · "), service_label(chat.service))
    } else if chat.service == Service::IMessage {
        "iMessage".to_owned()
    } else {
        "Text message".to_owned()
    };
    if sharing { format!("{base} · Sharing location") } else { base }
}

impl Render for ConversationHeader {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::trace::render("Header");
        self.sync_selection(cx);
        self.card_shown.set(self.card.clone(), |this: &mut Self| &mut this.card_shown, cx);
        let palette = Theme::get(cx);
        let blank = || div().w_full().h(TITLEBAR_HEIGHT).flex_shrink_0().border_b_1().border_color(palette.separator).into_any_element();
        let Some(live) = store(cx) else { return blank() };
        let state = live.state();
        let Some(chat) = self.current_chat.as_deref().and_then(|guid| state.chat(guid)).cloned() else {
            drop(state);
            return blank();
        };
        let title = chat_title(&chat);
        let handles = conversation_handles(&state, &chat.guid);
        let sharing = !chat.is_group
            && handles.first().is_some_and(|first| {
                let friends: Vec<FriendLocation> = state.locations.values().cloned().collect();
                let mut addresses = vec![first.address.clone()];
                addresses.extend(contact_addresses(&state.contacts, &first.address));
                match_friend(&friends, &addresses).is_some()
            });
        let silenced = conversation_focus(&state, &chat.guid) == FocusStatus::Silenced;
        let capabilities = state.capabilities;
        drop(state);
        let subtitle = subtitle(&chat, &handles, sharing);
        let assistant = config(cx).is_some_and(|config| config.canaryllm.as_ref().is_some_and(|settings| !settings.api_key.trim().is_empty()));
        self.info_open = root(cx).is_some_and(|app| app.read(cx).info_open());
        let info_label = format!("{} ({})", if self.info_open { "Hide details" } else { "Show details" }, shortcut("I", false, false));

        let card = self.card_shown.current().cloned().map(|card| {
            let open = self.card_shown.is_open();
            let view = motion::fade(card_view(&card, cx), self.card_shown.id("catch-up"), open, DURATION_FAST, DURATION_FAST);
            deferred(anchored().anchor(Anchor::TopRight).position(point(self.card_x, TITLEBAR_HEIGHT)).snap_to_window_with_margin(spacing::X2).child(view))
                .with_priority(3)
        });

        div()
            .w_full()
            .h(TITLEBAR_HEIGHT)
            .flex_shrink_0()
            .flex()
            .flex_row()
            .items_center()
            .gap(spacing::X2)
            .pl(spacing::X3)
            .pr(spacing::X2)
            .border_b_1()
            .border_color(palette.separator)
            .child(
                div()
                    .id("thread-identity")
                    .tab_index(0)
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(spacing::X2)
                    .flex_grow(1.)
                    .flex_basis(px(0.))
                    .min_w(px(0.))
                    .h(px(40.))
                    .pl(spacing::X1)
                    .pr(spacing::X2)
                    .rounded(radius::ROW)
                    .hover(|style| style.bg(palette.hover_wash))
                    .active(|style| style.bg(palette.press_wash))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleInfo), cx))
                    .on_key_down(|event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            window.dispatch_action(Box::new(ToggleInfo), cx);
                        }
                    })
                    .on_mouse_up(MouseButton::Right, cx.listener(|this, event: &MouseUpEvent, window, cx| this.open_chat_menu(event.position, window, cx)))
                    .child(avatar(chat.participants.first(), Some(&chat), px(30.), cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_grow(1.)
                            .min_w(px(0.))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(spacing::X1)
                                    .min_w(px(0.))
                                    .child(
                                        div()
                                            .id("thread-title")
                                            .debug_selector(|| "thread-title".into())
                                            .min_w(px(0.))
                                            .text_size(type_scale::TITLE.font_size)
                                            .line_height(type_scale::TITLE.line_height)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(palette.text)
                                            .text_ellipsis()
                                            .child(title),
                                    )
                                    .when(silenced, |el| el.child(Icon::new(IconName::Silenced).size(px(12.)).color(palette.tertiary))),
                            )
                            .child(div().text_size(type_scale::MICRO.font_size).line_height(type_scale::MICRO.line_height).text_color(palette.secondary).text_ellipsis().child(subtitle)),
                    ),
            )
            .when(capabilities.facetime && !chat.is_group, |el| {
                let chat_guid = chat.guid.clone();
                el.child(IconButton::new("facetime", IconName::Video, "FaceTime").size(px(17.)).on_click(move |_, _, cx| {
                    if let Some(store) = store(cx) {
                        let (task, chat_guid) = (store.clone(), chat_guid.clone());
                        store.spawn(async move { task.start_facetime(&chat_guid).await });
                    }
                }))
            })
            .when(assistant, |el| {
                let loading = matches!(self.card, Some(Card::Loading));
                el.child(div().relative().child(IconButton::new("catch-me-up", IconName::Sparkles, "Catch me up").size(px(17.)).disabled(loading).on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                    let x = event.mouse_position().map(|position| position.x).unwrap_or(window.viewport_size().width);
                    this.catch_me_up(x, cx)
                }))).children(card))
            })
            .child(IconButton::new("info", IconName::Info, info_label).size(px(17.)).active(self.info_open).on_click(|_, window, cx| {
                window.dispatch_action(Box::new(ToggleInfo), cx);
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use messages_core::{Chat, Handle};

    fn handle(address: &str, name: Option<&str>) -> Handle {
        Handle { address: address.to_owned(), service: Service::IMessage, name: name.map(str::to_owned), avatar: None }
    }

    #[::core::prelude::v1::test]
    fn subtitle_follows_header_tsx() {
        let named = Chat { participants: vec![handle("+15551234567", Some("Ana"))], ..Default::default() };
        let handles = vec![handle("+15551234567", Some("Ana")), handle("ana@example.com", Some("Ana"))];
        assert_eq!(subtitle(&named, &handles, false), format!("{} · ana@example.com · iMessage", format_address("+15551234567")));
        let unnamed = Chat { service: Service::Sms, participants: vec![handle("+15551234567", None)], ..Default::default() };
        assert_eq!(subtitle(&unnamed, &unnamed.participants, true), "Text message · Sharing location");
        let group = Chat { is_group: true, participants: vec![handle("a", Some("Ana")), handle("b", Some("Bo"))], ..Default::default() };
        assert_eq!(subtitle(&group, &group.participants, false), "Ana, Bo");
    }

    #[::core::prelude::v1::test]
    fn catch_up_falls_back_to_the_last_fifty() {
        let message = |from_me: bool| Arc::new(Message { from_me, ..Default::default() });
        let mut messages: Vec<Arc<Message>> = (0..60).map(|_| message(false)).collect();
        assert_eq!(messages_to_catch_up_on(&messages).len(), 50);
        messages.push(message(true));
        messages.push(message(false));
        assert_eq!(messages_to_catch_up_on(&messages).len(), 1);
        messages.push(message(true));
        assert_eq!(messages_to_catch_up_on(&messages).len(), 50);
    }
}
