//! Port of `apps/desktop/src/ui/gif.tsx`. An animated `img()` asks GPUI for a
//! new frame on every paint while the window is active, so a GIF handed to
//! the renderer as a file would redraw the window at the refresh rate. Here a
//! GIF is decoded once per shared (SHA-1) path into one multi-frame
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

/// Frames asking for less than this are shown for SLOW_FRAME, as browsers do.
const MIN_DELAY: Duration = Duration::from_millis(20);
const SLOW_FRAME: Duration = Duration::from_millis(100);
/// Decoded frames kept across every GIF; the least recently painted go first.
const BUDGET_BYTES: usize = 64 * 1024 * 1024;

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
    /// The box already has the picture's aspect.
    Fill,
    /// Centre crop to the box.
    Cover,
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
    let renderer = cx.svg_renderer();
    let job_path = path.clone();
    let job = move || {
        let bytes = std::fs::read(&*job_path).ok()?;
        Image::from_bytes(ImageFormat::Gif, bytes).to_image_data(renderer).ok()
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

/// Drops the least recently painted GIFs until the budget holds, never `keep`.
fn evict(keep: &Path, cx: &mut App) {
    let mut dropped = Vec::new();
    {
        let registry = registry(cx);
        while registry.bytes > BUDGET_BYTES {
            let oldest = registry
                .entries
                .iter()
                .filter(|(path, entry)| &***path != keep && matches!(entry.load, Load::Ready(_)))
                .min_by_key(|(_, entry)| entry.last_painted)
                .map(|(path, _)| path.clone());
            let Some(oldest) = oldest else { break };
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
    if fit == Fit::Fill || image.width.0 <= 0 || image.height.0 <= 0 {
        return bounds;
    }
    let scale = (bounds.size.width / px(image.width.0 as f32)).max(bounds.size.height / px(image.height.0 as f32));
    let size = size(px(image.width.0 as f32 * scale), px(image.height.0 as f32 * scale));
    let origin = point(bounds.origin.x + (bounds.size.width - size.width) / 2., bounds.origin.y + (bounds.size.height - size.height) / 2.);
    Bounds { origin, size }
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

    #[test]
    fn cover_centres_and_crops_to_the_box() {
        let bounds = Bounds { origin: point(px(0.), px(0.)), size: size(px(100.), px(100.)) };
        let fit = fitted(bounds, size(DevicePixels(200), DevicePixels(100)), Fit::Cover);
        assert_eq!(fit.size, size(px(200.), px(100.)));
        assert_eq!(fit.origin, point(px(-50.), px(0.)));
        assert_eq!(fitted(bounds, size(DevicePixels(200), DevicePixels(100)), Fit::Fill), bounds);
    }
}
