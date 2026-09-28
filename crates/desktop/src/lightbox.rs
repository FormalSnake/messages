//! Every picture and video in the loaded conversation, one at a time, over
//! the whole window. The thread mounts it as a deferred overlay; `open` is how
//! any screen (the thread, the details gallery) asks for it. A video entry
//! hands its box to `video::VideoPlayer`, which lives only while it is the
//! one on screen.

use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::conversations::conversation_messages;
use messages_core::format::format_bytes;
use messages_core::{Attachment, MessagesStore};

use crate::attachments::{is_data_url, run_for};
use crate::icons::IconName;
use crate::motion::{DURATION_BASE, DURATION_FAST, eased_since};
use crate::primitives::IconButton;
use crate::theme::{radius, spacing, type_scale};
use crate::video::VideoPlayer;

/// Margin kept between the image and every edge of the window.
const MARGIN: f32 = 48.;
const CAPTION_HEIGHT: f32 = 40.;
const COPIED_FOR: Duration = Duration::from_millis(1500);

/// Shows the picture `attachment_guid` of the message in `chat_guid`.
pub fn open(chat_guid: &str, attachment_guid: &str, window: &mut Window, cx: &mut App) {
    let (chat, attachment) = (chat_guid.to_owned(), attachment_guid.to_owned());
    crate::thread::with_thread(cx, |thread, cx| thread.open_lightbox(&chat, &attachment, window, cx));
}

#[derive(Clone)]
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
    /// The player for the video on screen, keyed by its attachment guid.
    player: Option<(String, Entity<VideoPlayer>)>,
}

fn is_video(attachment: &Attachment) -> bool {
    attachment.mime.starts_with("video/")
}

impl Lightbox {
    pub fn open(store: MessagesStore, chat_guid: String, attachment_guid: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Lightbox { store, chat_guid, current: attachment_guid, failed: false, fetching: None, copied: false, copied_task: None, focus, opened: Instant::now(), closing: None, player: None }
    }

    /// Every non-hidden picture and video across the loaded thread, in the date order the messages carry.
    fn entries(&self) -> Vec<Entry> {
        let state = self.store.state();
        conversation_messages(&state, &self.chat_guid)
            .iter()
            .flat_map(|message| {
                message.attachments.iter().filter(|attachment| !attachment.hidden && (attachment.mime.starts_with("image/") || is_video(attachment))).map(|attachment| Entry {
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

    /// The player for the video on screen, made when the entry changes and
    /// dropped (killing its decoders) as soon as something else is shown.
    fn player_for(&mut self, entry: Option<&Entry>, box_size: (f32, f32), scale: f32, cx: &mut Context<Self>) -> Option<Entity<VideoPlayer>> {
        let wanted = entry.filter(|entry| is_video(&entry.attachment)).and_then(|entry| entry.attachment.local_path.clone().filter(|path| !is_data_url(path)).map(|path| (entry.attachment.guid.clone(), path)));
        let Some((guid, path)) = wanted else {
            if self.player.take().is_some() {
                cx.notify();
            }
            return None;
        };
        if let Some((current, player)) = &self.player
            && *current == guid
        {
            player.update(cx, |player, _| player.set_box(box_size, scale));
            return Some(player.clone());
        }
        let store = self.store.clone();
        let player = cx.new(|cx| VideoPlayer::new(store, path, guid.clone(), box_size, scale, cx));
        cx.subscribe(&player, |_, _, _: &crate::video::Probed, cx| cx.notify()).detach();
        self.player = Some((guid, player.clone()));
        Some(player)
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
        let index = entries.iter().position(|entry| entry.attachment.guid == self.current);
        let count = entries.len();
        let entry = index.map(|index| &entries[index]);
        if let Some(entry) = entry.filter(|entry| entry.attachment.local_path.is_none()) {
            self.fetch(entry, cx);
        }

        let opacity = match self.closing {
            Some(started) => 1. - eased_since(started, DURATION_FAST, cx).unwrap_or(1.),
            None => eased_since(self.opened, DURATION_BASE, cx).unwrap_or(1.),
        };
        if (self.closing.is_some() || self.opened.elapsed() < DURATION_BASE) && !cx.reduce_motion() {
            window.request_animation_frame();
        }

        let width = f32::from(viewport.width);
        let height = f32::from(viewport.height);
        let available_w = (width - MARGIN * 2.).max(120.);
        let available_h = (height - MARGIN * 2. - CAPTION_HEIGHT - 8.).max(120.);
        let probed = self.player.as_ref().filter(|(guid, _)| Some(guid.as_str()) == entry.map(|entry| entry.attachment.guid.as_str())).and_then(|(_, player)| player.read(cx).info().map(|info| (info.width, info.height)));
        let (natural_w, natural_h) = probed
            .or_else(|| entry.and_then(|entry| entry.attachment.width.zip(entry.attachment.height)))
            .filter(|(w, h)| *w > 0 && *h > 0)
            .map(|(w, h)| (w as f32, h as f32))
            .unwrap_or_else(|| if entry.is_some_and(|entry| is_video(&entry.attachment)) { (16., 9.) } else { (4., 3.) });
        let scale = (available_w / natural_w).min(available_h / natural_h);
        let (display_w, display_h) = ((natural_w * scale).round(), (natural_h * scale).round());
        let src = entry.and_then(|entry| entry.attachment.local_path.clone());
        let copied = self.copied;
        let player = self.player_for(entry, (display_w, display_h), window.scale_factor(), cx);
        let entry = entry.cloned();

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
                .child(match (&src, &player) {
                    (Some(_), Some(player)) => player.clone().into_any_element(),
                    (Some(path), None) if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("gif")) => div().w(px(display_w)).h(px(display_h)).child(crate::gif::gif_image(std::sync::Arc::from(path.as_path()), crate::gif::Fit::Contain, Corners::default())).into_any_element(),
                    (Some(path), None) => crate::attachments::sized_img(path, display_w, display_h, ObjectFit::Contain).into_any_element(),
                    (None, _) => div()
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
                        .child(div().text_size(type_scale::MICRO.font_size).line_height(type_scale::MICRO.line_height).text_color(hsla(0., 0., 1., 0.65)).child(match index {
                            Some(index) if count > 1 => format!("{} of {}, {}", index + 1, count, format_bytes(entry.attachment.bytes)),
                            _ => format_bytes(entry.attachment.bytes),
                        })),
                )
        });
        let arrows = (count > 1).then(|| {
            let arrow = |id: &'static str, icon: IconName, label: &'static str, delta: isize, cx: &mut Context<Self>| {
                div()
                    .absolute()
                    .top(px(0.))
                    .bottom(px(0.))
                    .w(px(MARGIN))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(IconButton::new(id, icon, label).color(white()).hit(px(36.)).size(px(20.)).on_click(cx.listener(move |this, _, _, cx| this.step(delta, cx))))
            };
            [arrow("lightbox-prev", IconName::ChevronLeft, "Previous", -1, cx).left(px(0.)), arrow("lightbox-next", IconName::ChevronRight, "Next", 1, cx).right(px(0.))]
        });

        div()
            .id("lightbox")
            .debug_selector(|| "lightbox".into())
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
                let player = this.player.as_ref().map(|(_, player)| player.clone());
                if key == "c" && event.keystroke.modifiers.secondary() {
                    this.copy(cx);
                } else if key == "left" || key == "k" {
                    this.step(-1, cx);
                } else if key == "right" || key == "j" {
                    this.step(1, cx);
                } else if key == "space" && let Some(player) = player {
                    player.update(cx, |player, cx| player.toggle(cx));
                } else if key == "m" && let Some(player) = player {
                    player.update(cx, |player, cx| player.toggle_mute(cx));
                } else {
                    return;
                }
                cx.stop_propagation();
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close(cx)))
            .child(buttons)
            .children(arrows.into_iter().flatten())
            .children(content)
    }
}
