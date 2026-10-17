//! Every picture and video in the loaded conversation, one at a time, over the dimmed window:
//! the picture as large as the window allows, the title and commands above it, previous and
//! next at its sides. Previous and next wrap; Copy puts the file on the clipboard; Open hands it
//! to the system's app for it. Videos play at once, with play, pause, seek and mute under the
//! picture (Space and M, or a click on the picture); one the platform cannot decode says so
//! and leaves Open.

use messages_core::conversations::conversation_messages;
use messages_core::format::format_bytes;
use std::time::{Duration, Instant};

use messages_core::{Attachment, MessagesStore};
use nd::components::media::{VideoEnded, VideoFailed, VideoProgress, VideoReady, video};
use nd::prelude::*;
use nd::theme::Preset;

use crate::actions::{AttachmentRef, MessageRef};
use crate::thread::Thread;
use crate::ui;

/// The title and command bar, and the room left under the picture when there are no controls.
const BAR: f32 = 64.;
/// The previous and next buttons.
const STEP: f32 = 40.;
const TRANSPORT: f32 = 640.;
const OPEN_GRACE: Duration = Duration::from_millis(600);
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
    opened: Instant,
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
            opened: Instant::now(),
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
        let look = Look::of(&theme);
        let video_entry = entry.as_ref().is_some_and(|e| !e.attachment.mime.starts_with("image/"));
        let picture = match &entry {
            Some(entry) if !video_entry => {
                match entry.attachment.local_path.as_ref().and_then(|p| ui::image_source(&p.to_string_lossy())) {
                    Some(source) => image(source)
                        .id(format!("lightbox-{}", entry.attachment.guid))
                        .alt(entry.attachment.name.clone())
                        .contain()
                        .size_full()
                        .into_any_element(),
                    None => look.notice("Loading\u{2026}").into_any_element(),
                }
            }
            Some(_) if self.player.failed.is_some() => {
                look.notice("This video can\u{2019}t be played here. Open it in another app.").into_any_element()
            }
            Some(entry) => match entry.attachment.local_path.as_ref() {
                Some(path) => self.picture(entry, &path.to_string_lossy(), cx),
                None => look.notice("Loading\u{2026}").into_any_element(),
            },
            None => look.notice("This item is no longer in the conversation.").into_any_element(),
        };
        let (name, detail) =
            entry.as_ref().map(|e| (e.attachment.name.clone(), format_bytes(e.attachment.bytes))).unwrap_or_default();
        let index = entry.as_ref().and_then(|e| entries.iter().position(|x| x.attachment.guid == e.attachment.guid));
        let detail = match index {
            Some(i) if entries.len() > 1 => format!("{detail} \u{b7} {} of {}", i + 1, entries.len()),
            _ => detail,
        };
        let caption = look
            .card(v_flex().px_3().py_1p5().max_w(px(480.)))
            .when(look.centered_title, |c| c.items_center())
            .child(text(name).font_weight(FontWeight::SEMIBOLD).text_color(theme.text.primary).truncate())
            .child(text(detail).text_size(theme.type_scale.caption).text_color(theme.text.secondary));
        let actions = look
            .card(h_flex().p_1().gap_1())
            .child(
                icon_button("lightbox-copy", if self.copied { Icon::Check } else { Icon::Copy })
                    .tooltip(if self.copied { "Copied" } else { "Copy" })
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.copy_lightbox(cx))),
            )
            .child(icon_button("lightbox-open", Icon::ExternalLink).tooltip(look.open_label).on_click(cx.listener(
                |this, _: &ClickEvent, _, cx| {
                    if let Some(reference) = this.lightbox_mut().and_then(|l| l.current()) {
                        this.open_attachment(&reference, cx);
                    }
                },
            )))
            .child(
                icon_button("lightbox-close", Icon::X)
                    .tooltip("Close")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_lightbox(cx))),
            );
        // Fluent leads with the title and ends with the commands, as Photos does; Adwaita
        // centres the title between them, as a header bar does.
        let top = if look.centered_title {
            h_flex()
                .h(px(BAR))
                .px_4()
                .gap_2()
                .items_center()
                .child(div().flex_1())
                .child(caption)
                .child(h_flex().flex_1().justify_end().child(actions))
        } else {
            h_flex().h(px(BAR)).px_4().gap_2().items_center().child(caption).child(div().flex_1()).child(actions)
        };
        let many = entries.len() > 1;
        let step = |id: &'static str, icon: Icon, label: &'static str, delta: isize, cx: &mut Context<Thread>| {
            look.round(icon_button(id, icon).tooltip(label).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.step_lightbox(delta, cx);
                },
            )))
        };
        let middle = h_flex()
            .flex_1()
            .min_h_0()
            .px_4()
            .py_2()
            .gap_3()
            .items_center()
            .child(if many { step("lightbox-prev", Icon::ChevronLeft, "Previous", -1, cx) } else { div().w(px(STEP)) })
            // Out of the flow, so the picture's own size never grows the row past the window.
            .child(div().flex_1().h_full().min_w_0().relative().child(
                div().absolute().top_0().left_0().size_full().flex().items_center().justify_center().child(picture),
            ))
            .child(if many { step("lightbox-next", Icon::ChevronRight, "Next", 1, cx) } else { div().w(px(STEP)) });
        let bottom = match &entry {
            Some(entry) if video_entry && entry.attachment.local_path.is_some() && self.player.failed.is_none() => {
                h_flex().h(px(BAR + 40.)).pb_4().justify_center().items_center().child(self.transport(&look, cx))
            }
            _ => h_flex().h(px(BAR)),
        };
        cover("lightbox")
            .bg(look.scrim)
            .key_context("Lightbox")
            .on_dismiss(cx.listener(|this, _: &Dismissed, _, cx| this.close_lightbox(cx)))
            .child(
                v_flex()
                    .size_full()
                    .animate(Animation::ease_out(FADE))
                    .enter_from(|pose| pose.opacity(0.))
                    .child(top)
                    .child(middle)
                    .child(bottom),
            )
            .into_any_element()
    }

    /// The playing picture; a click on it plays or pauses.
    fn picture(&self, entry: &Entry, path: &str, cx: &mut Context<Thread>) -> AnyElement {
        let player = &self.player;
        let guid = entry.attachment.guid.clone();
        let mut picture = video(path.to_owned())
            .id(format!("lightbox-video-{guid}"))
            .alt(entry.attachment.name.clone())
            .playing(player.playing && !player.ended)
            .muted(player.muted)
            .size_full()
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
        div()
            .id("lightbox-picture")
            .size_full()
            .on_tap(cx.listener(|this, _: &TapEvent, _, cx| {
                // The second click of the double click that opened the viewer lands here.
                if let Some(lightbox) = this.lightbox_mut()
                    && lightbox.opened.elapsed() > OPEN_GRACE
                {
                    lightbox.toggle_play();
                    cx.notify();
                }
            }))
            .child(picture)
            .into_any_element()
    }

    /// The seek bar with its times, play or pause, mute: one floating bar under the picture.
    fn transport(&self, look: &Look, cx: &mut Context<Thread>) -> AnyElement {
        let player = &self.player;
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
        let seek = seek_bar("lightbox-seek", clock)
            .duration(player.duration.map(Duration::from_secs_f64))
            .show_times()
            .w_full()
            .on_seek(cx.listener(|this, seeked: &Seeked, _, cx| {
                if seeked.phase == SeekPhase::Committed
                    && let Some(lightbox) = this.lightbox_mut()
                {
                    lightbox.seek_to(seeked.position.as_secs_f64());
                }
                cx.notify();
            }));
        let play = icon_button("lightbox-play", play_icon).tooltip(play_label).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                if let Some(lightbox) = this.lightbox_mut() {
                    lightbox.toggle_play();
                }
                cx.notify();
            },
        ));
        let mute = icon_button("lightbox-mute", mute_icon).tooltip(mute_label).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                if let Some(lightbox) = this.lightbox_mut() {
                    lightbox.toggle_mute();
                }
                cx.notify();
            },
        ));
        // Two rows, the timeline over the buttons, as the double-row transport controls lay out.
        look.card(v_flex().w(px(TRANSPORT)).px_3().pt_2().pb_1().gap_1())
            .child(seek)
            .child(h_flex().items_center().child(h_flex().flex_1().child(mute)).child(play).child(div().flex_1()))
            .into_any_element()
    }
}

/// How the viewer is drawn on this platform: the dim over the window and the floating surfaces
/// the controls sit on (Fluent's flyout surface, 8 px corners; Adwaita's 12 px and round step
/// buttons).
struct Look {
    scrim: Hsla,
    surface: Hsla,
    border: Hsla,
    text: Hsla,
    radius: Pixels,
    centered_title: bool,
    open_label: &'static str,
}

impl Look {
    fn of(theme: &Theme) -> Self {
        let adwaita = theme.preset == Preset::Adwaita;
        Look {
            scrim: hsla(0., 0., 0., if theme.is_dark() { 0.8 } else { 0.7 }),
            surface: theme.surface.overlay,
            border: theme.border.subtle,
            text: theme.text.secondary,
            radius: if adwaita { px(12.) } else { theme.radius.lg },
            centered_title: adwaita,
            open_label: if adwaita { "Open With\u{2026}" } else { "Open with" },
        }
    }

    fn card(&self, el: Div) -> Div {
        el.bg(self.surface).border_1().border_color(self.border).rounded(self.radius).shadow_md()
    }

    fn round(&self, button: Button) -> Div {
        div()
            .size(px(STEP))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .bg(self.surface)
            .border_1()
            .border_color(self.border)
            .rounded_full()
            .shadow_md()
            .child(button)
    }

    /// A line of text on the dim, where the picture would be.
    fn notice(&self, message: &str) -> Div {
        self.card(div().px_4().py_3()).child(text(message.to_owned()).text_color(self.text))
    }
}
