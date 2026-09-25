//! Stills decoded at the size they are shown. GPUI samples a texture without
//! mipmaps, so a 240 px avatar SVG or a 4032 px photo drawn into a 32 px box
//! aliases (the demo avatars' initials came out as garbage) and keeps a
//! full-size texture alive for a thumbnail. Here each (source, device size)
//! pair is decoded once off the foreground thread, box-filtered down to the
//! box, and kept in a byte-capped cache that every view shares.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use gpui_kit::*;
use image::imageops::FilterType;
use image::{DynamicImage, Frame, ImageDecoder as _, ImageReader, RgbaImage};

use crate::attachments::decode_data_url;

/// Decoded bytes kept across every sized still; least recently used go first.
const BUDGET_BYTES: usize = 48 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    source: Arc<str>,
    width: u32,
    height: u32,
    cover: bool,
}

enum Slot {
    Loading(Vec<EntityId>),
    Ready(Arc<RenderImage>, usize),
    Failed,
}

struct Entry {
    slot: Slot,
    used: Instant,
}

#[derive(Default)]
struct Stills {
    entries: HashMap<Key, Entry>,
    bytes: usize,
}

impl Global for Stills {}

fn stills(cx: &mut App) -> &mut Stills {
    if !cx.has_global::<Stills>() {
        cx.set_global(Stills::default());
    }
    cx.global_mut::<Stills>()
}

/// `img()` source for `path` (a file or a fixture's data URL) drawn into a
/// `width` x `height` box with `fit`. Falls back to the full-size decode when
/// the file is something the `image` crate cannot read.
pub fn sized_image_source(path: &Path, width: Pixels, height: Pixels, fit: ObjectFit) -> ImageSource {
    let source: Arc<str> = Arc::from(path.to_string_lossy().as_ref());
    let cover = matches!(fit, ObjectFit::Cover | ObjectFit::Fill);
    ImageSource::Custom(Arc::new(move |window, cx| {
        let scale = window.scale_factor();
        let key = Key {
            source: source.clone(),
            width: (f32::from(width) * scale).round().max(1.) as u32,
            height: (f32::from(height) * scale).round().max(1.) as u32,
            cover,
        };
        let viewer = window.current_view();
        let (image, start, failed) = {
            let stills = stills(cx);
            let entry = stills.entries.entry(key.clone()).or_insert_with(|| Entry { slot: Slot::Loading(Vec::new()), used: Instant::now() });
            entry.used = Instant::now();
            match &mut entry.slot {
                Slot::Ready(image, _) => (Some(image.clone()), false, false),
                Slot::Failed => (None, false, true),
                Slot::Loading(viewers) => {
                    let start = viewers.is_empty();
                    if !viewers.contains(&viewer) {
                        viewers.push(viewer);
                    }
                    (None, start, false)
                }
            }
        };
        if let Some(image) = image {
            return Some(Ok(image));
        }
        if failed {
            return full_size(&source, window, cx);
        }
        if start {
            start_decode(key, cx);
        }
        None
    }))
}

/// What GPUI's own loader makes of the source, for formats the sized path cannot read.
fn full_size(source: &str, window: &mut Window, cx: &mut App) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
    crate::attachments::render_image(Path::new(source), window, cx).map(Ok)
}

fn start_decode(key: Key, cx: &mut App) {
    let renderer = cx.svg_renderer();
    let job_key = key.clone();
    let job = move || decode(&job_key, &renderer);
    let task = cx.background_spawn(async move { job() });
    cx.spawn(async move |cx| {
        let image = task.await;
        cx.update(|cx| finish(key, image, cx));
    })
    .detach();
}

fn finish(key: Key, image: Option<RenderImage>, cx: &mut App) {
    let viewers = {
        let stills = stills(cx);
        let Some(entry) = stills.entries.get_mut(&key) else { return };
        let slot = match image {
            Some(image) => {
                let bytes = image.as_bytes(0).map_or(0, <[u8]>::len);
                stills.bytes += bytes;
                Slot::Ready(Arc::new(image), bytes)
            }
            None => Slot::Failed,
        };
        match std::mem::replace(&mut entry.slot, slot) {
            Slot::Loading(viewers) => viewers,
            _ => Vec::new(),
        }
    };
    evict(&key, cx);
    for viewer in viewers {
        cx.notify(viewer);
    }
}

fn evict(keep: &Key, cx: &mut App) {
    let mut dropped = Vec::new();
    {
        let stills = stills(cx);
        while stills.bytes > BUDGET_BYTES {
            let oldest = stills
                .entries
                .iter()
                .filter(|(key, entry)| *key != keep && matches!(entry.slot, Slot::Ready(..)))
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone());
            let Some(oldest) = oldest else { break };
            if let Some(Entry { slot: Slot::Ready(image, bytes), .. }) = stills.entries.remove(&oldest) {
                stills.bytes = stills.bytes.saturating_sub(bytes);
                dropped.push(image);
            }
        }
    }
    for image in dropped {
        cx.drop_image(image, None);
    }
}

fn decode(key: &Key, renderer: &SvgRenderer) -> Option<RenderImage> {
    let (bytes, svg) = if key.source.starts_with("data:") {
        let (format, bytes) = decode_data_url(&key.source)?;
        (bytes, format == ImageFormat::Svg)
    } else {
        let bytes = std::fs::read(&*key.source).ok()?;
        let svg = key.source.ends_with(".svg");
        (bytes, svg)
    };
    let rgba = if svg { rasterize_svg(&bytes, renderer)? } else { decode_raster(&bytes)? };
    let full = rgba.dimensions();
    let mut scaled = fit(rgba, key.width, key.height, key.cover);
    if crate::trace::enabled() {
        let kb = |(w, h): (u32, u32)| w as usize * h as usize * 4 / 1024;
        crate::trace::log(&format!("still {}x{} -> {}x{}: {} KB instead of {} KB", full.0, full.1, scaled.width(), scaled.height(), kb(scaled.dimensions()), kb(full)));
    }
    if !svg {
        // The SVG path already hands back BGRA; `image` decodes RGBA.
        for pixel in scaled.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
    }
    Some(RenderImage::new(vec![Frame::new(scaled)]))
}

/// The SVG at twice its own size, in GPUI's BGRA, as the input to `fit`.
fn rasterize_svg(bytes: &[u8], renderer: &SvgRenderer) -> Option<RgbaImage> {
    let bytes = with_concrete_sans(bytes);
    let image = renderer.render_single_frame(&bytes, 1.0).ok()?;
    let size = image.size(0);
    RgbaImage::from_raw(size.width.0 as u32, size.height.0 as u32, image.as_bytes(0)?.to_vec())
}

/// GPUI's SVG font database maps `sans-serif` to Arial, then to a bundled
/// IBM Plex Sans this app does not ship; with neither installed (most Linux
/// systems) usvg takes the database's first face, which is how the demo
/// avatars' initials came out as garbage. Naming the UI font first fixes it.
fn with_concrete_sans(bytes: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    let Ok(text) = std::str::from_utf8(bytes) else { return bytes.into() };
    if !text.contains("sans-serif") {
        return bytes.into();
    }
    let family = format!("{}, sans-serif", crate::theme::font_sans());
    text.replace("font-family=\"sans-serif\"", &format!("font-family=\"{family}\"")).replace("font-family='sans-serif'", &format!("font-family='{family}'")).into_bytes().into()
}

fn decode_raster(bytes: &[u8]) -> Option<RgbaImage> {
    let mut decoder = ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?.into_decoder().ok()?;
    let orientation = decoder.orientation().ok()?;
    let mut image = DynamicImage::from_decoder(decoder).ok()?;
    image.apply_orientation(orientation);
    Some(image.into_rgba8())
}

/// Scales `image` so it covers (or fits inside) `width` x `height`, never up.
fn fit(image: RgbaImage, width: u32, height: u32, cover: bool) -> RgbaImage {
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 {
        return image;
    }
    let (sx, sy) = (width as f32 / w as f32, height as f32 / h as f32);
    let scale = if cover { sx.max(sy) } else { sx.min(sy) };
    if scale >= 1. {
        return image;
    }
    let (tw, th) = (((w as f32 * scale).round() as u32).max(1), ((h as f32 * scale).round() as u32).max(1));
    image::imageops::resize(&image, tw, th, FilterType::Triangle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    #[test]
    fn cover_fills_the_box_and_contain_fits_inside_it() {
        let wide = RgbaImage::new(400, 200);
        assert_eq!(fit(wide.clone(), 64, 64, true).dimensions(), (128, 64));
        assert_eq!(fit(wide, 64, 64, false).dimensions(), (64, 32));
    }

    #[test]
    fn a_generic_sans_serif_names_the_ui_font_first() {
        let out = with_concrete_sans(br#"<text font-family="sans-serif">AR</text>"#);
        let out = std::str::from_utf8(&out).unwrap();
        assert!(out.contains(&format!("font-family=\"{}, sans-serif\"", crate::theme::font_sans())));
    }

    #[test]
    fn a_still_smaller_than_its_box_is_never_scaled_up() {
        assert_eq!(fit(RgbaImage::new(20, 10), 64, 64, true).dimensions(), (20, 10));
    }
}
