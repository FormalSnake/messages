//! The conversation in the window's toolbar, as Messages shows it (its avatar over its name,
//! the Focus moon, the service and location line as the tooltip; a click opens the details),
//! the toolbar items that act on it (Catch me up, FaceTime), and what sits over the thread: the
//! incoming FaceTime banner and the Catch me up card.

use std::sync::Arc;

use messages_core::assistant::{CanaryLlmClient, MAX_SUMMARY_MESSAGES, SummarizeMessage};
use messages_core::conversations::{
    conversation_focus, conversation_guid, conversation_handles, conversation_messages,
};
use messages_core::findmy::{FriendLocation, contact_addresses, match_friend};
use messages_core::format::format_address;
use messages_core::store::{FaceTimeCall, FaceTimeCallStatus};
use messages_core::{Chat, FocusStatus, Handle, Message, Service, chat_title, handle_name};
use nd::prelude::*;

use crate::actions::*;
use crate::bridge::{self, Bridge, Topic};
use crate::ui;

/// How far back "Catch me up" looks when I have sent nothing in the thread.
const CATCH_UP_FALLBACK: usize = 50;

/// Everything since my last message, oldest first, or the last 50 when nothing follows it.
pub fn messages_to_catch_up_on(messages: &[Arc<Message>]) -> Vec<Arc<Message>> {
    let visible: Vec<Arc<Message>> = messages.iter().filter(|m| m.date_retracted.is_none()).cloned().collect();
    let since = match visible.iter().rposition(|m| m.from_me) {
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
        .map(|m| SummarizeMessage {
            sender: if m.from_me {
                "Me".to_owned()
            } else {
                m.sender.as_ref().map(|s| handle_name(s).to_owned()).unwrap_or_else(|| "Someone".to_owned())
            },
            text: if !m.text.trim().is_empty() {
                m.text.trim().to_owned()
            } else if !m.attachments.is_empty() {
                "[attachment]".to_owned()
            } else {
                String::new()
            },
            date: m.date,
            from_me: m.from_me,
        })
        .collect()
}

pub fn service_label(service: Service) -> &'static str {
    match service {
        Service::IMessage => "iMessage",
        Service::Sms => "SMS",
        Service::Rcs => "RCS",
    }
}

/// The line under the title.
pub fn subtitle(chat: &Chat, handles: &[Handle], sharing: bool) -> String {
    let base = if chat.is_group {
        chat.participants.iter().map(handle_name).collect::<Vec<_>>().join(", ")
    } else if handles.first().is_some_and(|first| first.name.is_some()) {
        let addresses: Vec<String> = handles.iter().map(|h| format_address(&h.address)).collect();
        format!("{} \u{b7} {}", addresses.join(" \u{b7} "), service_label(chat.service))
    } else if chat.service == Service::IMessage {
        "iMessage".to_owned()
    } else {
        "Text message".to_owned()
    };
    if sharing { format!("{base} \u{b7} Sharing location") } else { base }
}

#[derive(Clone)]
enum Card {
    Loading,
    Error(String),
    Summary(String),
}

pub struct Header {
    chat: Option<String>,
    card: Option<Card>,
    /// Bumped per request and per chat switch, so a late answer is dropped.
    request: u64,
}

/// The open conversation: its avatar and its name, the Focus moon after the name; a click on the
/// name toggles the details. `stacked` is Messages' look on macOS 26, the avatar over the name
/// in a Liquid Glass capsule, drawn by the transcript at the top of its half where it runs up
/// under the toolbar. Otherwise it is the toolbar's centre item, the avatar beside the name,
/// since an NSToolbar item cannot grow past the toolbar's row (docs/nd-parity.md).
pub fn identity(shown: bool, stacked: bool, cx: &mut App) -> Option<AnyElement> {
    if !shown {
        return None;
    }
    let store = bridge::store(cx)?;
    let (chat, handles, sharing, silenced) = {
        let state = store.state();
        let chat = state.selected_chat.as_deref().and_then(|g| state.chat(g)).cloned()?;
        let handles = conversation_handles(&state, &chat.guid);
        let sharing = !chat.is_group
            && handles.first().is_some_and(|first| {
                let friends: Vec<FriendLocation> = state.locations.values().cloned().collect();
                let mut addresses = vec![first.address.clone()];
                addresses.extend(contact_addresses(&state.contacts, &first.address));
                match_friend(&friends, &addresses).is_some()
            });
        let silenced = conversation_focus(&state, &chat.guid) == FocusStatus::Silenced;
        (chat, handles, sharing, silenced)
    };
    let theme = cx.theme().clone();
    let menu_chat = chat.clone();
    let name = button("chat-title", chat_title(&chat))
        .bezel(Bezel::Borderless)
        .tooltip(subtitle(&chat, &handles, sharing))
        .on_click(|_, window, cx| crate::ui::dispatch(window, cx, ToggleInfo));
    let moon = silenced.then(|| icon(Icon::Moon, 11.).text_color(theme.text.secondary).label("Focus on").id("silenced"));
    if stacked {
        return Some(
            v_flex()
                .id("thread-identity")
                .items_center()
                .context_menu(move |menu, _, _| crate::sidebar::chat_menu(menu, &menu_chat, false))
                .child(ui::chat_avatar(&chat, IDENTITY_AVATAR))
                .child(
                    glass().mt(px(-1.)).child(
                        h_flex()
                            .h(px(IDENTITY_CAPSULE))
                            .pl(px(4.))
                            .pr(px(8.))
                            .gap(px(2.))
                            .items_center()
                            .on_tap(|_, window, cx| crate::ui::dispatch(window, cx, ToggleInfo))
                            .child(name)
                            .children(moon)
                            .child(icon(Icon::ChevronRight, 9.).text_color(theme.text.secondary)),
                    ),
                )
                .into_any_element(),
        );
    }
    // In a Liquid Glass capsule, as Messages shows the conversation on macOS 26: nd draws glass
    // the size of a control as a capsule.
    Some(
        glass()
            .child(
                h_flex()
                    .id("thread-identity")
                    .on_tap(|_, window, cx| crate::ui::dispatch(window, cx, ToggleInfo))
                    .context_menu(move |menu, _, _| crate::sidebar::chat_menu(menu, &menu_chat, false))
                    .h(px(36.))
                    .pl(px(3.))
                    .pr_2()
                    .gap(px(6.))
                    .items_center()
                    .child(ui::chat_avatar(&chat, 30.))
                    .child(name)
                    .children(moon),
            )
            .into_any_element(),
    )
}

/// Messages' conversation over the transcript: a 40 pt avatar 6 pt from the window's top, the
/// name's capsule 27 pt tall under it.
const IDENTITY_AVATAR: f32 = 40.;
const IDENTITY_CAPSULE: f32 = 27.;
pub const IDENTITY_TOP: f32 = 6.;

/// The toolbar's items for the open conversation: Catch me up and FaceTime. The details open
/// from the conversation's name, as in Messages, or with Cmd-I.
pub fn toolbar_items(shown: bool, cx: &mut App) -> Vec<AnyElement> {
    if !shown {
        return Vec::new();
    }
    let Some(store) = bridge::store(cx) else { return Vec::new() };
    let (chat, capabilities) = {
        let state = store.state();
        (state.selected_chat.as_deref().and_then(|g| state.chat(g)).cloned(), state.capabilities)
    };
    let Some(chat) = chat else { return Vec::new() };
    let assistant =
        bridge::config(cx).is_some_and(|c| c.canaryllm.as_ref().is_some_and(|l| !l.api_key.trim().is_empty()));
    let mut items = Vec::new();
    if assistant {
        items.push(
            button("catch-up", "Catch Me Up")
                .tooltip("Summarize what you missed")
                .on_click(|_, _, cx| {
                    if let Some(header) = cx.try_global::<HeaderHost>().and_then(|h| h.0.upgrade()) {
                        header.update(cx, |header, cx| header.catch_me_up(cx));
                    }
                })
                .into_any_element(),
        );
    }
    if capabilities.facetime && !chat.is_group {
        let guid = chat.guid.clone();
        items.push(
            icon_button("facetime", Icon::Video)
                .tooltip("FaceTime")
                .on_click(move |_, _, cx| {
                    if let Some(store) = bridge::store(cx) {
                        let guid = guid.clone();
                        crate::rt::detach(async move { store.start_facetime(&guid).await });
                    }
                })
                .into_any_element(),
        );
    }
    items
}

struct HeaderHost(WeakEntity<Header>);

impl Header {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let weak = cx.weak_entity();
        for topic in [
            Topic::Selection,
            Topic::Focus,
            Topic::Locations,
            Topic::Contacts,
            Topic::Connection,
            Topic::ChatList,
            Topic::FaceTime,
        ] {
            Bridge::watch(cx, topic, &weak);
        }
        cx.set_global(HeaderHost(weak));
        Self { chat: None, card: None, request: 0 }
    }

    /// Switching threads makes any answer in flight stale.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let Some(store) = bridge::store(cx) else { return };
        let primary = {
            let state = store.state();
            state.selected_chat.as_deref().map(|guid| conversation_guid(&state.grouping, guid).to_owned())
        };
        if primary == self.chat {
            return;
        }
        if let Some(guid) = &primary {
            {
                let weak = cx.weak_entity();
                Bridge::watch(cx, Topic::Chat(guid.clone()), &weak);
            }
        }
        self.chat = primary;
        self.request += 1;
        self.card = None;
    }

    pub fn catch_me_up(&mut self, cx: &mut Context<Self>) {
        if self.card.is_some() {
            self.close_card(cx);
            return;
        }
        let (Some(store), Some(chat)) = (bridge::store(cx), self.chat.clone()) else { return };
        let Some(settings) = bridge::config(cx).and_then(|c| c.canaryllm.clone()) else { return };
        let payload = to_summarize(&messages_to_catch_up_on(&conversation_messages(&store.state(), &chat)));
        self.request += 1;
        let request = self.request;
        self.card = Some(Card::Loading);
        cx.notify();
        crate::rt::spawn_for(
            cx.weak_entity(),
            async move {
                let client = CanaryLlmClient::new(settings.api_key, settings.model, None, reqwest::Client::new());
                client.summarize(&payload).await.map_err(|e| e.to_string())
            },
            move |this, result, cx| {
                if this.request == request {
                    this.card = Some(match result {
                        Ok(text) => Card::Summary(text),
                        Err(error) => Card::Error(error),
                    });
                    cx.notify();
                }
            },
        );
    }

    fn close_card(&mut self, cx: &mut Context<Self>) {
        self.request += 1;
        self.card = None;
        cx.notify();
    }
}

fn status_line(call: &FaceTimeCall) -> String {
    match call.status {
        FaceTimeCallStatus::Incoming if call.can_answer => "Incoming FaceTime".to_owned(),
        FaceTimeCallStatus::Incoming => "Incoming FaceTime, answer on your Mac".to_owned(),
        FaceTimeCallStatus::Answering => "Answering on the Mac and creating a link\u{2026}".to_owned(),
        FaceTimeCallStatus::Ready => "Link ready. The Mac leaves the call about 15 seconds after you join.".to_owned(),
        FaceTimeCallStatus::Ended => "Call ended".to_owned(),
        FaceTimeCallStatus::Failed => call.error.clone().unwrap_or_else(|| "Could not answer".to_owned()),
    }
}

fn facetime_banner(call: &FaceTimeCall, theme: &Theme) -> AnyElement {
    let who = call.from.clone().unwrap_or_else(|| "Unknown caller".to_owned());
    let failed = call.status == FaceTimeCallStatus::Failed;
    let can_answer = call.status == FaceTimeCallStatus::Incoming && call.can_answer;
    let link = (call.status == FaceTimeCallStatus::Ready).then(|| call.link.clone()).flatten();
    h_flex()
        .id("facetime-banner")
        .gap_3()
        .mx_3()
        .mt_2()
        .px_4()
        .py_3()
        .rounded_xl()
        .bg(theme.surface.overlay)
        .child(v_flex().flex_1().min_w_0().child(text(who).font_weight(FontWeight::SEMIBOLD)).child(
            text(status_line(call)).text_size(theme.type_scale.caption).text_color(if failed {
                theme.text.danger
            } else {
                theme.text.secondary
            }),
        ))
        .when(can_answer, |row| {
            row.child(button("facetime-answer", "Answer in Browser").primary().on_click(|_, _, cx| {
                if let Some(store) = bridge::store(cx) {
                    crate::rt::detach(async move { store.answer_facetime().await });
                }
            }))
        })
        .when_some(link, |row, link| {
            row.child(button("facetime-join", "Join").primary().on_click(move |_, _, _| {
                let _ = nd::system::open_url(&link);
            }))
        })
        .when(can_answer, |row| {
            row.child(button("facetime-decline", "Decline").destructive().on_click(|_, _, cx| {
                if let Some(store) = bridge::store(cx) {
                    crate::rt::detach(async move { store.decline_facetime().await });
                }
            }))
        })
        .child(icon_button("facetime-dismiss", Icon::X).tooltip("Dismiss").on_click(|_, _, cx| {
            if let Some(store) = bridge::store(cx) {
                store.dismiss_facetime();
            }
        }))
        .into_any_element()
}

impl Header {
    /// Shows nothing: no FaceTime banner and no summary card, so the transcript can run up
    /// under the toolbar.
    pub fn is_empty(&self, cx: &App) -> bool {
        self.card.is_none() && bridge::store(cx).is_none_or(|store| store.state().facetime.is_none())
    }
}

impl Render for Header {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync(cx);
        let Some(store) = bridge::store(cx) else { return div().into_any_element() };
        let call = {
            let state = store.state();
            if self.chat.as_deref().and_then(|g| state.chat(g)).is_none() {
                return div().into_any_element();
            }
            state.facetime.clone()
        };
        let theme = cx.theme().clone();
        let card = self.card.clone().map(|card| {
            let (body, color) = match card {
                Card::Loading => ("Summarizing\u{2026}".to_owned(), theme.text.secondary),
                Card::Error(error) => (error, theme.text.danger),
                Card::Summary(summary) => (summary, theme.text.primary),
            };
            v_flex()
                .id("catch-up-card")
                .gap_2()
                .mx_3()
                .mt_2()
                .p_3()
                .rounded_xl()
                .bg(theme.surface.overlay)
                .child(
                    h_flex().gap_2().child(text("Catch Me Up").font_weight(FontWeight::SEMIBOLD).flex_1()).child(
                        icon_button("close-summary", Icon::X)
                            .tooltip("Close")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_card(cx))),
                    ),
                )
                .child(label(body).selectable().wraps().text_color(color))
        });
        v_flex()
            .id("conversation-header")
            .w_full()
            .flex_none()
            .children(call.map(|call| facetime_banner(&call, &theme)))
            .children(card)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(from_me: bool, date: i64) -> Arc<Message> {
        Arc::new(Message { guid: format!("m{date}"), from_me, date, ..Message::default() })
    }

    #[test]
    fn catches_up_from_my_last_message_or_the_last_fifty() {
        let thread = vec![message(false, 1), message(true, 2), message(false, 3), message(false, 4)];
        let since: Vec<i64> = messages_to_catch_up_on(&thread).iter().map(|m| m.date).collect();
        assert_eq!(since, vec![3, 4]);
        let many: Vec<Arc<Message>> = (0..80).map(|i| message(false, i)).collect();
        assert_eq!(messages_to_catch_up_on(&many).len(), CATCH_UP_FALLBACK);
    }
}
