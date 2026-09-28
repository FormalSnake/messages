//! An animated `img()` asks GPUI for a new frame on every paint while the
//! window is active, so a GIF handed to the renderer as a file would redraw
//! the window at the refresh rate. Here a GIF is decoded once per shared
//! (SHA-1) path and box size into one multi-frame `RenderImage`, and a canvas
//! paints whichever frame that entry's clock is on. Every copy of the file at
//! that size shares the clock. It advances only while some copy was painted
//! since the last step and the window is active, and each step notifies only
//! the views that painted the file, so nothing on screen means no timer and
//! no frames.
//!
//! Frames are decoded at the size they are shown (device pixels, rounded up
//! to a 64 px step), never larger than the file, so a 480 px GIF in a 160 px
//! tile costs a ninth of the memory. A file that would still pass
//! FILE_CAP_BYTES keeps every other frame with the delays merged, as many
//! times as it takes, so it always animates. Decodes run three at a time:
//! what is being painted goes to the front of the queue, what the thread
//! warms ahead of a scroll to the back, and a decode that failed (a file
//! mid-download) is tried again a few seconds later.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::*;
use image::imageops::FilterType;

use crate::stills::lru_victim;

/// Frames asking for less than this are shown for SLOW_FRAME, as browsers do.
const MIN_DELAY: Duration = Duration::from_millis(20);
const SLOW_FRAME: Duration = Duration::from_millis(100);
/// Decoded frames kept across every GIF; the least recently painted go first.
const BUDGET_BYTES: usize = 256 * 1024 * 1024;
/// Decoded frames one entry may hold; past this every other frame is dropped.
const FILE_CAP_BYTES: usize = 48 * 1024 * 1024;
/// Box sizes round up to this many device pixels, so a tail and its block, or
/// two rows a pixel apart, share one decode.
const EDGE_STEP: u32 = 64;
const DECODE_SLOTS: usize = 3;
const RETRY_AFTER: Duration = Duration::from_secs(3);
const MAX_ATTEMPTS: u32 = 3;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Key {
    path: Arc<Path>,
    /// Device pixels along the longer side of the box, an EDGE_STEP multiple.
    edge: u32,
}

enum Load {
    Queued,
    Loading,
    Ready(Arc<RenderImage>),
    Failed { at: Instant, attempts: u32 },
}

struct Entry {
    load: Load,
    delays: Vec<Duration>,
    frame: usize,
    bytes: usize,
    attempts: u32,
    /// Views that painted this entry since the clock last stepped.
    viewers: Vec<EntityId>,
    ticking: bool,
    last_painted: Instant,
}

impl Entry {
    fn new(load: Load) -> Self {
        Entry { load, delays: Vec::new(), frame: 0, bytes: 0, attempts: 0, viewers: Vec::new(), ticking: false, last_painted: Instant::now() }
    }
}

#[derive(Default)]
struct GifRegistry {
    entries: HashMap<Key, Entry>,
    bytes: usize,
    decoding: usize,
    queue: VecDeque<Key>,
    /// The display scale the last paint saw, for warms that have no window.
    scale: Option<f32>,
}

impl Global for GifRegistry {}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Centre crop to the box.
    Cover,
    /// Whole picture, centred inside the box.
    Contain,
}

/// What a decode produced: BGRA frames at the decode size and their delays.
struct Decoded {
    frames: Vec<image::Frame>,
    delays: Vec<Duration>,
}

fn clamp_delay(delay: Duration) -> Duration {
    if delay < MIN_DELAY { SLOW_FRAME } else { delay }
}

fn edge_for(width: f32, height: f32, scale: f32) -> u32 {
    let device = (width.max(height) * scale.max(0.5)).ceil().max(1.) as u32;
    device.div_ceil(EDGE_STEP) * EDGE_STEP
}

fn registry(cx: &mut App) -> &mut GifRegistry {
    if !cx.has_global::<GifRegistry>() {
        cx.set_global(GifRegistry::default());
    }
    cx.global_mut::<GifRegistry>()
}

/// Asks for `key` to be decoded: a new entry, or a failed one whose retry is
/// due. `urgent` puts it ahead of the warms.
fn request(key: Key, urgent: bool, cx: &mut App) {
    let registry = registry(cx);
    let queue = match registry.entries.get_mut(&key) {
        None => {
            registry.entries.insert(key.clone(), Entry::new(Load::Queued));
            true
        }
        Some(entry) => match entry.load {
            Load::Failed { at, attempts } if attempts < MAX_ATTEMPTS && at.elapsed() >= RETRY_AFTER => {
                entry.load = Load::Queued;
                true
            }
            Load::Queued if urgent => {
                registry.queue.retain(|queued| *queued != key);
                true
            }
            _ => false,
        },
    };
    if queue {
        if urgent {
            registry.queue.push_front(key);
        } else {
            registry.queue.push_back(key);
        }
    }
    pump(cx);
}

/// Starts decodes while there are slots and something queued.
fn pump(cx: &mut App) {
    loop {
        let registry = registry(cx);
        if registry.decoding >= DECODE_SLOTS {
            return;
        }
        let Some(key) = registry.queue.pop_front() else { return };
        let Some(entry) = registry.entries.get_mut(&key) else { continue };
        if !matches!(entry.load, Load::Queued) {
            continue;
        }
        entry.load = Load::Loading;
        entry.attempts += 1;
        registry.decoding += 1;
        start_decode(key, cx);
    }
}

/// Decodes off the foreground thread, on the store's runtime when there is
/// one (the blocking pool, so the two workers stay free for the network).
fn start_decode(key: Key, cx: &mut App) {
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<Decoded>>();
    let (job_path, edge) = (key.path.clone(), key.edge);
    let job = move || {
        let bytes = std::fs::read(&*job_path).ok()?;
        decode_frames(&bytes, edge, FILE_CAP_BYTES)
    };
    match cx.try_global::<crate::bridge::StoreHandle>().and_then(|handle| handle.0.clone()) {
        Some(store) => store.spawn(async move {
            let result = tokio::task::spawn_blocking(job).await.ok().flatten();
            let _ = tx.send(result);
        }),
        None => cx
            .background_spawn(async move {
                let _ = tx.send(job());
            })
            .detach(),
    }
    cx.spawn(async move |cx| {
        let result = rx.await.ok().flatten();
        cx.update(|cx| finish_decode(key, result, cx));
    })
    .detach();
}

/// Frames in BGRA at most `edge` device pixels along the longer side, decoded
/// one at a time. Every frame is the logical screen size, so the cost is known
/// up front: once the entry would pass `cap`, every other frame is dropped
/// and its delay folded into the one before, again if it comes to that. None
/// for a file with no frame at all.
fn decode_frames(bytes: &[u8], edge: u32, cap: usize) -> Option<Decoded> {
    use image::{AnimationDecoder, ImageDecoder};
    let decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).ok()?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let factor = (edge as f32 / width.max(height) as f32).min(1.);
    let (target_w, target_h) = (((width as f32 * factor).round() as u32).max(1), ((height as f32 * factor).round() as u32).max(1));
    let per_frame = target_w as usize * target_h as usize * 4;
    let mut frames: Vec<image::Frame> = Vec::new();
    let mut delays: Vec<Duration> = Vec::new();
    // Source frames kept are the multiples of `stride`; the rest lend their delay to the last kept.
    let mut stride: usize = 1;
    for (index, frame) in decoder.into_frames().enumerate() {
        let Ok(frame) = frame else { break };
        let (numer, denom) = frame.delay().numer_denom_ms();
        let delay = clamp_delay(if denom == 0 { SLOW_FRAME } else { Duration::from_millis((numer / denom).max(1) as u64) });
        if index % stride != 0 {
            if let Some(last) = delays.last_mut() {
                *last += delay;
            }
            continue;
        }
        if frames.len() * per_frame + per_frame > cap && !frames.is_empty() {
            thin(&mut frames, &mut delays);
            stride *= 2;
            if index % stride != 0 {
                if let Some(last) = delays.last_mut() {
                    *last += delay;
                }
                continue;
            }
        }
        let mut buffer = frame.into_buffer();
        if factor < 1. {
            buffer = image::imageops::resize(&buffer, target_w, target_h, FilterType::Triangle);
        }
        for pixel in buffer.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        frames.push(image::Frame::from_parts(buffer, 0, 0, image::Delay::from_saturating_duration(delay)));
        delays.push(delay);
    }
    if frames.is_empty() { None } else { Some(Decoded { frames, delays }) }
}

/// Keeps every other frame, each taking the delay of the one dropped after it.
fn thin(frames: &mut Vec<image::Frame>, delays: &mut Vec<Duration>) {
    let mut kept_frames = Vec::with_capacity(frames.len().div_ceil(2));
    let mut kept_delays = Vec::with_capacity(delays.len().div_ceil(2));
    for (index, (frame, delay)) in frames.drain(..).zip(delays.drain(..)).enumerate() {
        if index % 2 == 0 {
            kept_frames.push(frame);
            kept_delays.push(delay);
        } else if let Some(last) = kept_delays.last_mut() {
            *last += delay;
        }
    }
    *frames = kept_frames;
    *delays = kept_delays;
}

fn finish_decode(key: Key, decoded: Option<Decoded>, cx: &mut App) {
    let registry = registry(cx);
    registry.decoding = registry.decoding.saturating_sub(1);
    let Some(entry) = registry.entries.get_mut(&key) else {
        pump(cx);
        return;
    };
    let viewers = std::mem::take(&mut entry.viewers);
    if crate::trace::enabled() {
        crate::trace::log(&format!("gif {} @{} decoded: {} frames, {} viewers", key.path.display(), key.edge, decoded.as_ref().map_or(0, |decoded| decoded.frames.len()), viewers.len()));
    }
    match decoded {
        Some(Decoded { frames, delays }) => {
            entry.bytes = frames.iter().map(|frame| frame.buffer().len()).sum();
            entry.delays = delays;
            entry.frame = 0;
            registry.bytes += entry.bytes;
            entry.load = Load::Ready(Arc::new(RenderImage::new(frames)));
        }
        None => entry.load = Load::Failed { at: Instant::now(), attempts: entry.attempts },
    }
    evict(&key, cx);
    for viewer in viewers {
        cx.notify(viewer);
    }
    pump(cx);
}

/// Drops the least recently painted entries until the budget holds, never
/// `keep`, never one animating or painted since its last step.
fn evict(keep: &Key, cx: &mut App) {
    let mut dropped = Vec::new();
    {
        let registry = registry(cx);
        let now = Instant::now();
        while registry.bytes > BUDGET_BYTES {
            let candidates = registry.entries.iter().map(|(key, entry)| (key, entry.last_painted, matches!(entry.load, Load::Ready(_)) && !entry.ticking && entry.viewers.is_empty()));
            let Some(oldest) = lru_victim(candidates, keep, now).cloned() else { break };
            crate::trace::log_if_enabled(&format!("gif {} @{} evicted", oldest.path.display(), oldest.edge));
            if let Some(entry) = registry.entries.remove(&oldest) {
                registry.bytes = registry.bytes.saturating_sub(entry.bytes);
                if let Load::Ready(image) = entry.load {
                    dropped.push(image);
                }
            }
        }
    }
    for image in dropped {
        cx.drop_image(image, None);
    }
}

fn schedule_tick(key: Key, delay: Duration, cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(delay).await;
        cx.update(|cx| tick(key, cx));
    })
    .detach();
}

fn tick(key: Key, cx: &mut App) {
    let registry = registry(cx);
    let Some(entry) = registry.entries.get_mut(&key) else { return };
    let Load::Ready(image) = &entry.load else {
        entry.ticking = false;
        return;
    };
    // Nobody painted it since the last step: off screen, scrolled away or the
    // window went inactive. The clock stops until a paint starts it again.
    if entry.viewers.is_empty() {
        entry.ticking = false;
        crate::trace::log_if_enabled("gif clock stopped: nothing painted it since the last frame");
        return;
    }
    entry.frame = (entry.frame + 1) % image.frame_count().max(1);
    let delay = entry.delays.get(entry.frame).copied().unwrap_or(SLOW_FRAME);
    let viewers = std::mem::take(&mut entry.viewers);
    for viewer in viewers {
        cx.notify(viewer);
    }
    schedule_tick(key, delay, cx);
}

fn fitted(bounds: Bounds<Pixels>, image: Size<DevicePixels>, fit: Fit) -> Bounds<Pixels> {
    if image.width.0 <= 0 || image.height.0 <= 0 {
        return bounds;
    }
    let (sx, sy) = (bounds.size.width / px(image.width.0 as f32), bounds.size.height / px(image.height.0 as f32));
    let scale = if fit == Fit::Cover { sx.max(sy) } else { sx.min(sy) };
    let size = size(px(image.width.0 as f32 * scale), px(image.height.0 as f32 * scale));
    let origin = point(bounds.origin.x + (bounds.size.width - size.width) / 2., bounds.origin.y + (bounds.size.height - size.height) / 2.);
    Bounds { origin, size }
}

/// The frame the GIF at `path` is showing in a `width` by `height` box, once
/// its frames are decoded, so the tail under it can carry the same picture
/// without decoding it again.
pub fn current_frame(path: &Path, width: f32, height: f32, scale: f32, cx: &mut App) -> Option<(Arc<RenderImage>, usize)> {
    let key = Key { path: Arc::from(path), edge: edge_for(width, height, scale) };
    let entry = registry(cx).entries.get(&key)?;
    match &entry.load {
        Load::Ready(image) => Some((image.clone(), entry.frame)),
        _ => None,
    }
}

/// Decodes `path` for a `width` by `height` box ahead of its first paint, behind
/// whatever is being painted now. Nothing happens for an entry that exists.
pub fn warm(path: Arc<Path>, width: f32, height: f32, cx: &mut App) {
    let scale = registry(cx).scale.unwrap_or(1.);
    let key = Key { path, edge: edge_for(width, height, scale) };
    if registry(cx).entries.contains_key(&key) {
        return;
    }
    request(key, false, cx);
}

/// Forgets warms that have not started: the thread they were for is gone.
pub fn drop_pending_warms(cx: &mut App) {
    let registry = registry(cx);
    let queued: Vec<Key> = registry.queue.drain(..).collect();
    for key in queued {
        if registry.entries.get(&key).is_some_and(|entry| matches!(entry.load, Load::Queued) && entry.viewers.is_empty()) {
            registry.entries.remove(&key);
        } else {
            registry.queue.push_back(key);
        }
    }
}

/// Paints the current frame of the GIF at `path` into its box. The box shows
/// whatever is behind it until the frames are decoded, so there is no blank
/// first loop and no flash of the file animating by itself.
pub fn gif_image(path: Arc<Path>, fit: Fit, radii: Corners<Pixels>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let active = window.is_window_active();
            let viewer = window.current_view();
            let scale = window.scale_factor();
            let key = Key { path, edge: edge_for(f32::from(bounds.size.width), f32::from(bounds.size.height), scale) };
            registry(cx).scale = Some(scale);
            let (image, frame, start) = {
                let registry = registry(cx);
                let entry = registry.entries.entry(key.clone()).or_insert_with(|| Entry::new(Load::Queued));
                entry.last_painted = Instant::now();
                match &entry.load {
                    Load::Queued | Load::Loading | Load::Failed { .. } => {
                        if !entry.viewers.contains(&viewer) {
                            entry.viewers.push(viewer);
                        }
                        (None, 0, Some(None))
                    }
                    Load::Ready(image) => {
                        let image = image.clone();
                        let animated = image.frame_count() > 1;
                        if animated && active && !entry.viewers.contains(&viewer) {
                            entry.viewers.push(viewer);
                        }
                        let start = if animated && active && !entry.ticking {
                            entry.ticking = true;
                            Some(Some(entry.delays.get(entry.frame).copied().unwrap_or(SLOW_FRAME)))
                        } else {
                            None
                        };
                        (Some(image), entry.frame, start)
                    }
                }
            };
            match start {
                Some(None) => request(key, true, cx),
                Some(Some(delay)) => schedule_tick(key, delay, cx),
                None => {}
            }
            if let Some(image) = image {
                let image_bounds = fitted(bounds, image.size(frame), fit);
                let _ = window.paint_image(bounds, image_bounds, radii, image, frame, false);
            }
        },
    )
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::*;
    // gpui_kit's glob exports its own `test` attribute, which would shadow the std one.
    use std::prelude::v1::test;

    #[test]
    fn delays_under_twenty_ms_are_shown_for_a_tenth_of_a_second() {
        assert_eq!(clamp_delay(Duration::from_millis(0)), SLOW_FRAME);
        assert_eq!(clamp_delay(Duration::from_millis(10)), SLOW_FRAME);
        assert_eq!(clamp_delay(Duration::from_millis(20)), Duration::from_millis(20));
        assert_eq!(clamp_delay(Duration::from_millis(70)), Duration::from_millis(70));
    }

    #[test]
    fn a_box_rounds_up_to_the_step_in_device_pixels() {
        assert_eq!(edge_for(320., 240., 1.), 320);
        assert_eq!(edge_for(320., 240., 2.), 640);
        assert_eq!(edge_for(150., 150., 1.), 192);
        assert_eq!(edge_for(1., 1., 1.), 64);
    }

    fn gif_bytes(side: u32, colors: &[[u8; 4]]) -> Vec<u8> {
        use image::codecs::gif::GifEncoder;
        let mut bytes = Vec::new();
        let mut encoder = GifEncoder::new(&mut bytes);
        for color in colors {
            let frame = image::Frame::from_parts(image::RgbaImage::from_pixel(side, side, image::Rgba(*color)), 0, 0, image::Delay::from_numer_denom_ms(100, 1));
            encoder.encode_frame(frame).unwrap();
        }
        drop(encoder);
        bytes
    }

    #[test]
    fn a_gif_under_the_cap_keeps_every_frame_in_bgra_with_its_delay() {
        let decoded = decode_frames(&gif_bytes(2, &[[255, 0, 0, 255], [0, 0, 255, 255], [0, 255, 0, 255]]), 64, FILE_CAP_BYTES).unwrap();
        assert_eq!(decoded.frames.len(), 3);
        assert_eq!(&decoded.frames[0].buffer().as_raw()[0..4], &[0, 0, 255, 255]);
        assert_eq!(decoded.delays, vec![Duration::from_millis(100); 3]);
    }

    #[test]
    fn a_gif_over_the_cap_keeps_every_other_frame_and_folds_the_delays() {
        let bytes = gif_bytes(2, &[[255, 0, 0, 255], [0, 0, 255, 255], [0, 255, 0, 255], [9, 9, 9, 255], [1, 2, 3, 255]]);
        let two_frames = 2 * 2 * 4 * 2;
        // Five frames into two: thinned once at the third, again at the fifth, the half second intact.
        let decoded = decode_frames(&bytes, 64, two_frames).unwrap();
        assert_eq!(decoded.frames.len(), 2);
        assert_eq!(&decoded.frames[0].buffer().as_raw()[0..4], &[0, 0, 255, 255]);
        assert_eq!(&decoded.frames[1].buffer().as_raw()[0..4], &[3, 2, 1, 255]);
        assert_eq!(decoded.delays, vec![Duration::from_millis(400), Duration::from_millis(100)]);
    }

    #[test]
    fn a_gif_larger_than_its_box_is_decoded_at_the_box() {
        let decoded = decode_frames(&gif_bytes(128, &[[255, 0, 0, 255]]), 64, FILE_CAP_BYTES).unwrap();
        assert_eq!(decoded.frames[0].buffer().dimensions(), (64, 64));
        let small = decode_frames(&gif_bytes(16, &[[255, 0, 0, 255]]), 64, FILE_CAP_BYTES).unwrap();
        assert_eq!(small.frames[0].buffer().dimensions(), (16, 16));
    }

    #[test]
    fn cover_crops_and_contain_letterboxes_around_the_centre() {
        let bounds = Bounds { origin: point(px(0.), px(0.)), size: size(px(100.), px(100.)) };
        let fit = fitted(bounds, size(DevicePixels(200), DevicePixels(100)), Fit::Cover);
        assert_eq!(fit.size, size(px(200.), px(100.)));
        assert_eq!(fit.origin, point(px(-50.), px(0.)));
        let contain = fitted(bounds, size(DevicePixels(200), DevicePixels(100)), Fit::Contain);
        assert_eq!(contain.size, size(px(100.), px(50.)));
        assert_eq!(contain.origin, point(px(0.), px(25.)));
    }
}
