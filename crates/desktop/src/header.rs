//! Port of the conversation header in `apps/desktop/src/ui/header.tsx`.
//! `app.rs` mounts `ConversationHeader` above `Thread`.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::assistant::{CanaryLlmClient, SummarizeMessage, MAX_SUMMARY_MESSAGES};
use messages_core::conversations::{conversation_focus, conversation_guid, conversation_handles, conversation_messages};
use messages_core::{FocusStatus, chat_title, handle_name};

use crate::app::ToggleInfo;
use crate::bridge::{Bridge, StoreHandle, Topic};
use crate::icons::{Icon, IconName};
use crate::primitives::{IconButton, avatar};
use crate::theme::{Theme, TITLEBAR_HEIGHT, spacing, type_scale};

pub struct ConversationHeader {
    current_chat: Option<String>,
    canaryllm_key: Option<String>,
    summarizing: bool,
    summary: Option<String>,
    summary_error: Option<String>,
}

impl ConversationHeader {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade();
        Bridge::watch(cx, Topic::Selection, weak.clone().into());
        Bridge::watch(cx, Topic::Focus, weak.into());
        let mut this = Self { current_chat: None, canaryllm_key: None, summarizing: false, summary: None, summary_error: None };
        this.load_canaryllm_key(cx);
        this
    }

    fn store(cx: &App) -> Option<messages_core::MessagesStore> {
        cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone())
    }

    fn load_canaryllm_key(&mut self, cx: &mut Context<Self>) {
        let Some(store) = Self::store(cx) else { return };
        let (tx, rx) = tokio::sync::oneshot::channel();
        store.spawn(async move {
            let config = messages_core::config::load_config().await;
            let _ = tx.send(config.canaryllm.map(|c| c.api_key));
        });
        cx.spawn(async move |this, cx| {
            if let Ok(key) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.canaryllm_key = key;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Chat switches close the summary card and abort in-flight work by
    /// simply orphaning it: the response lands on the old chat's state only
    /// if this is still the current chat when it comes back.
    fn sync_selection(&mut self, cx: &mut Context<Self>) {
        let Some(store) = Self::store(cx) else { return };
        let state = store.state();
        let primary = state.selected_chat.as_deref().map(|guid| conversation_guid(&state.grouping, guid).to_owned());
        if primary == self.current_chat {
            return;
        }
        self.current_chat = primary;
        self.summarizing = false;
        self.summary = None;
        self.summary_error = None;
    }

    fn catch_me_up(&mut self, cx: &mut Context<Self>) {
        let Some(store) = Self::store(cx) else { return };
        let Some(chat) = self.current_chat.clone() else { return };
        let Some(api_key) = self.canaryllm_key.clone() else { return };
        let state = store.state();
        let handles = conversation_handles(&state, &chat);
        let mut messages = conversation_messages(&state, &chat);
        drop(state);
        let last_sent = messages.iter().rposition(|message| message.from_me);
        let scope: Vec<_> = match last_sent {
            Some(index) => messages.split_off(index + 1),
            None => {
                let start = messages.len().saturating_sub(50);
                messages.split_off(start)
            }
        };
        let payload: Vec<SummarizeMessage> = scope
            .iter()
            .take(MAX_SUMMARY_MESSAGES)
            .map(|message| {
                let sender = if message.from_me {
                    "Me".to_owned()
                } else {
                    message
                        .sender
                        .as_ref()
                        .map(handle_name)
                        .map(str::to_owned)
                        .or_else(|| handles.first().map(handle_name).map(str::to_owned))
                        .unwrap_or_default()
                };
                let text = if !message.text.is_empty() { message.text.clone() } else if !message.attachments.is_empty() { "[attachment]".to_owned() } else { String::new() };
                SummarizeMessage { sender, text, date: message.date, from_me: message.from_me }
            })
            .collect();

        self.summarizing = true;
        self.summary = None;
        self.summary_error = None;
        cx.notify();

        let (tx, rx) = tokio::sync::oneshot::channel();
        store.spawn(async move {
            let client = CanaryLlmClient::new(api_key, None, None, reqwest::Client::new());
            let result = client.summarize(&payload).await;
            let _ = tx.send(result.map_err(|error| error.to_string()));
        });
        let chat_for_result = chat.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(result) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    if this.current_chat.as_deref() != Some(chat_for_result.as_str()) {
                        return;
                    }
                    this.summarizing = false;
                    match result {
                        Ok(text) => this.summary = Some(text),
                        Err(error) => this.summary_error = Some(error),
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn close_summary(&mut self, cx: &mut Context<Self>) {
        self.summary = None;
        self.summary_error = None;
        cx.notify();
    }
}

impl Render for ConversationHeader {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_selection(cx);
        let palette = Theme::get(cx);
        let Some(store) = Self::store(cx) else {
            return div().w_full().h(TITLEBAR_HEIGHT).flex_shrink_0().border_b_1().border_color(palette.sidebar_border);
        };
        let state = store.state();
        let Some(chat) = self.current_chat.as_deref().and_then(|guid| state.chat(guid)).cloned() else {
            drop(state);
            return div().w_full().h(TITLEBAR_HEIGHT).flex_shrink_0().border_b_1().border_color(palette.sidebar_border);
        };
        let title = chat_title(&chat);
        let focus = self.current_chat.as_deref().map(|guid| conversation_focus(&state, guid)).unwrap_or_default();
        let capabilities = state.capabilities;
        drop(state);

        let subtitle: SharedString = if chat.is_group {
            title.clone().into()
        } else if let Some(first) = chat.participants.first() {
            let name = handle_name(first);
            let service_label = match chat.service {
                messages_core::Service::IMessage => "iMessage",
                messages_core::Service::Sms => "SMS",
                messages_core::Service::Rcs => "RCS",
            };
            if name != first.address {
                format!("{name} · {service_label}").into()
            } else {
                match chat.service {
                    messages_core::Service::IMessage => "iMessage".into(),
                    _ => "Text message".into(),
                }
            }
        } else {
            "".into()
        };

        let _ = window;
        let header = div()
            .w_full()
            .h(TITLEBAR_HEIGHT)
            .flex_shrink_0()
            .border_b_1()
            .border_color(palette.sidebar_border)
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_grow(1.)
                    .flex()
                    .items_center()
                    .px(spacing::X4)
                    .gap(spacing::X2)
                    .child(
                        div()
                            .id("header-identity")
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(spacing::X2)
                            .flex_grow(1.)
                            .min_w(px(0.))
                            .on_click(cx.listener(|_, _, window, cx| window.dispatch_action(Box::new(ToggleInfo), cx)))
                            .child(avatar(chat.participants.first(), Some(&chat), px(30.), cx))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .min_w(px(0.))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(spacing::X1)
                                            .child(div().text_size(type_scale::TITLE.font_size).line_height(type_scale::TITLE.line_height).text_color(palette.text).child(title))
                                            .when(focus == FocusStatus::Silenced, |el| {
                                                el.child(Icon::new(IconName::Silenced).size(px(12.)).color(palette.secondary))
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_size(type_scale::CAPTION.font_size)
                                            .line_height(type_scale::CAPTION.line_height)
                                            .text_color(palette.secondary)
                                            .child(subtitle),
                                    ),
                            ),
                    )
                    .when(self.canaryllm_key.is_some(), |el| {
                        el.child(
                            IconButton::new("catch-me-up", IconName::Sparkles, "Catch me up").disabled(self.summarizing).on_click(cx.listener(
                                |this, _, _, cx| this.catch_me_up(cx),
                            )),
                        )
                    })
                    .when(capabilities.facetime && !chat.is_group, |el| {
                        el.child(IconButton::new("facetime", IconName::Video, "FaceTime").on_click(cx.listener({
                            let chat_guid = chat.guid.clone();
                            move |this, _, _, cx| {
                                if let Some(store) = Self::store(cx) {
                                    let chat_guid = chat_guid.clone();
                                    let task_store = store.clone();
                                    store.spawn(async move { task_store.start_facetime(&chat_guid).await });
                                }
                                let _ = this;
                            }
                        })))
                    })
                    .child(IconButton::new("toggle-info", IconName::Info, "Show details (⌘I)").on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(ToggleInfo), cx);
                    })),
            )
            .when(self.summarizing || self.summary.is_some() || self.summary_error.is_some(), |el| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_start()
                        .gap(spacing::X2)
                        .px(spacing::X4)
                        .pb(spacing::X2)
                        .child(
                            div()
                                .flex_grow(1.)
                                .min_w(px(0.))
                                .text_size(type_scale::CAPTION.font_size)
                                .line_height(type_scale::CAPTION.line_height)
                                .text_color(if self.summary_error.is_some() { palette.danger } else { palette.text })
                                .child(if self.summarizing {
                                    "Summarizing…".to_owned()
                                } else if let Some(error) = &self.summary_error {
                                    error.clone()
                                } else {
                                    self.summary.clone().unwrap_or_default()
                                }),
                        )
                        .child(IconButton::new("close-summary", IconName::Close, "Close").size(px(12.)).hit(px(20.)).on_click(cx.listener(|this, _, _, cx| this.close_summary(cx)))),
                )
            });
        header
    }
}
