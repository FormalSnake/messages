//! Pick one or more recipients, write the first message, send it. `app.rs`
//! mounts `NewChat` in the main pane in place of header/thread/composer
//! while composing, and drops it when `on_close` runs (Escape or a
//! successful send).

use std::rc::Rc;

use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::format::format_address;

use crate::bridge::StoreHandle;
use crate::icons::IconName;
use crate::primitives::{IconButton, avatar};
use crate::theme::{TITLEBAR_HEIGHT, Theme, radius, spacing, type_scale};

fn is_address(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    if value.contains('@') {
        let mut parts = value.splitn(2, '@');
        let (local, domain) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        return !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.') && !domain.contains(' ');
    }
    let digits = value.chars().filter(|c| c.is_ascii_digit()).count();
    digits >= 6 && value.chars().all(|c| c.is_ascii_digit() || matches!(c, '+' | ' ' | '(' | ')' | '-'))
}

fn normalize(value: &str) -> String {
    if value.contains('@') {
        value.trim().to_lowercase()
    } else {
        value.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect()
    }
}

struct Recipient {
    address: String,
    name: Option<String>,
}

struct Suggestion {
    address: String,
    name: Option<String>,
}

pub struct NewChat {
    on_close: Rc<dyn Fn(&mut Window, &mut App)>,
    to_field: Entity<InputState>,
    draft: Entity<TextareaState>,
    recipients: Vec<Recipient>,
    busy: bool,
    error: bool,
    _subscriptions: Vec<Subscription>,
}

impl NewChat {
    pub fn new(window: &mut Window, on_close: impl Fn(&mut Window, &mut App) + 'static, cx: &mut Context<Self>) -> Self {
        let on_close = Rc::new(on_close);
        let to_field = cx.new(|cx| InputState::new(window, cx).placeholder("Name, phone number or email"));
        let draft = cx.new(|cx| TextareaState::new(window, cx).placeholder("Add someone first").auto_grow(1, 6));
        window.focus(&to_field.focus_handle(cx), cx);

        let to_sub = cx.subscribe_in(&to_field, window, |this: &mut Self, _state, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.commit_query(window, cx);
            }
        });
        let draft_sub = cx.subscribe_in(&draft, window, |this: &mut Self, _state, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { secondary: false, shift: false }) {
                this.send(window, cx);
            }
        });

        Self { on_close, to_field, draft, recipients: Vec::new(), busy: false, error: false, _subscriptions: vec![to_sub, draft_sub] }
    }

    #[cfg(test)]
    pub(crate) fn focus_draft(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.draft.focus_handle(cx), cx);
    }

    fn add(&mut self, address: String, name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if !self.recipients.iter().any(|item| item.address == address) {
            self.recipients.push(Recipient { address, name });
        }
        self.to_field.update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
    }

    fn suggestions(&self, cx: &App) -> Vec<Suggestion> {
        let needle = self.to_field.read(cx).value().trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        let Some(store) = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone()) else { return Vec::new() };
        let state = store.state();
        let chosen: std::collections::HashSet<&str> = self.recipients.iter().map(|item| item.address.as_str()).collect();
        let mut out: Vec<Suggestion> = Vec::new();
        for contact in state.contacts.iter() {
            for address in &contact.addresses {
                if chosen.contains(address.as_str()) {
                    continue;
                }
                if contact.name.to_lowercase().contains(&needle) || address.to_lowercase().contains(&needle) {
                    out.push(Suggestion { address: address.clone(), name: Some(contact.name.clone()) });
                }
            }
        }
        for chat in state.chats.iter() {
            if chat.is_group {
                continue;
            }
            for handle in &chat.participants {
                if chosen.contains(handle.address.as_str()) || out.iter().any(|item| item.address == handle.address) {
                    continue;
                }
                let name = handle.name.clone().unwrap_or_default();
                if name.to_lowercase().contains(&needle) || handle.address.to_lowercase().contains(&needle) {
                    out.push(Suggestion { address: handle.address.clone(), name: handle.name.clone() });
                }
            }
        }
        out.truncate(8);
        out
    }

    fn commit_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.to_field.read(cx).value().trim().to_string();
        if value.is_empty() {
            return;
        }
        let suggestions = self.suggestions(cx);
        if let Some(first) = suggestions.into_iter().next() {
            self.add(first.address, first.name, window, cx);
        } else if is_address(&value) {
            let address = normalize(&value);
            self.add(address, None, window, cx);
        }
    }

    fn remove(&mut self, address: &str, cx: &mut Context<Self>) {
        self.recipients.retain(|item| item.address != address);
        cx.notify();
    }

    fn ready(&self, cx: &App) -> bool {
        !self.draft.read(cx).value().trim().is_empty() && !self.recipients.is_empty() && !self.busy
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready(cx) {
            return;
        }
        let Some(store) = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone()) else { return };
        let text = self.draft.read(cx).value().trim().to_string();
        let addresses: Vec<String> = self.recipients.iter().map(|item| item.address.clone()).collect();
        self.busy = true;
        self.error = false;
        cx.notify();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let store_for_spawn = store.clone();
        store.spawn(async move {
            let result = store_for_spawn.create_chat(&addresses, &text).await;
            let _ = tx.send(result.is_ok());
        });
        let on_close = self.on_close.clone();
        cx.spawn_in(window, async move |this, cx| {
            let Ok(ok) = rx.await else { return };
            let _ = cx.update(|window, cx| {
                let _ = this.update(cx, |this, cx| {
                    if ok {
                        on_close(window, cx);
                    } else {
                        this.busy = false;
                        this.error = true;
                        cx.notify();
                    }
                });
            });
        })
        .detach();
    }
}

impl Render for NewChat {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        let suggestions = self.suggestions(cx);
        let needle = self.to_field.read(cx).value().trim().to_string();
        let ready = self.ready(cx);
        let has_recipients = !self.recipients.is_empty();
        let info_open = crate::app::root(cx).is_some_and(|app| app.read(cx).info_open());

        let to_row = div()
            .flex()
            .flex_row()
            .items_center()
            .flex_wrap()
            .gap(spacing::X2)
            .px(spacing::X4)
            .py(spacing::X2)
            .min_h(px(44.))
            .flex_shrink_0()
            .border_b_1()
            .border_color(palette.sidebar_border)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if event.keystroke.key == "backspace" && this.to_field.read(cx).value().is_empty() && !this.recipients.is_empty() {
                    this.recipients.pop();
                    cx.notify();
                }
            }))
            .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).text_color(palette.secondary).flex_shrink_0().child("To:"))
            .children(self.recipients.iter().map(|item| {
                let label = item.name.clone().unwrap_or_else(|| format_address(&item.address));
                let address = item.address.clone();
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(2.))
                    .h(px(24.))
                    .pl(spacing::X2)
                    .pr(spacing::X1)
                    .rounded(px(12.))
                    .bg(palette.selected_soft)
                    .flex_shrink_0()
                    .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.accent).child(label))
                    .child(IconButton::new(ElementId::Name(format!("remove-{address}").into()), IconName::Close, format!("Remove {address}")).size(px(11.)).hit(px(18.)).color(palette.accent).on_click(cx.listener(move |this, _, _, cx| this.remove(&address, cx))))
            }))
            .child(Input::new(&self.to_field).appearance(false));

        let list = div().id("new-chat-suggestions").flex().flex_col().flex_grow(1.).min_h(px(0.)).overflow_y_scroll().px(spacing::X2).pt(spacing::X2);
        let list = if !suggestions.is_empty() {
            list.children(suggestions.into_iter().map(|item| {
                let label = item.name.clone().unwrap_or_else(|| format_address(&item.address));
                let subtitle = item.name.clone().map(|_| format_address(&item.address));
                let address = item.address.clone();
                let name = item.name.clone();
                div()
                    .id(ElementId::Name(format!("suggest-{}", item.address).into()))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(spacing::X2)
                    .h(px(44.))
                    .px(spacing::X2)
                    .rounded(radius::ROW)
                    .cursor_pointer()
                    .flex_shrink_0()
                    .hover(move |style| style.bg(palette.raised))
                    .active(move |style| style.bg(palette.raised_hover))
                    .on_click(cx.listener(move |this, _, window, cx| this.add(address.clone(), name.clone(), window, cx)))
                    .child(avatar(Some(&messages_core::Handle { address: item.address.clone(), service: messages_core::Service::IMessage, name: item.name.clone(), avatar: None }), None, px(30.), cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_grow(1.)
                            .min_w(px(0.))
                            .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).text_color(palette.text).text_ellipsis().child(label))
                            .when_some(subtitle, |el, subtitle| {
                                el.child(div().text_size(type_scale::MICRO.font_size).line_height(type_scale::MICRO.line_height).text_color(palette.secondary).text_ellipsis().child(subtitle))
                            }),
                    )
            }))
        } else if !needle.is_empty() && is_address(&needle) {
            let query = needle.clone();
            list.child(
                div()
                    .id("suggest-raw")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(spacing::X2)
                    .h(px(44.))
                    .px(spacing::X2)
                    .rounded(radius::ROW)
                    .cursor_pointer()
                    .flex_shrink_0()
                    .hover(move |style| style.bg(palette.raised))
                    .active(move |style| style.bg(palette.raised_hover))
                    .on_click(cx.listener(|this, _, window, cx| this.commit_query(window, cx)))
                    .child(div().w(px(30.)).h(px(30.)).rounded(px(15.)).bg(palette.selected_soft).flex().items_center().justify_center().flex_shrink_0().child(crate::icons::Icon::new(IconName::Plus).size(px(15.)).color(palette.accent).strong(true)))
                    .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).text_color(palette.accent).child(format!("Message {query}"))),
            )
        } else if needle.is_empty() && !has_recipients {
            list.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(spacing::X1)
                    .pt(spacing::X10)
                    .px(spacing::X4)
                    .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).font_weight(FontWeight::SEMIBOLD).text_color(palette.text).child("Who is this going to?"))
                    .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).text_align(TextAlign::Center).child("Type a name, phone number or email above.")),
            )
        } else {
            list
        };

        let placeholder = if has_recipients { "iMessage" } else { "Add someone first" };
        self.draft.update(cx, |state, cx| {
            state.set_placeholder(placeholder, window, cx);
        });

        let composer = div()
            .flex()
            .flex_row()
            .items_end()
            .gap(spacing::X1)
            .px(spacing::X4)
            .pt(spacing::X2)
            .pb(spacing::X3)
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(spacing::X1)
                    .flex_grow(1.)
                    .min_w(px(0.))
                    .min_h(px(34.))
                    .rounded(px(17.))
                    .border_1()
                    .border_color(palette.separator)
                    .bg(palette.canvas)
                    .pl(spacing::X3)
                    .pr(spacing::X1)
                    .py(spacing::X1)
                    .child(Textarea::new(&self.draft).appearance(false))
                    .child(
                        div()
                            .id("new-chat-send")
                            .w(px(24.))
                            .h(px(24.))
                            .rounded(px(12.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .bg(if ready { palette.accent } else { palette.ghost })
                            .opacity(if ready { 1. } else { 0.5 })
                            .when(ready, |el| el.hover(|style| style.opacity(0.88)).active(|style| style.opacity(0.7)))
                            .on_click(cx.listener(|this, _, window, cx| this.send(window, cx)))
                            .child(crate::icons::Icon::new(IconName::Send).size(px(14.)).color(palette.on_accent).strong(true)),
                    ),
            );

        div()
            .id("new-chat")
            .flex()
            .flex_col()
            .flex_grow(1.)
            .min_w(px(0.))
            .h_full()
            .on_key_down({
                let on_close = self.on_close.clone();
                move |event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        on_close(window, cx);
                    }
                }
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .h(TITLEBAR_HEIGHT)
                    .pl(spacing::X4)
                    .pr(spacing::X2 + if info_open { px(0.) } else { crate::chrome::caption_reserve(window) })
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(palette.separator)
                    .child(div().text_size(type_scale::TITLE.font_size).line_height(type_scale::TITLE.line_height).text_color(palette.text).flex_grow(1.).child("New message"))
                    .child(IconButton::new("cancel-new-chat", IconName::Close, "Cancel (Esc)").on_click({
                        let on_close = self.on_close.clone();
                        move |_, window, cx| on_close(window, cx)
                    })),
            )
            .child(to_row)
            .child(list)
            .child(composer)
    }
}
