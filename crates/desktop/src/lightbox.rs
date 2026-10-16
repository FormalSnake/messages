//! Every picture and video in the loaded conversation, one at a time, in a panel over the
//! window. Previous and next wrap; Copy puts the file on the clipboard; Open hands it to the
//! system viewer. Videos play in the panel with play, pause, seek and mute (Space and M);
//! where the backend has no video surface they fall back to Open.

use messages_core::conversations::conversation_messages;
use messages_core::format::format_bytes;
use std::time::Duration;

use messages_core::{Attachment, MessagesStore};
use nd::components::media::{VideoEnded, VideoFailed, VideoProgress, VideoReady, video};
use nd::prelude::*;

use crate::actions::{AttachmentRef, MessageRef};
use crate::thread::Thread;
use crate::ui;

const PANEL: (f32, f32) = (860., 640.);
const CAPTION: f32 = 48.;
const FADE: Duration = Duration::from_millis(180);

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
    player: Player,
}

/// The video on show: what the app asked of it and what it last reported.
#[derive(Clone, Debug, Default)]
struct Player {
    playing: bool,
    muted: bool,
    ended: bool,
    position: f64,
    duration: Option<f64>,
    /// The last seek asked for, under its request number.
    seek: Option<(u64, f64)>,
    requests: u64,
    failed: Option<String>,
}

impl Lightbox {
    /// The lightbox on `reference`, or `None` when the message is not loaded.
    pub fn open(store: &MessagesStore, reference: &AttachmentRef) -> Option<Self> {
        let chat = {
            let state = store.state();
            let guid = state.selected_chat.clone()?;
            conversation_messages(&state, &guid).iter().any(|m| m.guid == reference.at.message).then_some(guid)?
        };
        Some(Self {
            store: store.clone(),
            chat,
            current: reference.attachment.clone(),
            copied: false,
            player: Player { playing: true, ..Player::default() },
        })
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
        self.player = Player { playing: true, muted: self.player.muted, ..Player::default() };
    }

    /// The current picture or video is not on disk yet.
    pub fn needs_download(&self) -> bool {
        self.entries().iter().any(|e| e.attachment.guid == self.current && e.attachment.local_path.is_none())
    }

    /// Space: play or pause; at the end, play again from the start.
    pub fn toggle_play(&mut self) {
        if self.player.ended {
            self.seek_to(0.);
            self.player.ended = false;
            self.player.playing = true;
        } else {
            self.player.playing = !self.player.playing;
        }
    }

    pub fn toggle_mute(&mut self) {
        self.player.muted = !self.player.muted;
    }

    fn seek_to(&mut self, seconds: f64) {
        self.player.requests += 1;
        self.player.seek = Some((self.player.requests, seconds));
        self.player.position = seconds;
        self.player.ended = false;
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
            Some(_) if self.player.failed.is_some() => {
                label("This video does not play here: choose Open.").secondary().into_any_element()
            }
            Some(entry) => match entry.attachment.local_path.as_ref() {
                Some(path) => self.video(entry, &path.to_string_lossy(), (w, h - 40.), cx),
                None => label("Loading\u{2026}").secondary().into_any_element(),
            },
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
                    .animate(Animation::ease_out(FADE))
                    .enter_from(|pose| pose.opacity(0.))
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

    /// The video with its controls: play or pause, the seek bar with times, mute.
    fn video(&self, entry: &Entry, path: &str, (w, h): (f32, f32), cx: &mut Context<Thread>) -> AnyElement {
        let player = &self.player;
        let guid = entry.attachment.guid.clone();
        let mut picture = video(path.to_owned())
            .id(format!("lightbox-video-{guid}"))
            .alt(entry.attachment.name.clone())
            .playing(player.playing && !player.ended)
            .muted(player.muted)
            .w(px(w))
            .h(px(h))
            .on_ready(cx.listener(|this, ready: &VideoReady, _, cx| {
                if let Some(lightbox) = this.lightbox_mut() {
                    lightbox.player.duration = ready.duration;
                }
                cx.notify();
            }))
            .on_progress(cx.listener(|this, progress: &VideoProgress, _, cx| {
                if let Some(lightbox) = this.lightbox_mut() {
                    lightbox.player.position = progress.position;
                }
                cx.notify();
            }))
            .on_end(cx.listener(|this, _: &VideoEnded, _, cx| {
                if let Some(lightbox) = this.lightbox_mut() {
                    lightbox.player.ended = true;
                    lightbox.player.position = lightbox.player.duration.unwrap_or(lightbox.player.position);
                }
                cx.notify();
            }))
            .on_error(cx.listener(|this, failed: &VideoFailed, _, cx| {
                if let Some(lightbox) = this.lightbox_mut() {
                    lightbox.player.failed = Some(failed.error.clone());
                }
                cx.notify();
            }));
        if let Some((request, seconds)) = player.seek {
            picture = picture.seek(request, seconds);
        }
        let position = Duration::from_secs_f64(player.position.max(0.));
        let clock = if player.playing && !player.ended {
            PlaybackClock::playing(position)
        } else {
            PlaybackClock::paused(position)
        };
        let (play_icon, play_label) = if player.ended {
            (Icon::RotateCw, "Play Again")
        } else if player.playing {
            (Icon::Pause, "Pause")
        } else {
            (Icon::Play, "Play")
        };
        let (mute_icon, mute_label) = if player.muted { (Icon::VolumeX, "Unmute") } else { (Icon::Volume2, "Mute") };
        v_flex()
            .gap_2()
            .items_center()
            .child(picture)
            .child(
                h_flex()
                    .w(px(w))
                    .gap_2()
                    .items_center()
                    .child(icon_button("lightbox-play", play_icon).tooltip(play_label).on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            if let Some(lightbox) = this.lightbox_mut() {
                                lightbox.toggle_play();
                            }
                            cx.notify();
                        },
                    )))
                    .child(
                        seek_bar("lightbox-seek", clock)
                            .duration(player.duration.map(Duration::from_secs_f64))
                            .show_times()
                            .flex_1()
                            .on_seek(cx.listener(|this, seeked: &Seeked, _, cx| {
                                if seeked.phase == SeekPhase::Committed
                                    && let Some(lightbox) = this.lightbox_mut()
                                {
                                    lightbox.seek_to(seeked.position.as_secs_f64());
                                }
                                cx.notify();
                            })),
                    )
                    .child(icon_button("lightbox-mute", mute_icon).tooltip(mute_label).on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            if let Some(lightbox) = this.lightbox_mut() {
                                lightbox.toggle_mute();
                            }
                            cx.notify();
                        },
                    ))),
            )
            .into_any_element()
    }
}
