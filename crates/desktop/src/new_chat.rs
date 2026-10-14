//! A new conversation: recipients in a To: row, suggestions from contacts and existing
//! one-to-one chats, the first message, send. Shown in place of the conversation; Escape or a
//! successful send closes it.

use messages_core::format::format_address;
use nd::prelude::*;

use crate::bridge;
use crate::root::Root;

pub fn is_address(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    if value.contains('@') {
        let mut parts = value.splitn(2, '@');
        let (local, domain) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        return !local.is_empty()
            && domain.contains('.')
            && !domain.starts_with('.')
            && !domain.ends_with('.')
            && !domain.contains(' ');
    }
    let digits = value.chars().filter(|c| c.is_ascii_digit()).count();
    digits >= 6 && value.chars().all(|c| c.is_ascii_digit() || matches!(c, '+' | ' ' | '(' | ')' | '-'))
}

/// Emails lowercased and trimmed, phone numbers as digits with their leading +.
pub fn normalize(value: &str) -> String {
    if value.contains('@') {
        value.trim().to_lowercase()
    } else {
        value.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect()
    }
}

#[derive(Clone)]
struct Recipient {
    address: String,
    name: Option<String>,
}

pub struct NewChat {
    /// The To: field takes focus on the first frame.
    focus: bool,
    root: WeakEntity<Root>,
    query: SharedString,
    draft: Draft,
    recipients: Vec<Recipient>,
    busy: bool,
    error: bool,
}

impl NewChat {
    pub fn new(root: WeakEntity<Root>, _cx: &mut Context<Self>) -> Self {
        Self {
            focus: true,
            root,
            query: SharedString::default(),
            draft: Draft::default(),
            recipients: Vec::new(),
            busy: false,
            error: false,
        }
    }

    fn add(&mut self, address: String, name: Option<String>, cx: &mut Context<Self>) {
        if !self.recipients.iter().any(|r| r.address == address) {
            self.recipients.push(Recipient { address, name });
        }
        self.query = SharedString::default();
        cx.notify();
    }

    fn suggestions(&self, cx: &App) -> Vec<Recipient> {
        let needle = self.query.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        let Some(store) = bridge::store(cx) else { return Vec::new() };
        let state = store.state();
        let chosen: std::collections::HashSet<&str> = self.recipients.iter().map(|r| r.address.as_str()).collect();
        let mut out: Vec<Recipient> = Vec::new();
        for contact in state.contacts.iter() {
            for address in &contact.addresses {
                if !chosen.contains(address.as_str())
                    && (contact.name.to_lowercase().contains(&needle) || address.to_lowercase().contains(&needle))
                {
                    out.push(Recipient { address: address.clone(), name: Some(contact.name.clone()) });
                }
            }
        }
        for chat in state.chats.iter().filter(|c| !c.is_group) {
            for handle in &chat.participants {
                if chosen.contains(handle.address.as_str()) || out.iter().any(|r| r.address == handle.address) {
                    continue;
                }
                let name = handle.name.clone().unwrap_or_default();
                if name.to_lowercase().contains(&needle) || handle.address.to_lowercase().contains(&needle) {
                    out.push(Recipient { address: handle.address.clone(), name: handle.name.clone() });
                }
            }
        }
        out.truncate(8);
        out
    }

    /// Return in the To: field: the first suggestion, else the typed address when it is one.
    fn commit_query(&mut self, cx: &mut Context<Self>) {
        if let Some(first) = self.suggestions(cx).into_iter().next() {
            self.add(first.address, first.name, cx);
        } else if is_address(&self.query) {
            let address = normalize(&self.query);
            self.add(address, None, cx);
        }
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.recipients.is_empty() || self.draft.text.trim().is_empty() {
            return;
        }
        let Some(store) = bridge::store(cx) else { return };
        let addresses: Vec<String> = self.recipients.iter().map(|r| r.address.clone()).collect();
        let text = self.draft.text.trim().to_owned();
        self.busy = true;
        self.error = false;
        cx.notify();
        crate::rt::spawn_for(
            cx.weak_entity(),
            async move { store.create_chat(&addresses, &text).await },
            |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        if let Some(root) = this.root.upgrade() {
                            root.update(cx, |root, cx| root.close_new_chat(cx));
                        }
                    }
                    Err(error) => {
                        tracing::warn!("new chat: {error}");
                        this.error = true;
                    }
                }
                cx.notify();
            },
        );
    }
}

impl Render for NewChat {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus && crate::ui::focus_id(window, cx, "new-chat-to") {
            self.focus = false;
        }
        let theme = cx.theme().clone();
        let suggestions = self.suggestions(cx);
        let fallback = (suggestions.is_empty() && is_address(&self.query)).then(|| normalize(&self.query));
        let ready = !self.recipients.is_empty() && !self.draft.text.trim().is_empty() && !self.busy;
        let chips = self.recipients.iter().enumerate().map(|(index, r)| {
            let label_text = r.name.clone().unwrap_or_else(|| format_address(&r.address));
            h_flex()
                .id(format!("recipient-{index}"))
                .gap_1()
                .pl_2()
                .rounded_full()
                .bg(theme.accent_container.bg)
                .child(text(label_text.clone()).text_color(theme.accent_container.fg))
                .child(
                    icon_button(format!("remove-recipient-{index}"), Icon::X)
                        .tooltip(format!("Remove {label_text}"))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            if index < this.recipients.len() {
                                this.recipients.remove(index);
                                cx.notify();
                            }
                        })),
                )
        });
        let rows = suggestions.iter().map(|s| {
            let pick = s.clone();
            h_flex()
                .id(format!("suggestion-{}", s.address))
                .gap_2()
                .px_4()
                .py_2()
                .on_tap(
                    cx.listener(move |this, _: &TapEvent, _, cx| this.add(pick.address.clone(), pick.name.clone(), cx)),
                )
                .child(avatar(Person::new(s.name.clone().unwrap_or_else(|| s.address.clone())), 32.))
                .child(
                    v_flex().child(text(s.name.clone().unwrap_or_else(|| format_address(&s.address)))).child(
                        text(format_address(&s.address))
                            .text_size(theme.type_scale.caption)
                            .text_color(theme.text.secondary),
                    ),
                )
        });
        v_flex()
            .id("new-chat-view")
            .size_full()
            .child(
                h_flex()
                    .h(px(52.))
                    .px_4()
                    .gap_2()
                    .child(label("New Message").text_style(TextStyle::Headline).flex_1())
                    .child(button("new-chat-cancel", "Cancel").on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        if let Some(root) = this.root.upgrade() {
                            root.update(cx, |root, cx| root.close_new_chat(cx));
                        }
                    }))),
            )
            .child(separator())
            .child(
                h_flex()
                    .px_4()
                    .py_2()
                    .gap_2()
                    .flex_wrap()
                    .child(text("To:").text_color(theme.text.secondary))
                    .children(chips)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(180.))
                            .key_context("NewChatTo")
                            .on_action(cx.listener(|this, _: &crate::actions::RemoveLastRecipient, _, cx| {
                                // Backspace on an empty To: field takes the last recipient.
                                if this.query.is_empty() && this.recipients.pop().is_some() {
                                    cx.notify();
                                } else {
                                    cx.propagate();
                                }
                            }))
                            .child(
                                text_field("new-chat-to")
                                    .text(self.query.clone())
                                    .placeholder("Name, phone number or email")
                                    .label("To")
                                    .flex_1()
                                    .min_w(px(180.))
                                    .on_change(cx.listener(|this, e: &TextChanged, _, cx| {
                                        this.query = e.text.clone();
                                        cx.notify();
                                    }))
                                    .on_submit(cx.listener(|this, _: &TextSubmitted, _, cx| this.commit_query(cx))),
                            ),
                    ),
            )
            .child(separator())
            .child(
                v_flex()
                    .id("new-chat-suggestions")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows)
                    .when_some(fallback, |col, address| {
                        let pick = address.clone();
                        col.child(
                            div()
                                .id("new-chat-fallback")
                                .px_4()
                                .py_2()
                                .on_tap(cx.listener(move |this, _: &TapEvent, _, cx| this.add(pick.clone(), None, cx)))
                                .child(text(format!("Message {}", format_address(&address)))),
                        )
                    })
                    .when(self.recipients.is_empty() && self.query.is_empty(), |col| {
                        col.child(
                            v_flex()
                                .items_center()
                                .pt_8()
                                .gap_1()
                                .child(label("Who Is This Going To?").text_style(TextStyle::Headline))
                                .child(label("Type a name, phone number or email above.").secondary()),
                        )
                    })
                    .when(self.error, |col| {
                        col.child(label("Could not start the conversation.").tone(LabelTone::Danger).mx_4())
                    }),
            )
            .child(
                div().px_4().py_2().child(
                    composer("new-chat-composer", &self.draft)
                        .placeholder(if self.recipients.is_empty() { "Add someone first" } else { "iMessage" })
                        .disabled(self.busy)
                        .without_attach()
                        .on_event(cx.listener(move |this, e: &ComposerEvent, _, cx| {
                            if *e == ComposerEvent::Send {
                                this.send(cx);
                            } else {
                                this.draft.apply(e);
                            }
                            cx.notify();
                        })),
                ),
            )
            .when(!ready, |col| col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_phone_numbers_or_emails() {
        assert!(is_address("+1 (555) 010-2000"));
        assert!(is_address("someone@example.com"));
        assert!(!is_address("12345"));
        assert!(!is_address("someone@example"));
        assert_eq!(normalize(" +1 (555) 010-2000 "), "+15550102000");
        assert_eq!(normalize(" Someone@Example.com "), "someone@example.com");
    }
}
