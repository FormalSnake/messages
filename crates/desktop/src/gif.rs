//! An animated `img()` asks GPUI for a new frame on every paint while the
//! window is active, so a GIF handed to the renderer as a file would redraw
//! the window at the refresh rate. Here a GIF is decoded once per shared
//! (SHA-1) path into one multi-frame
//! `RenderImage`, and a canvas paints whichever frame the file's clock is on.
//! Every copy of the file shares that clock. It advances only while some copy
//! was painted since the last step and the window is active, and each step
//! notifies only the views that painted the file, so nothing on screen means
//! no timer and no frames.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::*;

use crate::stills::lru_victim;

/// Frames asking for less than this are shown for SLOW_FRAME, as browsers do.
const MIN_DELAY: Duration = Duration::from_millis(20);
const SLOW_FRAME: Duration = Duration::from_millis(100);
/// Decoded frames kept across every GIF; the least recently painted go first.
const BUDGET_BYTES: usize = 64 * 1024 * 1024;
/// Decoded frames one file may hold; past this it is painted as a still of its first frame.
const FILE_CAP_BYTES: usize = BUDGET_BYTES;

enum Load {
    Loading,
    Ready(Arc<RenderImage>),
    Failed,
}

struct Entry {
    load: Load,
    delays: Vec<Duration>,
    frame: usize,
    bytes: usize,
    /// Views that painted this file since the clock last stepped.
    viewers: Vec<EntityId>,
    ticking: bool,
    last_painted: Instant,
}

#[derive(Default)]
struct GifRegistry {
    entries: HashMap<Arc<Path>, Entry>,
    bytes: usize,
}

impl Global for GifRegistry {}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Centre crop to the box.
    Cover,
    /// Whole picture, centred inside the box.
    Contain,
}

fn clamp_delay(delay: Duration) -> Duration {
    if delay < MIN_DELAY { SLOW_FRAME } else { delay }
}

fn registry(cx: &mut App) -> &mut GifRegistry {
    if !cx.has_global::<GifRegistry>() {
        cx.set_global(GifRegistry::default());
    }
    cx.global_mut::<GifRegistry>()
}

/// Decodes `path` off the foreground thread. The shared path is the key, so
/// forty copies of one GIF are one decode and one set of textures.
fn start_decode(path: Arc<Path>, cx: &mut App) {
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<Arc<RenderImage>>>();
    let job_path = path.clone();
    let job = move || {
        let bytes = std::fs::read(&*job_path).ok()?;
        let frames = decode_frames(&bytes, FILE_CAP_BYTES)?;
        Some(Arc::new(RenderImage::new(frames)))
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
        cx.update(|cx| finish_decode(path, result, cx));
    })
    .detach();
}

/// Frames in BGRA, decoded one at a time. Every frame is the logical screen
/// size, so the next one's cost is known before it is decoded: once the file
/// would pass `cap` only the first frame is kept and it paints as a still. None
/// when not even one frame fits.
fn decode_frames(bytes: &[u8], cap: usize) -> Option<Vec<image::Frame>> {
    use image::{AnimationDecoder, ImageDecoder};
    let decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).ok()?;
    let (width, height) = decoder.dimensions();
    let per_frame = width as usize * height as usize * 4;
    let mut source = decoder.into_frames();
    let mut frames: Vec<image::Frame> = Vec::new();
    let mut decoded = 0usize;
    loop {
        if decoded.saturating_add(per_frame) > cap {
            frames.truncate(1);
            break;
        }
        let Some(Ok(mut frame)) = source.next() else { break };
        for pixel in frame.buffer_mut().chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        decoded += frame.buffer().len();
        frames.push(frame);
    }
    if frames.is_empty() { None } else { Some(frames) }
}

fn finish_decode(path: Arc<Path>, image: Option<Arc<RenderImage>>, cx: &mut App) {
    let registry = registry(cx);
    let Some(entry) = registry.entries.get_mut(&path) else { return };
    let viewers = std::mem::take(&mut entry.viewers);
    if crate::trace::enabled() {
        crate::trace::log(&format!("gif {} decoded: {} frames, {} viewers", path.display(), image.as_ref().map_or(0, |image| image.frame_count()), viewers.len()));
    }
    match image {
        Some(image) => {
            let count = image.frame_count();
            entry.delays = (0..count).map(|index| clamp_delay(Duration::from(image.delay(index)))).collect();
            entry.bytes = (0..count)
                .map(|index| {
                    let size = image.size(index);
                    size.width.0.max(0) as usize * size.height.0.max(0) as usize * 4
                })
                .sum();
            registry.bytes += entry.bytes;
            entry.load = Load::Ready(image);
        }
        None => entry.load = Load::Failed,
    }
    evict(&path, cx);
    for viewer in viewers {
        cx.notify(viewer);
    }
}

/// Drops the least recently painted GIFs until the budget holds, never `keep`
/// and never one still animating or painted within `IN_USE`.
fn evict(keep: &Path, cx: &mut App) {
    let mut dropped = Vec::new();
    {
        let registry = registry(cx);
        let now = Instant::now();
        while registry.bytes > BUDGET_BYTES {
            let candidates = registry.entries.iter().map(|(path, entry)| (path, entry.last_painted, matches!(entry.load, Load::Ready(_)) && !entry.ticking));
            let Some(oldest) = lru_victim(candidates, &Arc::from(keep), now).cloned() else { break };
            crate::trace::log_if_enabled(&format!("gif {} evicted", oldest.display()));
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

fn schedule_tick(path: Arc<Path>, delay: Duration, cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(delay).await;
        cx.update(|cx| tick(path, cx));
    })
    .detach();
}

fn tick(path: Arc<Path>, cx: &mut App) {
    let registry = registry(cx);
    let Some(entry) = registry.entries.get_mut(&path) else { return };
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
    schedule_tick(path, delay, cx);
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

/// The frame the GIF at `path` is showing, once its frames are decoded, so the
/// tail under it can carry the same picture without decoding it again.
pub fn current_frame(path: &Path, cx: &mut App) -> Option<(Arc<RenderImage>, usize)> {
    let entry = registry(cx).entries.get(path)?;
    match &entry.load {
        Load::Ready(image) => Some((image.clone(), entry.frame)),
        _ => None,
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
            let (image, frame, start) = {
                let registry = registry(cx);
                let fresh = !registry.entries.contains_key(&path);
                let entry = registry.entries.entry(path.clone()).or_insert_with(|| Entry {
                    load: Load::Loading,
                    delays: Vec::new(),
                    frame: 0,
                    bytes: 0,
                    viewers: Vec::new(),
                    ticking: false,
                    last_painted: Instant::now(),
                });
                entry.last_painted = Instant::now();
                match &entry.load {
                    Load::Loading => {
                        if !entry.viewers.contains(&viewer) {
                            entry.viewers.push(viewer);
                        }
                        (None, 0, if fresh { Some(None) } else { None })
                    }
                    Load::Failed => (None, 0, None),
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
                Some(None) => start_decode(path.clone(), cx),
                Some(Some(delay)) => schedule_tick(path.clone(), delay, cx),
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

    fn gif_bytes(colors: &[[u8; 4]]) -> Vec<u8> {
        use image::codecs::gif::GifEncoder;
        let mut bytes = Vec::new();
        let mut encoder = GifEncoder::new(&mut bytes);
        for color in colors {
            encoder.encode_frame(image::Frame::new(image::RgbaImage::from_pixel(2, 2, image::Rgba(*color)))).unwrap();
        }
        drop(encoder);
        bytes
    }

    #[test]
    fn a_gif_under_the_cap_keeps_every_frame_in_bgra() {
        let frames = decode_frames(&gif_bytes(&[[255, 0, 0, 255], [0, 0, 255, 255], [0, 255, 0, 255]]), FILE_CAP_BYTES).unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(&frames[0].buffer().as_raw()[0..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn a_gif_over_the_cap_is_cut_to_its_first_frame() {
        let bytes = gif_bytes(&[[255, 0, 0, 255], [0, 0, 255, 255], [0, 255, 0, 255], [9, 9, 9, 255]]);
        let two_frames = 2 * 2 * 4 * 2;
        let frames = decode_frames(&bytes, two_frames).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(&frames[0].buffer().as_raw()[0..4], &[0, 0, 255, 255]);
        assert!(decode_frames(&bytes, two_frames / 2 - 1).is_none());
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
