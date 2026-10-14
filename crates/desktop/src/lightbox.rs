//! Every picture and video in the loaded conversation, one at a time, in a panel over the
//! window. Previous and next wrap; Copy puts the file on the clipboard; Open hands it to the
//! system viewer, which is also where a video plays (nd has no video surface yet).

use messages_core::conversations::conversation_messages;
use messages_core::format::format_bytes;
use messages_core::{Attachment, MessagesStore};
use nd::prelude::*;

use crate::actions::{AttachmentRef, MessageRef};
use crate::thread::Thread;
use crate::ui;

const PANEL: (f32, f32) = (860., 640.);
const CAPTION: f32 = 48.;

#[derive(Clone)]
struct Entry {
    attachment: Attachment,
    reference: AttachmentRef,
}

pub struct Lightbox {
    store: MessagesStore,
    chat: String,
    current: String,
    copied: bool,
}

impl Lightbox {
    /// The lightbox on `reference`, or `None` when the message is not loaded.
    pub fn open(store: &MessagesStore, reference: &AttachmentRef) -> Option<Self> {
        let chat = {
            let state = store.state();
            let guid = state.selected_chat.clone()?;
            conversation_messages(&state, &guid).iter().any(|m| m.guid == reference.at.message).then_some(guid)?
        };
        Some(Self { store: store.clone(), chat, current: reference.attachment.clone(), copied: false })
    }

    /// Every non-hidden picture and video in the loaded thread, in date order.
    fn entries(&self) -> Vec<Entry> {
        let state = self.store.state();
        conversation_messages(&state, &self.chat)
            .iter()
            .flat_map(|message| {
                message
                    .attachments
                    .iter()
                    .filter(|a| !a.hidden && (a.mime.starts_with("image/") || a.mime.starts_with("video/")))
                    .map(|a| Entry {
                        attachment: a.clone(),
                        reference: AttachmentRef {
                            at: MessageRef { chat: message.chat_guid.clone(), message: message.guid.clone() },
                            attachment: a.guid.clone(),
                        },
                    })
            })
            .collect()
    }

    pub fn step(&mut self, delta: isize) {
        let entries = self.entries();
        if entries.is_empty() {
            return;
        }
        let index = entries.iter().position(|e| e.attachment.guid == self.current).unwrap_or(0) as isize;
        let next = (index + delta).rem_euclid(entries.len() as isize) as usize;
        self.current = entries[next].attachment.guid.clone();
        self.copied = false;
    }

    pub fn mark_copied(&mut self) {
        self.copied = true;
    }

    pub fn current(&self) -> Option<AttachmentRef> {
        self.entries().into_iter().find(|e| e.attachment.guid == self.current).map(|e| e.reference)
    }

    pub fn element(&self, cx: &mut Context<Thread>) -> AnyElement {
        let entries = self.entries();
        let entry = entries.iter().find(|e| e.attachment.guid == self.current).cloned();
        let theme = cx.theme().clone();
        let (w, h) = (PANEL.0 - 32., PANEL.1 - CAPTION - 32.);
        let picture = match &entry {
            Some(entry) if entry.attachment.mime.starts_with("image/") => {
                match entry.attachment.local_path.as_ref().and_then(|p| ui::image_source(&p.to_string_lossy())) {
                    Some(source) => image(source)
                        .id(format!("lightbox-{}", entry.attachment.guid))
                        .alt(entry.attachment.name.clone())
                        .contain()
                        .w(px(w))
                        .h(px(h))
                        .into_any_element(),
                    None => label("Loading\u{2026}").secondary().into_any_element(),
                }
            }
            Some(_) => label("Videos play in the system player: choose Open.").secondary().into_any_element(),
            None => label("Could not load.").secondary().into_any_element(),
        };
        let caption = entry
            .as_ref()
            .map(|e| format!("{} \u{b7} {}", e.attachment.name, format_bytes(e.attachment.bytes)))
            .unwrap_or_default();
        let many = entries.len() > 1;
        panel("lightbox")
            .w(px(PANEL.0))
            .h(px(PANEL.1))
            .p_4()
            .key_context("Lightbox")
            .on_dismiss(cx.listener(|this, _: &Dismissed, _, cx| this.close_lightbox(cx)))
            .child(
                v_flex()
                    .size_full()
                    .gap_2()
                    .child(div().flex_1().min_h_0().flex().items_center().justify_center().child(picture))
                    .child(
                        h_flex()
                            .h(px(CAPTION))
                            .gap_2()
                            .child(text(caption).text_color(theme.text.secondary).flex_1().min_w_0())
                            .when(many, |row| {
                                row.child(icon_button("lightbox-prev", Icon::ChevronLeft).tooltip("Previous").on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| {
                                        if let Some(lightbox) = this.lightbox_mut() {
                                            lightbox.step(-1);
                                        }
                                        cx.notify();
                                    }),
                                ))
                                .child(
                                    icon_button("lightbox-next", Icon::ChevronRight).tooltip("Next").on_click(
                                        cx.listener(|this, _: &ClickEvent, _, cx| {
                                            if let Some(lightbox) = this.lightbox_mut() {
                                                lightbox.step(1);
                                            }
                                            cx.notify();
                                        }),
                                    ),
                                )
                            })
                            .child(button("lightbox-copy", if self.copied { "Copied" } else { "Copy" }).on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| {
                                    let current = this.lightbox_mut().and_then(|l| {
                                        l.copied = true;
                                        l.current()
                                    });
                                    if let Some(reference) = current {
                                        this.copy_attachment(&reference, cx);
                                    }
                                    cx.notify();
                                }),
                            ))
                            .child(button("lightbox-open", "Open").on_click(cx.listener(
                                |this, _: &ClickEvent, _, cx| {
                                    if let Some(reference) = this.lightbox_mut().and_then(|l| l.current()) {
                                        this.open_attachment(&reference, cx);
                                    }
                                },
                            )))
                            .child(
                                button("lightbox-close", "Close")
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_lightbox(cx))),
                            ),
                    ),
            )
            .into_any_element()
    }
}
