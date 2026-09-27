//! The attachment surfaces: photos (alone and as a grid), video posters,
//! voice notes, stickers, file pills and link cards, plus the tail every
//! block can carry. Sizes and labels are settled in `Media::sync` when the
//! message changes; render only lays out what is already known.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::format::format_bytes;
use messages_core::image::{ImageSize, fit_inside};
use messages_core::media::MediaWorker;
use messages_core::{Attachment, Message, MessagesStore};

use crate::bubble::MessageRow;
pub use crate::stills::sized_image_source;
use crate::icons::{Icon, IconName};
use crate::theme::{Palette, radius, type_scale};

pub const AUTO_DOWNLOAD_BYTES: u64 = 25 * 1024 * 1024;
const AUTO_DOWNLOAD_VIDEO_BYTES: u64 = 60 * 1024 * 1024;
const STICKER_WIDTH: f32 = 110.;
const PREVIEW_WIDTH: f32 = 280.;
const PREVIEW_IMAGE_HEIGHT: f32 = 150.;
/// Cap for an inline photo or video poster; a bubble stays readable past it.
pub const MEDIA_MAX_WIDTH: u32 = 320;
const MEDIA_MAX_HEIGHT: u32 = 420;
const PLAY_CIRCLE: f32 = 44.;
const GRID_GAP: f32 = 2.;
const GRID_MAX_TILES: usize = 4;
const TAIL_WIDTH: f32 = 14.;
const TAIL_HEIGHT: f32 = 16.;
const AUDIO_POLL: Duration = Duration::from_millis(400);

pub fn media_worker() -> &'static Arc<MediaWorker> {
    static WORKER: OnceLock<Arc<MediaWorker>> = OnceLock::new();
    WORKER.get_or_init(MediaWorker::new)
}

/// Runs `future` on the store's runtime and hands its output back to the row
/// on the foreground thread.
pub fn run_for<V: 'static, T: Send + 'static>(
    store: &MessagesStore,
    future: impl std::future::Future<Output = T> + Send + 'static,
    cx: &mut Context<V>,
    done: impl FnOnce(&mut V, T, &mut Context<V>) + 'static,
) {
    let (tx, rx) = tokio::sync::oneshot::channel();
    store.spawn(async move {
        let _ = tx.send(future.await);
    });
    cx.spawn(async move |this, cx| {
        if let Ok(value) = rx.await {
            let _ = this.update(cx, |this, cx| done(this, value, cx));
        }
    })
    .detach();
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        Some(match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    }
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in input.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=') {
        buffer = (buffer << 6) | value(byte)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn percent_decode(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&input[index + 1..index + 3], 16) {
                out.push(value);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    out
}

pub(crate) fn decode_data_url(url: &str) -> Option<(ImageFormat, Vec<u8>)> {
    let (meta, payload) = url.strip_prefix("data:")?.split_once(',')?;
    let mime = meta.split(';').next().unwrap_or("");
    let format = match mime {
        "image/svg+xml" => ImageFormat::Svg,
        "image/png" => ImageFormat::Png,
        "image/jpeg" | "image/jpg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::Webp,
        "image/bmp" => ImageFormat::Bmp,
        _ => return None,
    };
    let bytes = if meta.ends_with(";base64") { base64_decode(payload)? } else { percent_decode(payload) };
    Some((format, bytes))
}

thread_local! {
    /// A data URL decodes to one `Image` per string, so GPUI's asset cache keys it once.
    static DATA_IMAGES: RefCell<HashMap<String, Option<Arc<Image>>>> = RefCell::new(HashMap::new());
}

fn data_image(url: &str) -> Option<Arc<Image>> {
    DATA_IMAGES.with(|cache| {
        if let Some(image) = cache.borrow().get(url) {
            return image.clone();
        }
        let image = decode_data_url(url).map(|(format, bytes)| Arc::new(Image::from_bytes(format, bytes)));
        cache.borrow_mut().insert(url.to_owned(), image.clone());
        image
    })
}

pub fn is_data_url(path: &Path) -> bool {
    path.as_os_str().to_string_lossy().starts_with("data:")
}

/// What `img()` should be handed for a cached file or a fixture's data URL.
pub fn image_source(path: &Path) -> ImageSource {
    let text = path.to_string_lossy();
    if text.starts_with("data:") {
        if let Some(image) = data_image(&text) {
            return ImageSource::Image(image);
        }
    }
    ImageSource::Resource(Resource::Path(Arc::from(path)))
}

/// `img()` of the still at `path`, sized to a `width` x `height` box. Left
/// without an aspect ratio, `img()` takes the picture's own, which beats the
/// explicit height: a cover crop then lays out taller than its box, spills past
/// it and loses its bottom corners.
pub fn sized_img(path: &Path, width: f32, height: f32, fit: ObjectFit) -> Img {
    let source = sized_image_source(path, px(width), px(height), if matches!(fit, ObjectFit::Cover) { ObjectFit::Cover } else { ObjectFit::Contain });
    img(source).w(px(width)).h(px(height)).aspect_ratio(width / height.max(1.)).object_fit(fit)
}

/// The decoded still behind `path`, from the same cache `img()` uses.
pub fn render_image(path: &Path, window: &mut Window, cx: &mut App) -> Option<Arc<RenderImage>> {
    let text = path.to_string_lossy();
    if text.starts_with("data:") {
        return data_image(&text)?.use_render_image(window, cx);
    }
    window.use_asset::<ImgResourceLoader>(&Resource::Path(Arc::from(path)), cx).and_then(Result::ok)
}

/// The image scaled to fit inside `cap` x 420 without cropping; 4:3 when the size is unknown.
pub fn media_dims(attachment: &Attachment, cap: u32) -> (f32, f32) {
    match (attachment.width, attachment.height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => {
            let fit = fit_inside(ImageSize { width, height }, cap, MEDIA_MAX_HEIGHT);
            (fit.width as f32, fit.height as f32)
        }
        _ => (cap as f32, (cap as f32 / (4. / 3.)).round()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileBox {
    pub index: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Two squares side by side, a wide tile over two squares, or a two-by-two.
pub fn grid_rows(count: usize, total: f32) -> (Vec<Vec<TileBox>>, f32) {
    let half = ((total - GRID_GAP) / 2.).floor();
    let step = half + GRID_GAP;
    let square = |index, x, y| TileBox { index, x, y, width: half, height: half };
    match count {
        2 => (vec![vec![square(0, 0., 0.), square(1, step, 0.)]], half),
        3 => {
            let wide = (total / 2.).round();
            (vec![vec![TileBox { index: 0, x: 0., y: 0., width: total, height: wide }], vec![square(1, 0., wide + GRID_GAP), square(2, step, wide + GRID_GAP)]], wide + GRID_GAP + half)
        }
        _ => (vec![vec![square(0, 0., 0.), square(1, step, 0.)], vec![square(2, 0., step), square(3, step, step)]], step + half),
    }
}

/// The block's outer corners keep the bubble radius; every corner inside it is tight.
pub fn tile_radius(tile: &TileBox, width: f32, height: f32) -> Corners<Pixels> {
    let big = radius::BUBBLE;
    let small = radius::BUBBLE_TIGHT;
    let right = (tile.x + tile.width - width).abs() < 0.5;
    let bottom = (tile.y + tile.height - height).abs() < 0.5;
    Corners {
        top_left: if tile.x == 0. && tile.y == 0. { big } else { small },
        top_right: if right && tile.y == 0. { big } else { small },
        bottom_left: if tile.x == 0. && bottom { big } else { small },
        bottom_right: if right && bottom { big } else { small },
    }
}

/// m:ss, the way Messages labels an audio message.
pub fn format_duration(ms: f64) -> String {
    let total = (ms / 1000.).round().max(0.) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// The strings an attachment shows, formatted once per message change.
pub struct Labels {
    pub size: SharedString,
    pub download: SharedString,
    pub duration: Option<SharedString>,
}

pub fn labels_for(attachment: &Attachment) -> Labels {
    let size = format_bytes(attachment.bytes);
    Labels { download: format!("Click to download ({size})").into(), size: size.into(), duration: attachment.duration_ms.map(|ms| format_duration(ms).into()) }
}

pub fn host_of(url: &str) -> String {
    let lower = url.to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        &url[8..]
    } else if lower.starts_with("http://") {
        &url[7..]
    } else {
        url
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    if host.is_empty() {
        return url.to_owned();
    }
    if host.len() >= 4 && host[..4].eq_ignore_ascii_case("www.") { host[4..].to_owned() } else { host.to_owned() }
}

/// Protocol, trailing slashes and case do not make two links different.
pub fn same_url(a: &str, b: &str) -> bool {
    fn strip(value: &str) -> String {
        let value = value.trim().to_ascii_lowercase();
        let value = value.strip_prefix("https://").or_else(|| value.strip_prefix("http://")).unwrap_or(&value).to_owned();
        value.trim_end_matches('/').to_owned()
    }
    strip(a) == strip(b)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Sticker,
    Audio,
    Image,
    Video,
    File,
}

pub fn kind_of(message: &Message, attachment: &Attachment) -> Kind {
    // A stickerFor message is a sticker someone dropped on another message,
    // even when the server did not flag the attachment itself.
    if attachment.is_sticker || message.sticker_for.is_some() {
        Kind::Sticker
    } else if message.is_audio || attachment.mime.starts_with("audio/") {
        Kind::Audio
    } else if attachment.mime.starts_with("image/") {
        Kind::Image
    } else if attachment.mime.starts_with("video/") {
        Kind::Video
    } else {
        Kind::File
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transcript {
    Loading,
    Text(SharedString),
    Error(SharedString),
}

/// Per-attachment state the row keeps between renders.
#[derive(Default)]
pub struct AttachmentState {
    pub failed: bool,
    fetching: bool,
    /// The cached file's header was read once to settle an EXIF-blind size.
    verified: bool,
    /// None until asked; Some(None) when the original is what gets painted.
    preview: Option<Option<PathBuf>>,
    preview_for: Option<PathBuf>,
    poster: Option<Option<(PathBuf, f32, f32)>>,
    pub transcript: Option<Transcript>,
}

#[derive(Default)]
pub struct Media {
    pub states: HashMap<String, AttachmentState>,
    pub playing: Option<String>,
    poll: Option<Task<()>>,
}

impl Media {
    pub fn state(&self, guid: &str) -> Option<&AttachmentState> {
        self.states.get(guid)
    }

    /// Starts every download, header read, preview and poster the message's
    /// attachments need now. Called when the message changes, never from render.
    pub fn sync(row: &mut MessageRow, cx: &mut Context<MessageRow>) {
        let message = row.model.message.clone();
        let chat_guid = message.chat_guid.clone();
        let message_guid = message.guid.clone();
        let preview_image = message.url_preview.as_ref().and_then(|preview| if preview.image_path.is_some() { None } else { preview.image_attachment_guid.clone() });
        for attachment in &message.attachments {
            let is_preview_image = preview_image.as_deref() == Some(attachment.guid.as_str());
            if attachment.hidden && !is_preview_image {
                continue;
            }
            let kind = kind_of(&message, attachment);
            let state = row.media.states.entry(attachment.guid.clone()).or_default();
            let wants = match (&attachment.local_path, kind) {
                (None, _) if is_preview_image => !state.failed,
                (None, Kind::Sticker) => !state.failed,
                (None, Kind::Image) => attachment.bytes <= AUTO_DOWNLOAD_BYTES && !state.failed,
                (None, Kind::Video) => attachment.bytes <= AUTO_DOWNLOAD_VIDEO_BYTES && !state.failed,
                // A cached file comes with the size the server reported, which
                // ignores EXIF orientation, so its header is read once.
                (Some(path), Kind::Image) => !attachment.measured && !state.verified && !is_data_url(path),
                _ => false,
            };
            if wants && !state.fetching {
                state.fetching = true;
                if attachment.local_path.is_some() {
                    state.verified = true;
                }
                let (name, mime) = if is_preview_image { ("preview.jpg".to_owned(), Some("image/jpeg".to_owned())) } else { (attachment.name.clone(), Some(attachment.mime.clone())) };
                fetch(row, &chat_guid, &message_guid, &attachment.guid, name, mime, cx);
                // A file already on disk paints now; the fetch only re-reads its header.
                if attachment.local_path.is_none() {
                    continue;
                }
            }
            let Some(path) = attachment.local_path.clone() else { continue };
            let state = row.media.states.entry(attachment.guid.clone()).or_default();
            match kind {
                Kind::Image => {
                    let oversized = attachment.width.zip(attachment.height).is_some_and(|(w, h)| w.max(h) > messages_core::media::PREVIEW_MAX_EDGE);
                    let still = oversized && attachment.mime != "image/gif" && !is_data_url(&path);
                    if !still {
                        state.preview = Some(None);
                    } else if state.preview_for.as_ref() != Some(&path) {
                        state.preview_for = Some(path.clone());
                        state.preview = None;
                        let guid = attachment.guid.clone();
                        let worker = media_worker().clone();
                        run_for(&row.store, async move { worker.preview(&path).await }, cx, move |row, preview, cx| {
                            row.media.states.entry(guid).or_default().preview = Some(preview);
                            cx.notify();
                        });
                    }
                }
                Kind::Video if state.poster.is_none() => {
                    state.poster = Some(None);
                    let guid = attachment.guid.clone();
                    let worker = media_worker().clone();
                    let attachment_guid = attachment.guid.clone();
                    run_for(
                        &row.store,
                        async move {
                            let poster = worker.video_poster(&path, &attachment_guid).await?;
                            let size = messages_core::image::image_size(&poster.to_string_lossy()).await?;
                            let fit = fit_inside(size, MEDIA_MAX_WIDTH, MEDIA_MAX_HEIGHT);
                            Some((poster, fit.width as f32, fit.height as f32))
                        },
                        cx,
                        move |row, poster, cx| {
                            if poster.is_some() {
                                row.media.states.entry(guid).or_default().poster = Some(poster);
                                cx.notify();
                            }
                        },
                    );
                }
                _ => {}
            }
        }
    }

    /// The still a photo paints: the scaled preview once it is settled, the
    /// original when none is needed, nothing while one is being made (a frame
    /// holding the original already pays for its full-size texture).
    pub fn still(&self, attachment: &Attachment) -> Option<PathBuf> {
        let src = attachment.local_path.clone()?;
        match self.states.get(&attachment.guid).and_then(|state| state.preview.clone()) {
            Some(Some(preview)) => Some(preview),
            Some(None) => Some(src),
            None => None,
        }
    }
}

fn fetch(row: &MessageRow, chat_guid: &str, message_guid: &str, attachment_guid: &str, name: String, mime: Option<String>, cx: &mut Context<MessageRow>) {
    let store = row.store.clone();
    let (chat, message, attachment) = (chat_guid.to_owned(), message_guid.to_owned(), attachment_guid.to_owned());
    let key = attachment.clone();
    run_for(
        &row.store,
        async move { store.attachment_src(&chat, &message, &attachment, &name, mime.as_deref()).await.is_ok() },
        cx,
        move |row, ok, cx| {
            let state = row.media.states.entry(key).or_default();
            state.fetching = false;
            if !ok {
                state.failed = true;
                cx.notify();
            }
        },
    );
}

/// How a block's tail is filled.
#[derive(Clone)]
pub enum TailFill {
    Color(Hsla),
    /// The picture's own bottom outer corner, as painted into a box of this
    /// size, or the colour when the picture does not reach that corner.
    Picture(PathBuf, f32, f32, Source, Hsla),
}

/// Where the lobe's picture comes from: the still the block paints (decoded
/// at the block's size with the block's fit), or the frame a GIF is on.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Source {
    Still { cover: bool },
    Gif { cover: bool },
}

/// What the tail under a picture block carries: the picture once there is one
/// to show, the block's placeholder colour until then.
pub fn picture_tail(tail: Option<TailFill>, still: Option<PathBuf>, mime: &str, width: f32, height: f32, cover: bool) -> Option<TailFill> {
    let source = if mime == "image/gif" && still.as_ref().is_some_and(|path| !is_data_url(path)) { Source::Gif { cover } } else { Source::Still { cover } };
    match (tail, still) {
        (Some(TailFill::Color(color)), Some(path)) => Some(TailFill::Picture(path, width, height, source, color)),
        (tail, _) => tail,
    }
}

/// Where the picture sits relative to the lobe, as (width, height, overflow
/// past the box's outer side, overflow past its bottom), all in lobe space.
/// The lobe shows the bottom outer 12% by 14% of the box, the same cut Messages
/// masks into its tail. None when a contained picture leaves that corner to
/// the box's own fill.
fn lobe_picture(box_w: f32, box_h: f32, image_w: f32, image_h: f32, cover: bool) -> Option<(f32, f32, f32, f32)> {
    let (sx, sy) = (box_w / image_w, box_h / image_h);
    let scale = if cover { sx.max(sy) } else { sx.min(sy) };
    let (w, h) = (image_w * scale, image_h * scale);
    if w < box_w - 1. || h < box_h - 1. {
        return None;
    }
    let (fx, fy) = (TAIL_WIDTH / (0.12 * box_w), TAIL_HEIGHT / (0.14 * box_h));
    Some((w * fx, h * fy, (w - box_w) / 2. * fx, (h - box_h) / 2. * fy))
}

fn lobe_and_cut(from_me: bool, lobe: AnyElement, palette: &Palette) -> [AnyElement; 2] {
    let cut = div().absolute().bottom_0().w(px(10.)).h(px(20.)).bg(palette.canvas);
    let cut = if from_me { cut.right(px(-10.)).rounded_bl(px(10.)) } else { cut.left(px(-10.)).rounded_br(px(10.)) };
    [lobe, cut.into_any_element()]
}

/// The bubble's fill continues past its rounded corner, then a canvas-coloured
/// quad carves the concave curve back out. Painted before the block, which
/// covers the half of the lobe that sits inside it.
fn tail(from_me: bool, fill: &TailFill, palette: &Palette) -> [AnyElement; 2] {
    let lobe = match fill {
        TailFill::Color(color) => {
            let lobe = div().absolute().bottom_0().w(px(TAIL_WIDTH)).h(px(TAIL_HEIGHT)).bg(*color);
            if from_me { lobe.right(px(-5.)).rounded_bl(px(14.)) } else { lobe.left(px(-5.)).rounded_br(px(14.)) }.into_any_element()
        }
        TailFill::Picture(path, box_w, box_h, source, fallback) => {
            let (path, box_w, box_h, source, fallback) = (path.clone(), *box_w, *box_h, *source, *fallback);
            let lobe = div().absolute().bottom_0().w(px(TAIL_WIDTH)).h(px(TAIL_HEIGHT)).child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        let (picture, cover) = match source {
                            Source::Gif { cover } => (crate::gif::current_frame(&path, cx), cover),
                            Source::Still { cover } => (crate::stills::sized_image(&path, px(box_w), px(box_h), if cover { ObjectFit::Cover } else { ObjectFit::Contain }, window, cx).map(|image| (image, 0)), cover),
                        };
                        let Some((image, frame)) = picture else { return };
                        let pixels = image.size(frame);
                        if pixels.width.0 <= 0 || pixels.height.0 <= 0 {
                            return;
                        }
                        let radii = if from_me { Corners { bottom_left: px(14.), ..Corners::default() } } else { Corners { bottom_right: px(14.), ..Corners::default() } };
                        let Some((width, height, overflow_x, overflow_y)) = lobe_picture(box_w, box_h, pixels.width.0 as f32, pixels.height.0 as f32, cover) else {
                            window.paint_quad(gpui_kit::fill(bounds, fallback).corner_radii(radii));
                            return;
                        };
                        let size = size(px(width), px(height));
                        let bottom = bounds.origin.y + bounds.size.height + px(overflow_y);
                        let origin = if from_me {
                            point(bounds.origin.x + bounds.size.width + px(overflow_x) - size.width, bottom - size.height)
                        } else {
                            point(bounds.origin.x - px(overflow_x), bottom - size.height)
                        };
                        let _ = window.paint_image(bounds, Bounds { origin, size }, radii, image, frame, false);
                    },
                )
                .size_full(),
            );
            if from_me { lobe.right(px(-5.)) } else { lobe.left(px(-5.)) }.into_any_element()
        }
    };
    lobe_and_cut(from_me, lobe, palette)
}

/// Holds one block so its tail hangs off that block's corner rather than the widest one in the message.
pub fn tail_box(from_me: bool, fill: Option<TailFill>, palette: &Palette, child: impl IntoElement) -> Div {
    div()
        .relative()
        .flex()
        .flex_col()
        .max_w_full()
        .min_w(px(0.))
        .when(from_me, |el| el.items_end())
        .when(!from_me, |el| el.items_start())
        .when_some(fill, |el, fill| el.children(tail(from_me, &fill, palette)))
        .child(child)
}

fn caption(text: impl Into<SharedString>, color: Hsla) -> Div {
    div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(color).child(text.into())
}

fn placeholder(id: ElementId, icon: IconName, label: SharedString, width: f32, height: f32, palette: &Palette) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(width))
        .h(px(height))
        .rounded(radius::BUBBLE)
        .bg(palette.received)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .cursor_pointer()
        .child(Icon::new(icon).size(px(22.)).color(palette.secondary))
        .child(caption(label, palette.secondary))
}

fn retry_label(row: &MessageRow, attachment: &Attachment, auto: u64) -> SharedString {
    let state = row.media.state(&attachment.guid);
    if state.is_some_and(|state| state.failed) {
        "Could not load. Click to retry.".into()
    } else if attachment.bytes <= auto || state.is_some_and(|state| state.fetching) {
        "Loading\u{2026}".into()
    } else {
        row.model.labels.get(&attachment.guid).map(|labels| labels.download.clone()).unwrap_or_default()
    }
}

fn retry(attachment: &Attachment) -> impl Fn(&mut MessageRow, &ClickEvent, &mut Window, &mut Context<MessageRow>) + 'static {
    let (guid, name, mime) = (attachment.guid.clone(), attachment.name.clone(), attachment.mime.clone());
    move |row, _, _, cx| {
        let state = row.media.states.entry(guid.clone()).or_default();
        state.failed = false;
        state.fetching = true;
        let message = row.model.message.clone();
        fetch(row, &message.chat_guid, &message.guid, &guid, name.clone(), Some(mime.clone()), cx);
        cx.notify();
    }
}

fn open_lightbox(attachment: &Attachment, message: &Message) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let (chat, guid) = (message.chat_guid.clone(), attachment.guid.clone());
    move |_, window, cx| crate::lightbox::open(&chat, &guid, window, cx)
}

pub fn image(row: &MessageRow, index: usize, attachment: &Attachment, tail: Option<TailFill>, palette: &Palette, cx: &mut Context<MessageRow>) -> AnyElement {
    let from_me = row.model.message.from_me;
    let (width, height) = media_dims(attachment, MEDIA_MAX_WIDTH);
    if attachment.local_path.is_none() {
        let label = retry_label(row, attachment, AUTO_DOWNLOAD_BYTES);
        let body = placeholder(("image", index).into(), IconName::Image, label, width, height, palette).on_click(cx.listener(retry(attachment)));
        return tail_box(from_me, tail, palette, body).into_any_element();
    }
    let still = row.media.still(attachment);
    let radii = Corners::all(radius::BUBBLE);
    let picture: Option<AnyElement> = still.clone().map(|path| {
        if attachment.mime == "image/gif" && !is_data_url(&path) {
            crate::gif::gif_image(Arc::from(path.as_path()), crate::gif::Fit::Contain, radii).into_any_element()
        } else {
            sized_img(&path, width, height, ObjectFit::Contain).rounded(radius::BUBBLE).into_any_element()
        }
    });
    let body = div()
        .id(("image", index))
        .w(px(width))
        .h(px(height))
        .rounded(radius::BUBBLE)
        .border_1()
        .border_color(crate::primitives::image_outline(palette))
        .bg(palette.received)
        .relative()
        .cursor_pointer()
        .on_click(open_lightbox(attachment, &row.model.message))
        .children(picture);
    tail_box(from_me, picture_tail(tail, still, &attachment.mime, width, height, false), palette, body).into_any_element()
}

pub fn photo_grid(row: &MessageRow, photos: &[Attachment], tail: Option<TailFill>, palette: &Palette, cx: &mut Context<MessageRow>) -> AnyElement {
    let from_me = row.model.message.from_me;
    let shown = &photos[..photos.len().min(GRID_MAX_TILES)];
    let width = MEDIA_MAX_WIDTH as f32;
    let (rows, height) = grid_rows(shown.len(), width);
    let last_row = rows.last().cloned().unwrap_or_default();
    // The tail belongs to the tile in the grid's bottom outer corner.
    let tail_tile = if from_me { last_row.last().copied() } else { last_row.first().copied() };
    let more = photos.len() - shown.len();
    let message = row.model.message.clone();
    div()
        .flex()
        .flex_col()
        .gap(px(GRID_GAP))
        .w(px(width))
        .flex_shrink_0()
        .children(rows.into_iter().map(|tiles| {
            div().flex().flex_row().gap(px(GRID_GAP)).children(tiles.into_iter().map(|tile| {
                let attachment = &shown[tile.index];
                let radii = tile_radius(&tile, width, height);
                let still = row.media.still(attachment);
                let picture = still.clone().map(|path| {
                    if attachment.mime == "image/gif" && !is_data_url(&path) {
                        crate::gif::gif_image(Arc::from(path.as_path()), crate::gif::Fit::Cover, radii).into_any_element()
                    } else {
                        sized_img(&path, tile.width, tile.height, ObjectFit::Cover).rounded_tl(radii.top_left).rounded_tr(radii.top_right).rounded_bl(radii.bottom_left).rounded_br(radii.bottom_right).into_any_element()
                    }
                });
                let label = (tile.index == shown.len() - 1 && more > 0).then(|| {
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .bg(hsla(0., 0., 0., 0.5))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(div().text_size(px(22.)).line_height(px(28.)).font_weight(FontWeight::SEMIBOLD).text_color(white()).children(row.model.more.clone()))
                });
                let tile_el = div()
                    .id(("tile", tile.index))
                    .w(px(tile.width))
                    .h(px(tile.height))
                    .relative()
                    .flex_shrink_0()
                    .bg(palette.received)
                    .rounded_tl(radii.top_left)
                    .rounded_tr(radii.top_right)
                    .rounded_bl(radii.bottom_left)
                    .rounded_br(radii.bottom_right)
                    .cursor_pointer()
                    .hover(|style| style.opacity(0.94))
                    .on_click(open_lightbox(attachment, &message))
                    .on_mouse_down(MouseButton::Right, cx.listener({
                        let guid = attachment.guid.clone();
                        move |row, event: &MouseDownEvent, window, cx| row.open_attachment_menu(&guid, event.position, window, cx)
                    }))
                    .children(picture)
                    .children(label);
                let fill = if Some(tile) == tail_tile { picture_tail(tail.clone(), still.clone(), &attachment.mime, tile.width, tile.height, true) } else { None };
                tail_box(from_me, fill, palette, tile_el).flex_shrink_0()
            }))
        }))
        .into_any_element()
}

pub fn video(row: &MessageRow, index: usize, attachment: &Attachment, tail: Option<TailFill>, palette: &Palette, cx: &mut Context<MessageRow>) -> AnyElement {
    let from_me = row.model.message.from_me;
    let state = row.media.state(&attachment.guid);
    let poster = state.and_then(|state| state.poster.clone()).flatten();
    let (width, height) = match &poster {
        Some((_, width, height)) => (*width, *height),
        None => media_dims(attachment, MEDIA_MAX_WIDTH),
    };
    if attachment.local_path.is_none() {
        let label = retry_label(row, attachment, AUTO_DOWNLOAD_VIDEO_BYTES);
        let body = placeholder(("video", index).into(), IconName::Video, label, width, height, palette).on_click(cx.listener(retry(attachment)));
        return tail_box(from_me, tail, palette, body).into_any_element();
    }
    let duration = row.model.labels.get(&attachment.guid).and_then(|labels| labels.duration.clone()).filter(|_| poster.is_none());
    let attachment_for_open = attachment.clone();
    let body = div()
        .id(("video", index))
        .w(px(width))
        .h(px(height))
        .rounded(radius::BUBBLE)
        .relative()
        .cursor_pointer()
        .bg(hsla(240. / 360., 0.03, 0.11, 1.))
        .hover(|style| style.opacity(0.94))
        .on_click(cx.listener(move |row, _, _, cx| {
            open_attachment(row, &attachment_for_open, cx);
        }))
        .when_some(poster.clone(), |el, (path, _, _)| el.child(sized_img(&path, width, height, ObjectFit::Contain).rounded(radius::BUBBLE)))
        .child(
            div()
                .absolute()
                .top(px((height / 2. - PLAY_CIRCLE / 2.).round()))
                .left(px((width / 2. - PLAY_CIRCLE / 2.).round()))
                .w(px(PLAY_CIRCLE))
                .h(px(PLAY_CIRCLE))
                .rounded(px(PLAY_CIRCLE / 2.))
                .bg(hsla(0., 0., 0., 0.5))
                .flex()
                .items_center()
                .justify_center()
                .child(svg().path(gpui_kit::assets::IconName::Play.path()).size(px(18.)).ml(px(2.)).text_color(white())),
        )
        .when_some(duration, |el, duration| {
            el.child(div().absolute().bottom(px(8.)).right(px(10.)).text_size(type_scale::MICRO.font_size).line_height(type_scale::MICRO.line_height).font_weight(FontWeight::SEMIBOLD).text_color(white()).child(duration))
        });
    tail_box(from_me, picture_tail(tail, poster.map(|(path, _, _)| path), "", width, height, false), palette, body).into_any_element()
}

/// Runs `then` with the file on disk, downloading it first when needed.
fn with_local(row: &mut MessageRow, attachment: &Attachment, cx: &mut Context<MessageRow>, then: impl FnOnce(&mut MessageRow, PathBuf, &mut Context<MessageRow>) + 'static) {
    if let Some(path) = attachment.local_path.clone().filter(|path| !is_data_url(path)) {
        then(row, path, cx);
        return;
    }
    let store = row.store.clone();
    let message = row.model.message.clone();
    let (guid, name, mime) = (attachment.guid.clone(), attachment.name.clone(), attachment.mime.clone());
    run_for(
        &row.store,
        async move { store.attachment_src(&message.chat_guid, &message.guid, &guid, &name, Some(&mime)).await.ok() },
        cx,
        move |row, path, cx| {
            if let Some(path) = path.filter(|path| !is_data_url(path)) {
                then(row, path, cx);
            }
        },
    );
}

pub fn open_attachment(row: &mut MessageRow, attachment: &Attachment, cx: &mut Context<MessageRow>) {
    with_local(row, attachment, cx, |_, path, _| messages_core::open::open_external(&path.to_string_lossy()));
}

pub fn copy_attachment(row: &mut MessageRow, attachment: &Attachment, cx: &mut Context<MessageRow>) {
    let store = row.store.clone();
    let mime = attachment.mime.clone();
    with_local(row, attachment, cx, move |_, path, _| store.spawn(async move { messages_core::clipboard::copy_file(&path, &mime).await }));
}

fn toggle_audio(row: &mut MessageRow, attachment: &Attachment, cx: &mut Context<MessageRow>) {
    if row.media.playing.is_some() {
        messages_core::open::stop_audio();
        row.media.playing = None;
        row.media.poll = None;
        cx.notify();
        return;
    }
    let guid = attachment.guid.clone();
    with_local(row, attachment, cx, move |row, path, cx| {
        if messages_core::open::play_audio(&path) {
            row.media.playing = Some(guid);
            watch_player(row, cx);
            cx.notify();
        }
    });
}

/// The player is a child process nobody hears back from, so while it runs the
/// row checks on it every 400 ms, and stops checking once it has exited.
fn watch_player(row: &mut MessageRow, cx: &mut Context<MessageRow>) {
    row.media.poll = Some(cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(AUDIO_POLL).await;
            let playing = this
                .update(cx, |row, cx| {
                    if row.media.playing.is_some() && !messages_core::open::is_audio_playing() {
                        row.media.playing = None;
                        cx.notify();
                    }
                    row.media.playing.is_some()
                })
                .unwrap_or(false);
            if !playing {
                break;
            }
        }
    }));
}

fn transcribe(row: &mut MessageRow, attachment: &Attachment, cx: &mut Context<MessageRow>) {
    let Some(assistant) = crate::thread::assistant(cx) else { return };
    let state = row.media.states.entry(attachment.guid.clone()).or_default();
    if state.transcript == Some(Transcript::Loading) {
        return;
    }
    state.transcript = Some(Transcript::Loading);
    cx.notify();
    let guid = attachment.guid.clone();
    let store = row.store.clone();
    let message = row.model.message.clone();
    let local = attachment.local_path.clone();
    let (att_guid, name, mime) = (attachment.guid.clone(), attachment.name.clone(), attachment.mime.clone());
    run_for(
        &row.store,
        async move {
            let path = match local {
                Some(path) => Some(path),
                None => store.attachment_src(&message.chat_guid, &message.guid, &att_guid, &name, Some(&mime)).await.ok(),
            };
            match path.filter(|path| !is_data_url(path)) {
                None => Transcript::Error("Could not load the audio file.".into()),
                Some(path) => match assistant.client.transcribe(&path).await {
                    Ok(text) => Transcript::Text(text.into()),
                    Err(error) => Transcript::Error(error.0.into()),
                },
            }
        },
        cx,
        move |row, transcript, cx| {
            row.media.states.entry(guid).or_default().transcript = Some(transcript);
            cx.notify();
        },
    );
}

pub fn audio(row: &MessageRow, index: usize, attachment: &Attachment, tail: Option<TailFill>, fill: Hsla, palette: &Palette, cx: &mut Context<MessageRow>) -> AnyElement {
    let from_me = row.model.message.from_me;
    let playing = row.media.playing.as_deref() == Some(attachment.guid.as_str());
    let on_fill = if from_me { palette.on_accent } else { palette.text };
    let label: SharedString = row.model.labels.get(&attachment.guid).and_then(|labels| labels.duration.clone()).unwrap_or_else(|| "Audio message".into());
    let transcript = row.media.state(&attachment.guid).and_then(|state| state.transcript.clone());
    let assistant = row.props.assistant;
    let for_toggle = attachment.clone();
    let for_transcribe = attachment.clone();
    let glyph = if playing { gpui_kit::assets::IconName::Square } else { gpui_kit::assets::IconName::Play };
    let pill = div()
        .id(("audio", index))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.))
        .h(px(40.))
        .pl(px(4.))
        .pr(px(14.))
        .rounded(radius::PILL)
        .bg(fill)
        .cursor_pointer()
        .hover(|style| style.opacity(0.9))
        .on_click(cx.listener(move |row, _, _, cx| toggle_audio(row, &for_toggle, cx)))
        .child(
            div()
                .w(px(32.))
                .h(px(32.))
                .rounded(px(16.))
                .bg(if from_me { hsla(0., 0., 1., 0.16) } else { palette.ghost })
                .flex()
                .items_center()
                .justify_center()
                .child(svg().path(glyph.path()).size(px(13.)).when(!playing, |el| el.ml(px(2.))).text_color(on_fill)),
        )
        .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).font_weight(FontWeight::SEMIBOLD).text_color(on_fill).child(label));
    let transcript_text = match &transcript {
        Some(Transcript::Text(text)) => Some(text.clone()),
        _ => None,
    };
    let link = (assistant && transcript_text.is_none()).then(|| {
        let (text, color): (SharedString, Hsla) = match &transcript {
            Some(Transcript::Loading) => ("Transcribing\u{2026}".into(), palette.accent),
            Some(Transcript::Error(error)) => (error.clone(), palette.danger),
            _ => ("Transcribe".into(), palette.accent),
        };
        let loading = transcript == Some(Transcript::Loading);
        div()
            .id(("transcribe", index))
            .when(!loading, |el| el.cursor_pointer().hover(|style| style.opacity(0.8)))
            .on_click(cx.listener(move |row, _, _, cx| transcribe(row, &for_transcribe, cx)))
            .child(caption(text, color))
    });
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .max_w(px(260.))
        .when(from_me, |el| el.items_end())
        .when(!from_me, |el| el.items_start())
        .child(tail_box(from_me, tail, palette, pill))
        .children(link)
        .when_some(transcript_text, |el, text| el.child(caption(text, if from_me { palette.on_accent_soft } else { palette.secondary })))
        .into_any_element()
}

pub fn sticker(attachment: &Attachment) -> AnyElement {
    let height = match (attachment.width, attachment.height) {
        (Some(w), Some(h)) if w > 0 => (STICKER_WIDTH * h as f32 / w as f32).round(),
        _ => STICKER_WIDTH,
    };
    match &attachment.local_path {
        None => div().w(px(STICKER_WIDTH)).h(px(height)).into_any_element(),
        Some(path) => sized_img(path, STICKER_WIDTH, height, ObjectFit::Contain).into_any_element(),
    }
}

pub fn file(row: &MessageRow, index: usize, attachment: &Attachment, fill: Hsla, palette: &Palette, cx: &mut Context<MessageRow>) -> AnyElement {
    let from_me = row.model.message.from_me;
    let audio = row.model.message.is_audio;
    let name: SharedString = if audio { "Audio message".into() } else { attachment.name.clone().into() };
    let fg = if from_me { palette.on_accent } else { palette.text };
    let for_open = attachment.clone();
    div()
        .id(("file", index))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.))
        .pl(px(12.))
        .pr(px(14.))
        .py(px(10.))
        .rounded(radius::BUBBLE)
        .bg(fill)
        .cursor_pointer()
        .max_w(px(320.))
        .on_click(cx.listener(move |row, _, _, cx| open_attachment(row, &for_open, cx)))
        .child(Icon::new(if audio { IconName::Audio } else { IconName::File }).size(px(20.)).color(fg))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w(px(0.))
                .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).font_weight(FontWeight::SEMIBOLD).text_color(fg).whitespace_nowrap().text_ellipsis().child(name))
                .child(div().text_size(type_scale::MICRO.font_size).line_height(type_scale::MICRO.line_height).text_color(if from_me { hsla(0., 0., 1., 0.7) } else { palette.secondary }).children(row.model.labels.get(&attachment.guid).map(|labels| labels.size.clone()))),
        )
        .into_any_element()
}

/// The rich card for a link the server resolved. Its picture box is there from
/// the first paint, filled or not, so the row never changes height under the list.
pub fn link_preview(row: &MessageRow, id: usize, palette: &Palette) -> AnyElement {
    let message = &row.model.message;
    let Some(preview) = message.url_preview.as_ref() else { return div().into_any_element() };
    let image = preview.image_attachment_guid.as_ref().and_then(|guid| message.attachments.iter().find(|item| &item.guid == guid));
    let src: Option<PathBuf> = preview.image_path.as_ref().map(PathBuf::from).or_else(|| image.and_then(|image| image.local_path.clone()));
    let failed = image.is_some_and(|image| row.media.state(&image.guid).is_some_and(|state| state.failed));
    let has_picture = src.is_some() || (image.is_some() && !failed);
    let url = preview.url.clone();
    let site: SharedString = preview.site_name.clone().filter(|site| !site.is_empty()).unwrap_or_else(|| host_of(&preview.url)).into();
    let top = Corners { top_left: radius::BUBBLE, top_right: radius::BUBBLE, ..Corners::default() };
    div()
        .id(("preview", id))
        .w(px(PREVIEW_WIDTH))
        .rounded(radius::BUBBLE)
        .overflow_hidden()
        .bg(palette.received)
        .cursor_pointer()
        .hover(|style| style.opacity(0.9))
        .on_click(move |_, _, _| messages_core::open::open_external(&url))
        .when(has_picture, |el| {
            el.child(
                div().w(px(PREVIEW_WIDTH)).h(px(PREVIEW_IMAGE_HEIGHT)).bg(palette.raised).rounded_tl(radius::BUBBLE).rounded_tr(radius::BUBBLE).when_some(src, |el, src| {
                    el.child(sized_img(&src, PREVIEW_WIDTH, PREVIEW_IMAGE_HEIGHT, ObjectFit::Cover).rounded_tl(top.top_left).rounded_tr(top.top_right))
                }),
            )
        })
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .px(px(12.))
                .pt(px(8.))
                .pb(px(9.))
                .when_some(preview.title.clone(), |el, title| {
                    el.child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).font_weight(FontWeight::SEMIBOLD).text_color(palette.text).line_clamp(2).child(title))
                })
                .child(caption(site, palette.secondary)),
        )
        .into_any_element()
}

/// A bare URL Messages pulled out of the text: host over the address.
pub fn link_card(id: usize, href: &SharedString, host: &SharedString, from_me: bool, fill: Hsla, palette: &Palette) -> AnyElement {
    let url = href.clone();
    div()
        .id(("link", id))
        .w(px(PREVIEW_WIDTH))
        .rounded(radius::BUBBLE)
        .bg(fill)
        .cursor_pointer()
        .px(px(12.))
        .py(px(8.))
        .flex()
        .flex_col()
        .gap(px(2.))
        .hover(|style| style.opacity(0.9))
        .on_click(move |_, _, _| messages_core::open::open_external(&url))
        .child(
            div()
                .text_size(type_scale::BODY.font_size)
                .line_height(type_scale::BODY.line_height)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if from_me { palette.on_accent } else { palette.text })
                .whitespace_nowrap()
                .text_ellipsis()
                .child(host.clone()),
        )
        .child(
            div()
                .text_size(type_scale::CAPTION.font_size)
                .line_height(type_scale::CAPTION.line_height)
                .text_color(if from_me { palette.on_accent_soft } else { palette.secondary })
                .whitespace_nowrap()
                .text_ellipsis()
                .child(href.clone()),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    // gpui_kit's glob exports its own `test` attribute, which would shadow the std one.
    use std::prelude::v1::test;

    #[test]
    fn media_boxes_fit_the_cap_and_fall_back_to_four_by_three() {
        let mut attachment = Attachment {
            guid: "a".into(),
            name: "a.jpg".into(),
            mime: "image/jpeg".into(),
            bytes: 1,
            width: Some(3024),
            height: Some(4032),
            measured: true,
            is_sticker: false,
            local_path: None,
            hidden: false,
            duration_ms: None,
        };
        assert_eq!(media_dims(&attachment, 320), (315., 420.));
        attachment.width = None;
        assert_eq!(media_dims(&attachment, 320), (320., 240.));
    }

    #[test]
    fn grids_lay_out_two_three_and_four() {
        let (rows, height) = grid_rows(2, 320.);
        assert_eq!(rows.len(), 1);
        assert_eq!(height, 159.);
        let (rows, height) = grid_rows(3, 320.);
        assert_eq!(rows[0][0].width, 320.);
        assert_eq!(height, 160. + 2. + 159.);
        let (rows, _) = grid_rows(4, 320.);
        assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), vec![2, 2]);
        let corners = tile_radius(&rows[1][1], 320., 320.);
        assert_eq!(corners.bottom_right, radius::BUBBLE);
        assert_eq!(corners.top_left, radius::BUBBLE_TIGHT);
    }

    #[test]
    fn a_picture_tail_carries_the_picture_or_the_frame_a_gif_is_on() {
        let color = hsla(0., 0., 0.2, 1.);
        let path = PathBuf::from("/cache/a.gif");
        match picture_tail(Some(TailFill::Color(color)), Some(path.clone()), "image/gif", 120., 120., false) {
            Some(TailFill::Picture(p, w, h, Source::Gif { cover: false }, fallback)) => assert_eq!((p, w, h, fallback), (path.clone(), 120., 120., color)),
            _ => panic!("a gif's tail should follow its frames"),
        }
        assert!(matches!(picture_tail(Some(TailFill::Color(color)), Some(PathBuf::from("/cache/b.jpg")), "image/jpeg", 320., 213., true), Some(TailFill::Picture(_, _, _, Source::Still { cover: true }, _))));
        assert!(matches!(picture_tail(Some(TailFill::Color(color)), None, "image/jpeg", 320., 213., false), Some(TailFill::Color(_))));
        assert!(picture_tail(None, Some(path), "image/gif", 120., 120., false).is_none());
    }

    #[test]
    fn the_lobe_shows_the_bottom_outer_corner_unless_the_picture_misses_it() {
        let (w, h, dx, dy) = lobe_picture(320., 240., 640., 480., false).unwrap();
        assert!((w - TAIL_WIDTH / 0.12).abs() < 0.01 && (h - TAIL_HEIGHT / 0.14).abs() < 0.01);
        assert_eq!((dx, dy), (0., 0.));
        let (_, _, dx, dy) = lobe_picture(159., 159., 1200., 800., true).unwrap();
        assert!(dx > 0. && dy == 0.);
        // A square GIF letterboxed in a 4:3 box leaves the corner to the box's fill.
        assert!(lobe_picture(320., 240., 498., 498., false).is_none());
    }

    #[test]
    fn urls_compare_without_protocol_slash_or_case() {
        assert!(same_url("https://Example.com/a/", "http://example.com/a"));
        assert!(!same_url("https://example.com/a", "https://example.com/b"));
        assert_eq!(host_of("https://www.example.com/path?q=1"), "example.com");
        assert_eq!(host_of("example.org"), "example.org");
    }

    #[test]
    fn durations_read_as_minutes_and_seconds() {
        assert_eq!(format_duration(0.), "0:00");
        assert_eq!(format_duration(65_400.), "1:05");
    }

    #[test]
    fn data_urls_decode() {
        let (format, bytes) = decode_data_url("data:image/svg+xml;base64,PHN2Zy8+").unwrap();
        assert_eq!(format, ImageFormat::Svg);
        assert_eq!(bytes, b"<svg/>");
        let (_, bytes) = decode_data_url("data:image/svg+xml,%3Csvg%2F%3E").unwrap();
        assert_eq!(bytes, b"<svg/>");
    }
}
