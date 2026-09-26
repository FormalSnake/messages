//! Every picture in the loaded conversation, one at a time, over the whole
//! window. The thread mounts it
//! as a deferred overlay; `open` is how any screen (the thread, the details
//! gallery) asks for it.

use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::conversations::conversation_messages;
use messages_core::format::format_bytes;
use messages_core::{Attachment, MessagesStore};

use crate::attachments::{image_source, is_data_url, run_for};
use crate::icons::IconName;
use crate::motion::{DURATION_BASE, DURATION_FAST, EASE_OUT, cubic_bezier};
use crate::primitives::IconButton;
use crate::theme::{radius, spacing, type_scale};

/// Margin kept between the image and every edge of the window.
const MARGIN: f32 = 48.;
const CAPTION_HEIGHT: f32 = 40.;
const COPIED_FOR: Duration = Duration::from_millis(1500);

/// Shows the picture `attachment_guid` of the message in `chat_guid`.
pub fn open(chat_guid: &str, attachment_guid: &str, window: &mut Window, cx: &mut App) {
    let (chat, attachment) = (chat_guid.to_owned(), attachment_guid.to_owned());
    crate::thread::with_thread(cx, |thread, cx| thread.open_lightbox(&chat, &attachment, window, cx));
}

struct Entry {
    attachment: Attachment,
    chat_guid: String,
    message_guid: String,
}

pub struct Lightbox {
    store: MessagesStore,
    chat_guid: String,
    current: String,
    failed: bool,
    fetching: Option<String>,
    copied: bool,
    copied_task: Option<Task<()>>,
    focus: FocusHandle,
    opened: Instant,
    closing: Option<Instant>,
}

impl Lightbox {
    pub fn open(store: MessagesStore, chat_guid: String, attachment_guid: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Lightbox { store, chat_guid, current: attachment_guid, failed: false, fetching: None, copied: false, copied_task: None, focus, opened: Instant::now(), closing: None }
    }

    /// Every non-hidden picture across the loaded thread, in the date order the messages carry.
    fn entries(&self) -> Vec<Entry> {
        let state = self.store.state();
        conversation_messages(&state, &self.chat_guid)
            .iter()
            .flat_map(|message| {
                message.attachments.iter().filter(|attachment| !attachment.hidden && attachment.mime.starts_with("image/")).map(|attachment| Entry {
                    attachment: attachment.clone(),
                    chat_guid: message.chat_guid.clone(),
                    message_guid: message.guid.clone(),
                })
            })
            .collect()
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        if self.closing.is_some() {
            return;
        }
        self.closing = Some(Instant::now());
        cx.notify();
        cx.spawn(async move |_, cx| {
            cx.background_executor().timer(DURATION_FAST).await;
            cx.update(|cx| crate::thread::with_thread(cx, |thread, cx| thread.close_lightbox(cx)));
        })
        .detach();
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let entries = self.entries();
        if entries.is_empty() {
            return;
        }
        let index = entries.iter().position(|entry| entry.attachment.guid == self.current).unwrap_or(0) as isize;
        let next = (index + delta).rem_euclid(entries.len() as isize) as usize;
        self.current = entries[next].attachment.guid.clone();
        self.failed = false;
        self.copied = false;
        cx.notify();
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.entries().into_iter().find(|entry| entry.attachment.guid == self.current) else { return };
        let Some(path) = entry.attachment.local_path.filter(|path| !is_data_url(path)) else { return };
        let mime = entry.attachment.mime;
        run_for(&self.store, async move { messages_core::clipboard::copy_file(&path, &mime).await }, cx, |this, _, cx| {
            this.copied = true;
            cx.notify();
            this.copied_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(COPIED_FOR).await;
                let _ = this.update(cx, |this, cx| {
                    this.copied = false;
                    cx.notify();
                });
            }));
        });
    }

    fn fetch(&mut self, entry: &Entry, cx: &mut Context<Self>) {
        if self.fetching.as_deref() == Some(entry.attachment.guid.as_str()) || self.failed {
            return;
        }
        self.fetching = Some(entry.attachment.guid.clone());
        let store = self.store.clone();
        let (chat, message, guid, name, mime) = (entry.chat_guid.clone(), entry.message_guid.clone(), entry.attachment.guid.clone(), entry.attachment.name.clone(), entry.attachment.mime.clone());
        let key = guid.clone();
        run_for(&self.store, async move { store.attachment_src(&chat, &message, &guid, &name, Some(&mime)).await.is_ok() }, cx, move |this, ok, cx| {
            if this.fetching.as_deref() == Some(key.as_str()) {
                this.fetching = None;
            }
            if !ok && this.current == key {
                this.failed = true;
            }
            cx.notify();
        });
    }
}

fn label_button(id: &'static str, label: &'static str, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> impl IntoElement {
    div()
        .id(id)
        .h(px(28.))
        .px(spacing::X3)
        .rounded(radius::CONTROL)
        .flex()
        .items_center()
        .bg(hsla(0., 0., 1., 0.12))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(0., 0., 1., 0.2)))
        .on_click(on_click)
        .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).font_weight(FontWeight::SEMIBOLD).text_color(white()).child(label))
}

impl Render for Lightbox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let entries = self.entries();
        let entry = entries.into_iter().find(|entry| entry.attachment.guid == self.current);
        if let Some(entry) = entry.as_ref().filter(|entry| entry.attachment.local_path.is_none()) {
            self.fetch(entry, cx);
        }

        let (x1, y1, x2, y2) = EASE_OUT;
        let ease = cubic_bezier(x1, y1, x2, y2);
        let opacity = match self.closing {
            Some(started) => 1. - ease((started.elapsed().as_secs_f32() / DURATION_FAST.as_secs_f32()).min(1.)),
            None => ease((self.opened.elapsed().as_secs_f32() / DURATION_BASE.as_secs_f32()).min(1.)),
        };
        if self.closing.is_some() || self.opened.elapsed() < DURATION_BASE {
            window.request_animation_frame();
        }

        let width = f32::from(viewport.width);
        let height = f32::from(viewport.height);
        let available_w = (width - MARGIN * 2.).max(120.);
        let available_h = (height - MARGIN * 2. - CAPTION_HEIGHT - 8.).max(120.);
        let (natural_w, natural_h) = entry
            .as_ref()
            .and_then(|entry| entry.attachment.width.zip(entry.attachment.height))
            .filter(|(w, h)| *w > 0 && *h > 0)
            .map(|(w, h)| (w as f32, h as f32))
            .unwrap_or((4., 3.));
        let scale = (available_w / natural_w).min(available_h / natural_h);
        let (display_w, display_h) = ((natural_w * scale).round(), (natural_h * scale).round());
        let src = entry.as_ref().and_then(|entry| entry.attachment.local_path.clone());
        let copied = self.copied;

        let buttons = div()
            .absolute()
            .top(px(20.))
            .right(px(20.))
            .flex()
            .flex_row()
            .items_center()
            .gap(spacing::X2)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when_some(src.clone().filter(|path| !is_data_url(path)), |el, path| {
                el.child(label_button("lightbox-copy", if copied { "Copied" } else { "Copy" }, cx.listener(|this, _, _, cx| this.copy(cx))))
                    .child(label_button("lightbox-open", "Open", move |_, _, _| messages_core::open::open_external(&path.to_string_lossy())))
            })
            .child(IconButton::new("lightbox-close", IconName::Close, "Close").color(white()).hit(px(28.)).on_click(cx.listener(|this, _, _, cx| this.close(cx))));

        let content = entry.map(|entry| {
            let failed = self.failed;
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(spacing::X2)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(match &src {
                    Some(path) if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("gif")) => img(image_source(path)).w(px(display_w)).h(px(display_h)).object_fit(ObjectFit::Contain).into_any_element(),
                    Some(path) => img(crate::attachments::sized_image_source(path, px(display_w), px(display_h), ObjectFit::Contain)).w(px(display_w)).h(px(display_h)).object_fit(ObjectFit::Contain).into_any_element(),
                    None => div()
                        .w(px(display_w))
                        .h(px(display_h))
                        .rounded(radius::CARD)
                        .bg(hsla(0., 0., 1., 0.08))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .text_size(type_scale::CAPTION.font_size)
                                .line_height(type_scale::CAPTION.line_height)
                                .text_color(hsla(0., 0., 1., 0.7))
                                .child(if failed { "Could not load." } else { "Loading\u{2026}" }),
                        )
                        .into_any_element(),
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(white()).child(entry.attachment.name.clone()))
                        .child(div().text_size(type_scale::MICRO.font_size).line_height(type_scale::MICRO.line_height).text_color(hsla(0., 0., 1., 0.65)).child(format_bytes(entry.attachment.bytes))),
                )
        });

        div()
            .id("lightbox")
            .track_focus(&self.focus)
            .occlude()
            .w(viewport.width)
            .h(viewport.height)
            .bg(hsla(0., 0., 0., 0.9))
            .opacity(opacity)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .on_action(cx.listener(|this, _: &crate::app::Dismiss, _, cx| this.close(cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let key = event.keystroke.key.as_str();
                if key == "c" && event.keystroke.modifiers.secondary() {
                    this.copy(cx);
                } else if key == "left" || key == "k" {
                    this.step(-1, cx);
                } else if key == "right" || key == "j" {
                    this.step(1, cx);
                } else {
                    return;
                }
                cx.stop_propagation();
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close(cx)))
            .child(buttons)
            .children(content)
    }
}
