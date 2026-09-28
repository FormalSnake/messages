//! The player the lightbox shows for a video. Frames arrive from
//! `messages_core::video` already paced, each one becomes a `RenderImage`
//! painted on a canvas, and the previous texture is released as the next
//! lands, so the atlas holds one frame at a time. The poster stays under the
//! picture until the first frame, and the controls (play, time, scrubber,
//! sound) sit on a scrim along the bottom edge.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use image::{Frame, RgbaImage};
use messages_core::MessagesStore;
use messages_core::video::{self, Playback, VideoFrame, VideoInfo};
use tokio::sync::mpsc;

use crate::attachments::{format_duration, media_worker, run_for, sized_img};
use crate::icons::{Icon, IconName};
use crate::primitives::IconButton;
use crate::theme::{radius, spacing, type_scale};

const CONTROLS_HEIGHT: f32 = 44.;
const BIG_BUTTON: f32 = 64.;
/// Frames queued between the pacer and the paint. Two lets one late paint
/// catch up without the pacer stalling.
const FRAME_QUEUE: usize = 2;

/// The probe answered, so the box the lightbox lays out can take the file's own size.
pub struct Probed;

impl EventEmitter<Probed> for VideoPlayer {}

pub struct VideoPlayer {
    store: MessagesStore,
    path: PathBuf,
    guid: String,
    info: Option<VideoInfo>,
    /// ffmpeg is missing, the probe failed or the decoder gave nothing.
    unplayable: bool,
    poster: Option<PathBuf>,
    playback: Option<Playback>,
    frame: Option<Arc<RenderImage>>,
    receiver: Option<Task<()>>,
    /// Seconds into the file: the last frame shown, or where play resumes.
    position: f64,
    playing: bool,
    ended: bool,
    muted: bool,
    /// The box the picture is laid out in and the display scale; frames decode at that size.
    box_size: (f32, f32),
    scale: f32,
    scrubber: Rc<Cell<Bounds<Pixels>>>,
}

impl VideoPlayer {
    pub fn new(store: MessagesStore, path: PathBuf, guid: String, box_size: (f32, f32), scale: f32, cx: &mut Context<Self>) -> Self {
        let mut player = VideoPlayer {
            store,
            path,
            guid,
            info: None,
            unplayable: !video::available(),
            poster: None,
            playback: None,
            frame: None,
            receiver: None,
            position: 0.,
            playing: false,
            ended: false,
            muted: false,
            box_size,
            scale,
            scrubber: Rc::new(Cell::new(Bounds::default())),
        };
        player.load(cx);
        player
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let (path, guid) = (self.path.clone(), self.guid.clone());
        let worker = media_worker().clone();
        run_for(&self.store, async move { worker.video_poster(&path, &guid).await }, cx, |this, poster, cx| {
            this.poster = poster;
            cx.notify();
        });
        if self.unplayable {
            return;
        }
        let path = self.path.clone();
        run_for(&self.store, async move { video::probe(&path).await }, cx, |this, info, cx| match info {
            Some(info) => {
                this.info = Some(info);
                cx.emit(Probed);
                this.start(0., cx);
            }
            None => {
                this.unplayable = true;
                cx.notify();
            }
        });
    }

    pub fn info(&self) -> Option<&VideoInfo> {
        self.info.as_ref()
    }

    /// The box the picture is laid out in this frame. A change only matters
    /// at the next start, so nothing is restarted for it.
    pub fn set_box(&mut self, box_size: (f32, f32), scale: f32) {
        self.box_size = box_size;
        self.scale = scale;
    }

    fn start(&mut self, from: f64, cx: &mut Context<Self>) {
        let Some(info) = &self.info else { return };
        let (width, height) = video::decode_size(info, self.box_size.0, self.box_size.1, self.scale);
        let (tx, mut rx) = mpsc::channel::<VideoFrame>(FRAME_QUEUE);
        self.playback = Some(Playback::start(self.store.runtime(), &self.path, info, width, height, from, self.muted, tx));
        self.position = from;
        self.playing = true;
        self.ended = false;
        self.receiver = Some(cx.spawn(async move |this, cx| {
            while let Some(frame) = rx.recv().await {
                if this.update(cx, |this, cx| this.show(frame, cx)).is_err() {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| this.finished(cx));
        }));
        cx.notify();
    }

    fn show(&mut self, frame: VideoFrame, cx: &mut Context<Self>) {
        let Some(buffer) = RgbaImage::from_raw(frame.width, frame.height, frame.bgra) else { return };
        let image = Arc::new(RenderImage::new(vec![Frame::new(buffer)]));
        if let Some(previous) = self.frame.replace(image) {
            cx.drop_image(previous, None);
        }
        self.position = frame.position;
        cx.notify();
    }

    fn finished(&mut self, cx: &mut Context<Self>) {
        let ended = self.playback.as_ref().is_some_and(|playback| playback.ended());
        if ended && self.frame.is_some() {
            self.ended = true;
            self.playing = false;
            self.position = self.info.as_ref().map_or(self.position, |info| info.duration);
        } else {
            self.unplayable = true;
            self.playing = false;
        }
        self.playback = None;
        cx.notify();
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.ended || (self.playback.is_none() && self.info.is_some()) {
            self.start(0., cx);
            return;
        }
        let Some(playback) = &self.playback else { return };
        if self.playing {
            playback.pause();
        } else {
            playback.resume();
        }
        self.playing = !self.playing;
        cx.notify();
    }

    pub fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        self.muted = !self.muted;
        if let Some(playback) = &self.playback {
            playback.set_muted(self.muted);
        }
        cx.notify();
    }

    fn seek(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let Some(duration) = self.info.as_ref().map(|info| info.duration).filter(|duration| *duration > 0.) else { return };
        let to = (fraction.clamp(0., 1.) as f64 * duration).min(duration - 0.05).max(0.);
        let was_playing = self.playing && !self.ended;
        self.start(to, cx);
        if !was_playing && let Some(playback) = &self.playback {
            playback.pause();
            self.playing = false;
        }
    }

    fn picture(&self, width: f32, height: f32) -> AnyElement {
        let radii = Corners::all(radius::CARD);
        match (&self.frame, &self.poster) {
            (Some(frame), _) => {
                let frame = frame.clone();
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let _ = window.paint_image(bounds, fit_contain(bounds, frame.size(0)), radii, frame, 0, false);
                    },
                )
                .w(px(width))
                .h(px(height))
                .into_any_element()
            }
            (None, Some(poster)) => sized_img(poster, width, height, ObjectFit::Contain).rounded(radius::CARD).into_any_element(),
            (None, None) => div().w(px(width)).h(px(height)).rounded(radius::CARD).bg(hsla(0., 0., 1., 0.08)).into_any_element(),
        }
    }

    fn controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let duration = self.info.as_ref().map_or(0., |info| info.duration);
        let fraction = if duration > 0. { (self.position / duration).clamp(0., 1.) as f32 } else { 0. };
        let time = format!("{} / {}", format_duration(self.position * 1000.), format_duration(duration * 1000.));
        let tabular = FontFeatures(Arc::new(vec![("tnum".into(), 1)]));
        let scrubber = self.scrubber.clone();
        let (play_icon, play_label) = if self.ended {
            (IconName::Replay, "Play again")
        } else if self.playing {
            (IconName::Pause, "Pause")
        } else {
            (IconName::Play, "Play")
        };
        let (sound_icon, sound_label) = if self.muted { (IconName::SoundOff, "Unmute") } else { (IconName::SoundOn, "Mute") };
        div()
            .absolute()
            .bottom(px(0.))
            .left(px(0.))
            .right(px(0.))
            .h(px(CONTROLS_HEIGHT))
            .px(spacing::X2)
            .rounded_b(radius::CARD)
            .bg(hsla(0., 0., 0., 0.55))
            .flex()
            .flex_row()
            .items_center()
            .gap(spacing::X2)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(IconButton::new("video-play", play_icon, play_label).color(white()).on_click(cx.listener(|this, _, _, cx| this.toggle(cx))))
            .child(
                div()
                    .text_size(type_scale::CAPTION.font_size)
                    .line_height(type_scale::CAPTION.line_height)
                    .font_features(tabular)
                    .text_color(white())
                    .flex_shrink_0()
                    .child(time),
            )
            .child(
                div()
                    .id("video-scrubber")
                    .flex_grow(1.)
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            let bounds = this.scrubber.get();
                            if bounds.size.width > px(0.) {
                                this.seek(f32::from((event.position.x - bounds.origin.x) / bounds.size.width), cx);
                            }
                            cx.stop_propagation();
                        }),
                    )
                    .child(
                        div().relative().w_full().h(px(4.)).rounded(radius::PILL).bg(hsla(0., 0., 1., 0.3)).child(div().absolute().left(px(0.)).top(px(0.)).bottom(px(0.)).rounded(radius::PILL).bg(white()).w(relative(fraction))).child(
                            canvas(
                                move |bounds, _, _| scrubber.set(bounds),
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        ),
                    ),
            )
            .child(IconButton::new("video-sound", sound_icon, sound_label).color(white()).on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))))
    }
}

/// The whole picture, centred inside the box.
fn fit_contain(bounds: Bounds<Pixels>, image: Size<DevicePixels>) -> Bounds<Pixels> {
    if image.width.0 <= 0 || image.height.0 <= 0 {
        return bounds;
    }
    let scale = (bounds.size.width / px(image.width.0 as f32)).min(bounds.size.height / px(image.height.0 as f32));
    let size = size(px(image.width.0 as f32 * scale), px(image.height.0 as f32 * scale));
    let origin = point(bounds.origin.x + (bounds.size.width - size.width) / 2., bounds.origin.y + (bounds.size.height - size.height) / 2.);
    Bounds { origin, size }
}

impl Render for VideoPlayer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (width, height) = self.box_size;
        let show_big = !self.playing && !self.unplayable && self.info.is_some();
        let big_icon = if self.ended { IconName::Replay } else { IconName::Play };
        let state = if self.unplayable {
            "video-unplayable"
        } else if self.ended {
            "video-ended"
        } else if self.frame.is_some() && self.playing {
            "video-playing"
        } else if self.frame.is_some() {
            "video-paused"
        } else {
            "video-loading"
        };
        div()
            .id("video-player")
            .debug_selector(|| state.into())
            .relative()
            .w(px(width))
            .h(px(height))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.toggle(cx)))
            .child(self.picture(width, height))
            .when(show_big, |el| {
                el.child(
                    div()
                        .absolute()
                        .top(px(((height - BIG_BUTTON) / 2.).round()))
                        .left(px(((width - BIG_BUTTON) / 2.).round()))
                        .w(px(BIG_BUTTON))
                        .h(px(BIG_BUTTON))
                        .rounded(radius::PILL)
                        .bg(hsla(0., 0., 0., 0.55))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(big_icon).size(px(28.)).color(white()).filled(big_icon == IconName::Play).strong(true)),
                )
            })
            .when(self.unplayable, |el| {
                el.child(
                    div()
                        .absolute()
                        .bottom(px(CONTROLS_HEIGHT + 12.))
                        .left(px(0.))
                        .right(px(0.))
                        .flex()
                        .justify_center()
                        .child(div().px(spacing::X3).py(spacing::X1).rounded(radius::CONTROL).bg(hsla(0., 0., 0., 0.6)).text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(white()).child(
                            if video::available() { "Could not play this video. Open it instead." } else { "Install ffmpeg to play videos here." },
                        )),
                )
            })
            .when(!self.unplayable, |el| el.child(self.controls(cx)))
    }
}
