//! One message row. The text is nd's `chat_bubble` (tail, run corners, reactions, selectable
//! text), pictures are `image()` from nd's pipeline (decoded at the size shown, GIFs animated,
//! the box sized from the attachment before the file is read so a row never jumps), people are
//! `avatar()`. What the message alone decides is worked out in `blocks_of`; the thread passes
//! what it knows besides (`RowProps`).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use messages_core::format::{format_bytes, format_time, is_emoji_only};
use messages_core::image::{ImageSize, fit_inside};
use messages_core::open::{TextSegment, split_links};
use messages_core::{
    Attachment, Capabilities, DeliveryState, Message, MessagePart, RichRun, Service, TextEffect, delivery_state,
    handle_name,
};
use nd::prelude::*;
use nd::components::media::BUBBLE;
use nd::theme::FillRole;

use crate::actions::*;
use crate::rows::{EDIT_WINDOW_MS, MessageRowData, TapbackGroup, UNSEND_WINDOW_MS, effect_name, tapback_groups};
use crate::ui;

/// Images up to this size download on their own; bigger ones wait for a click.
pub const AUTO_DOWNLOAD_BYTES: u64 = 25 * 1024 * 1024;
pub const AUTO_DOWNLOAD_VIDEO_BYTES: u64 = 60 * 1024 * 1024;
/// The box an inline photo or video poster fits inside.
pub const MEDIA_MAX_WIDTH: u32 = 320;
pub const MEDIA_MAX_HEIGHT: u32 = 420;
const STICKER_WIDTH: f32 = 110.;
const LINK_CARD_WIDTH: f32 = 280.;
const GRID_SIDE: f32 = 128.;
const GRID_MAX_TILES: usize = 4;
pub const AVATAR_COLUMN: f32 = 28.;
/// The time beside a hovered row's bubble.
const TIME_COLUMN: f32 = 54.;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Quote {
    pub target: String,
    pub who: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Translation {
    Loading,
    Text(String),
    Error(String),
}

/// What the thread knows about a row that the message alone does not say.
#[derive(Clone, Debug, PartialEq)]
pub struct RowProps {
    /// The conversation's primary guid: replies and drafts key on it.
    pub primary: String,
    pub is_group: bool,
    pub capabilities: Capabilities,
    pub assistant: bool,
    pub quote: Option<Quote>,
    /// "1 reply" / "N replies".
    pub replies: Option<String>,
    pub highlighted: bool,
    pub translation: Option<Translation>,
    /// The tapback picker is open on this message.
    pub picker: bool,
    /// The pointer is over the row: its time shows beside the bubble.
    pub hovered: bool,
}

/// Downloads the thread started for its attachments, and what came of them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Media {
    pub fetching: HashSet<String>,
    pub failed: HashSet<String>,
    /// Video posters made with ffmpeg, by attachment guid.
    pub posters: HashMap<String, PathBuf>,
    /// The voice note playing now.
    pub playing: Option<String>,
    /// Transcripts of voice notes, by attachment guid.
    pub transcripts: HashMap<String, Translation>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Photos(Vec<Attachment>),
    Attachment(Attachment),
    Link { href: String, host: String, matches: bool },
    Text { subject: Option<String>, body: RichText },
    Preview,
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
    // A stickerFor message is a sticker dropped on another message, flagged or not.
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
        let value =
            value.strip_prefix("https://").or_else(|| value.strip_prefix("http://")).unwrap_or(&value).to_owned();
        value.trim_end_matches('/').to_owned()
    }
    strip(a) == strip(b)
}

/// m:ss, the way Messages labels an audio message.
pub fn format_duration(ms: f64) -> String {
    let total = (ms / 1000.).round().max(0.) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// An inline photo's box: fits inside the cap without cropping, 4:3 when the size is unknown.
pub fn media_dims(attachment: &Attachment) -> (f32, f32) {
    match (attachment.width, attachment.height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => {
            let fit = fit_inside(ImageSize { width, height }, MEDIA_MAX_WIDTH, MEDIA_MAX_HEIGHT);
            (fit.width as f32, fit.height as f32)
        }
        _ => (MEDIA_MAX_WIDTH as f32, (MEDIA_MAX_WIDTH as f32 / (4. / 3.)).round()),
    }
}

pub fn emoji_only(message: &Message) -> bool {
    !message.attachments.iter().any(|item| !item.hidden) && is_emoji_only(&message.text, 3)
}

/// The blocks a body paints, in order: the photo grid, the attachments the attributed body did
/// not place, then the parts with every link split into its own card, and the rich preview last
/// when no link carried it.
/// iMessage's Big and Small effects against the bubble's 14.5 pt text (22 pt and 11 pt).
const BIG_SCALE: f32 = 22. / 14.5;
const SMALL_SCALE: f32 = 11. / 14.5;

/// The attributed body only marks links someone typed as a link, so bare URLs get the same
/// pass the plain text path gives them.
fn linkify(runs: &[RichRun]) -> Vec<RichRun> {
    let mut out = Vec::new();
    for run in runs {
        if run.link.is_some() || run.mention.is_some() {
            out.push(run.clone());
            continue;
        }
        for segment in split_links(&run.text) {
            match segment {
                TextSegment::Link { text, href } => out.push(RichRun { text, link: Some(href), ..run.clone() }),
                TextSegment::Text(text) => out.push(RichRun { text, ..run.clone() }),
            }
        }
    }
    out
}

/// One run of the attributed body as an nd span. Mentions are bold in the accent, underlines
/// take the accent too, as in Messages.
fn span_of(run: &RichRun) -> Span {
    let mut span = span(run.text.clone());
    if run.bold || run.mention.is_some() {
        span = span.bold();
    }
    if run.italic {
        span = span.italic();
    }
    if run.underline {
        span = span.underline().color(TextColor::Accent);
    }
    if run.strike {
        span = span.strikethrough();
    }
    if run.mention.is_some() {
        span = span.mention();
    }
    match run.effect {
        Some(TextEffect::Big) => span.scale(BIG_SCALE),
        Some(TextEffect::Small) => span.scale(SMALL_SCALE),
        _ => span,
    }
}

fn rich_body(runs: &[RichRun]) -> RichText {
    let mut runs = runs.to_vec();
    if let Some(first) = runs.first_mut() {
        first.text = first.text.trim_start().to_owned();
    }
    if let Some(last) = runs.last_mut() {
        last.text = last.text.trim_end().to_owned();
    }
    RichText::new(runs.iter().map(span_of))
}

pub fn blocks_of(message: &Message) -> Vec<Block> {
    if emoji_only(message) {
        return Vec::new();
    }
    let visible: Vec<&Attachment> = message.attachments.iter().filter(|item| !item.hidden).collect();
    let parts: &[MessagePart] = message.parts.as_deref().unwrap_or(&[]);
    let placed: HashSet<&str> = parts
        .iter()
        .filter_map(|part| match part {
            MessagePart::Attachment { guid } => Some(guid.as_str()),
            MessagePart::Text { .. } => None,
        })
        .collect();
    let photos: Vec<Attachment> = if message.sticker_for.is_some() {
        Vec::new()
    } else {
        visible
            .iter()
            .filter(|item| item.mime.starts_with("image/") && !item.is_sticker)
            .map(|item| (*item).clone())
            .collect()
    };
    let grid: Option<HashSet<String>> =
        (photos.len() >= 2).then(|| photos.iter().map(|item| item.guid.clone()).collect());
    let preview = message.url_preview.as_ref();

    let mut blocks = Vec::new();
    if grid.is_some() {
        blocks.push(Block::Photos(photos));
    }
    let in_grid = |guid: &str| grid.as_ref().is_some_and(|grid| grid.contains(guid));
    for attachment in &visible {
        if placed.contains(attachment.guid.as_str()) || in_grid(&attachment.guid) {
            continue;
        }
        blocks.push(Block::Attachment((*attachment).clone()));
    }

    let mut preview_rendered = false;
    let mut push_link = |blocks: &mut Vec<Block>, href: String| {
        let matches = preview.is_some_and(|preview| same_url(&href, &preview.url));
        preview_rendered |= matches;
        let host = host_of(&href);
        blocks.push(Block::Link { href, host, matches });
    };
    let push_text = |blocks: &mut Vec<Block>, text: &str, push_link: &mut dyn FnMut(&mut Vec<Block>, String)| {
        for segment in split_links(text) {
            match segment {
                TextSegment::Link { href, .. } => push_link(blocks, href),
                TextSegment::Text(value) if !value.trim().is_empty() => {
                    blocks.push(Block::Text { subject: None, body: RichText::new([value.trim().to_owned()]) })
                }
                TextSegment::Text(_) => {}
            }
        }
    };

    if !parts.is_empty() {
        for part in parts {
            match part {
                MessagePart::Attachment { guid } => {
                    if in_grid(guid) {
                        continue;
                    }
                    if let Some(attachment) = visible.iter().find(|item| &item.guid == guid) {
                        blocks.push(Block::Attachment((*attachment).clone()));
                    }
                }
                MessagePart::Text { runs } => {
                    let mut pending: Vec<RichRun> = Vec::new();
                    let flush = |pending: &mut Vec<RichRun>, blocks: &mut Vec<Block>| {
                        if pending.iter().any(|run| !run.text.trim().is_empty()) {
                            blocks.push(Block::Text { subject: None, body: rich_body(pending) });
                        }
                        pending.clear();
                    };
                    for run in linkify(runs) {
                        if let Some(href) = run.link.clone() {
                            flush(&mut pending, &mut blocks);
                            push_link(&mut blocks, href);
                            continue;
                        }
                        pending.push(run);
                    }
                    flush(&mut pending, &mut blocks);
                }
            }
        }
    } else if !message.text.trim().is_empty() {
        push_text(&mut blocks, &message.text, &mut push_link);
    }

    if let Some(subject) = message.subject.as_ref().filter(|subject| !subject.is_empty()) {
        match blocks.iter_mut().find(|block| matches!(block, Block::Text { .. })) {
            Some(Block::Text { subject: slot, .. }) => *slot = Some(subject.clone()),
            _ => blocks.insert(0, Block::Text { subject: Some(subject.clone()), body: RichText::default() }),
        }
    }
    if preview.is_some() && !preview_rendered {
        blocks.push(Block::Preview);
    }
    blocks
}

pub(crate) fn at(data: &MessageRowData) -> MessageRef {
    MessageRef { chat: data.message.chat_guid.clone(), message: data.message.guid.clone() }
}

pub(crate) fn failed(message: &Message) -> bool {
    delivery_state(message) == DeliveryState::Failed
}

/// The message's context menu: tapbacks, retry, reply, copy, translate, links, the first
/// attachment, edit and undo send, each where the server and the message allow it.
pub fn message_menu(menu: ContextMenu, data: &MessageRowData, props: &RowProps) -> ContextMenu {
    let message = &data.message;
    let caps = props.capabilities;
    let failed = failed(message);
    let age = ui::now_ms() - message.date;
    let mut menu = menu;
    if caps.reactions && !failed {
        let mine = message.tapbacks.iter().find(|t| t.from_me).map(|t| t.kind);
        let mut react = Menu::new("React");
        for kind in messages_core::TapbackKind::NAMED {
            let glyph = messages_core::tapback_glyph(kind, None);
            let item = MenuItem::action(format!("{glyph}  {}", tapback_name(kind)), React { at: at(data), kind })
                .checked(mine == Some(kind));
            react = react.item(item);
        }
        menu = menu.submenu(react);
        let groups = tapback_groups(&message.tapbacks);
        if !groups.is_empty() {
            let mut who = Menu::new("Reactions");
            for group in groups {
                for name in group.who {
                    who = who.item(MenuItem::action(format!("{}  {name}", group.glyph), CloseTapbacks).enabled(false));
                }
            }
            menu = menu.submenu(who);
        }
        menu = menu.separator();
    }
    if failed {
        menu = menu.item("Try Again", Retry(at(data)));
    }
    if caps.replies && !failed {
        menu = menu.item("Reply", Reply(at(data)));
    }
    if !message.text.is_empty() {
        menu = menu.item("Copy", CopyText(at(data)));
    }
    if props.assistant && !message.text.trim().is_empty() {
        menu =
            menu.item(if props.translation.is_some() { "Hide Translation" } else { "Translate" }, Translate(at(data)));
    }
    for segment in split_links(&message.text) {
        if let TextSegment::Link { href, .. } = segment {
            menu = menu.item(format!("Open {}", host_of(&href)), OpenLink(href));
        }
    }
    if let Some(file) = message.attachments.iter().find(|a| !a.hidden && a.local_path.is_some()) {
        let reference = AttachmentRef { at: at(data), attachment: file.guid.clone() };
        menu = menu
            .item("Open Attachment", OpenAttachment(reference.clone()))
            .item(
                if file.mime.starts_with("image/") { "Copy Image" } else { "Copy File" },
                CopyAttachment(reference.clone()),
            )
            .item("Copy File Name", CopyAttachmentName(reference));
    }
    if message.from_me && !failed && message.service == Service::IMessage && (caps.edit || caps.unsend) {
        menu = menu.separator();
        if caps.edit {
            let editable = age <= EDIT_WINDOW_MS && message.attachments.is_empty();
            menu = menu.entry(MenuItem::action("Edit", Edit(at(data))).enabled(editable));
        }
        if caps.unsend {
            menu = menu
                .entry(MenuItem::action("Undo Send", Unsend(at(data))).destructive().enabled(age <= UNSEND_WINDOW_MS));
        }
    }
    menu
}

pub fn tapback_name(kind: messages_core::TapbackKind) -> &'static str {
    use messages_core::TapbackKind::*;
    match kind {
        Love => "Love",
        Like => "Like",
        Dislike => "Dislike",
        Laugh => "Laugh",
        Emphasize => "Emphasize",
        Question => "Question",
        Emoji => "Emoji",
    }
}

fn reactions(groups: &[TapbackGroup]) -> Vec<Reaction> {
    groups.iter().map(|g| Reaction { glyph: g.glyph.clone().into(), count: g.count as u32, mine: g.mine }).collect()
}

/// The tail of a block that ends a run, in the block's fill at its bottom outer corner, the
/// shape `chat_bubble` gives a text block: it starts inside the block's rounded corner and
/// curves out past its edge.
fn with_tail(block: AnyElement, side: BubbleSide, fill: Hsla) -> AnyElement {
    let (tw, th) = (BUBBLE.tail_width, BUBBLE.tail_height);
    let lobe = canvas(move |bounds, painter, _| {
        let (x0, top) = (bounds.origin.x, bounds.origin.y);
        let bottom = top + px(th);
        let at = |x: f32, y: Pixels| point(x0 + px(x), y);
        let path = match side {
            BubbleSide::Sent => CanvasPath::new()
                .move_to(at(0., top))
                .line_to(at(th, top))
                .quad_to(at(th, bottom - px(2.)), at(th + tw, bottom))
                .quad_to(at(th - tw * 0.5, bottom), at(0., bottom - px(3.)))
                .close(),
            BubbleSide::Received => CanvasPath::new()
                .move_to(at(tw + th, top))
                .line_to(at(tw, top))
                .quad_to(at(tw, bottom - px(2.)), at(0., bottom))
                .quad_to(at(tw * 1.5, bottom), at(tw + th, bottom - px(3.)))
                .close(),
        };
        painter.fill_path(path, fill);
    })
    .w(px(tw + th))
    .h(px(th))
    .absolute()
    .bottom_0();
    // The lobe hangs past the block into the room a text bubble keeps for its own tail, so the
    // block lines up with the bubbles above it.
    let (lobe, wrapper) = match side {
        BubbleSide::Sent => (lobe.right(px(-tw)), div().mr(px(tw))),
        BubbleSide::Received => (lobe.left(px(-tw)), div().ml(px(tw))),
    };
    // Under the block: the part of the lobe inside the block's corner is hidden by its fill.
    wrapper.relative().child(lobe).child(block).into_any_element()
}

/// One block that is not text. `tail` is set when it ends a run.
fn attachment_block(
    data: &MessageRowData,
    attachment: &Attachment,
    media: &Media,
    assistant: bool,
    tail: Option<BubbleSide>,
    theme: &Theme,
) -> AnyElement {
    let message = &data.message;
    let reference = AttachmentRef { at: at(data), attachment: attachment.guid.clone() };
    let id = format!("att-{}", attachment.guid);
    let menu_ref = reference.clone();
    let menu = move |menu: ContextMenu, _: &mut Window, _: &mut App| {
        menu.item("Open", OpenAttachment(menu_ref.clone()))
            .item("Copy", CopyAttachment(menu_ref.clone()))
            .item("Copy File Name", CopyAttachmentName(menu_ref.clone()))
    };
    let failed = media.failed.contains(&attachment.guid);
    let fetching = media.fetching.contains(&attachment.guid);
    match kind_of(message, attachment) {
        Kind::Image | Kind::Sticker => {
            let sticker = kind_of(message, attachment) == Kind::Sticker;
            let (w, h) = if sticker {
                let (w, h) = media_dims(attachment);
                (STICKER_WIDTH, (STICKER_WIDTH * h / w.max(1.)).round())
            } else {
                media_dims(attachment)
            };
            let tail = tail.filter(|_| !sticker);
            let picture = match attachment.local_path.as_ref().and_then(|p| ui::image_source(&p.to_string_lossy())) {
                Some(source) => image(source)
                    .id(format!("img-{}", attachment.guid))
                    .alt(attachment.name.clone())
                    .when_some(attachment.width.zip(attachment.height), |img, (w, h)| img.size_hint(w, h))
                    .when(sticker, Image::contain)
                    .when_some(tail, |img, side| img.bubble_tail(side))
                    .w(px(w + if tail.is_some() { BUBBLE.tail_width } else { 0. }))
                    .h(px(h))
                    .rounded_lg()
                    .into_any_element(),
                None => placeholder(
                    w,
                    h,
                    if failed {
                        "Could not load. Click to retry.".to_owned()
                    } else if fetching {
                        "Loading\u{2026}".to_owned()
                    } else {
                        format!("Click to download ({})", format_bytes(attachment.bytes))
                    },
                    theme,
                ),
            };
            let local = attachment.local_path.is_some();
            let (show, download) = (ShowAttachment(reference.clone()), Download(reference));
            div()
                .id(id)
                .on_tap(move |_, window, cx| {
                    if local {
                        crate::ui::dispatch(window, cx, show.clone());
                    } else {
                        crate::ui::dispatch(window, cx, download.clone());
                    }
                })
                .context_menu(menu)
                .child(picture)
                .into_any_element()
        }
        Kind::Video => {
            let (w, h) = media_dims(attachment);
            let poster = media
                .posters
                .get(&attachment.guid)
                .filter(|p| !p.as_os_str().is_empty())
                .and_then(|p| ui::image_source(&p.to_string_lossy()));
            let duration = attachment.duration_ms.map(format_duration).unwrap_or_default();
            // A click plays it in the lightbox once it is here, and fetches it before that.
            let local = attachment.local_path.is_some();
            let (show, download) = (ShowAttachment(reference.clone()), Download(reference));
            // With a poster the tail is cut from it; without one the box takes a plain tail.
            let poster_tail = tail.filter(|_| poster.is_some());
            let box_w = w + if poster_tail.is_some() { BUBBLE.tail_width } else { 0. };
            let block = div()
                .id(id)
                .w(px(box_w))
                .h(px(h))
                .rounded_lg()
                .when(poster.is_none(), |el| el.bg(theme.surface.raised))
                .flex()
                .items_center()
                .justify_center()
                .on_tap(move |_, window, cx| {
                    if local {
                        crate::ui::dispatch(window, cx, show.clone());
                    } else {
                        crate::ui::dispatch(window, cx, download.clone());
                    }
                })
                .context_menu(menu)
                .when_some(poster, |el, poster| {
                    el.child(
                        image(poster)
                            .alt(attachment.name.clone())
                            .when_some(poster_tail, |img, side| img.bubble_tail(side))
                            .w(px(box_w))
                            .h(px(h))
                            .rounded_lg()
                            .absolute()
                            .top_0()
                            .left_0(),
                    )
                })
                // The play mark and the length sit on the overlay surface, so they read over
                // any frame, light or dark.
                .child(
                    v_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .size(px(44.))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(theme.surface.overlay)
                                .child(icon(Icon::Play, 22.).text_color(theme.text.primary).label("Play")),
                        )
                        .child(
                            div()
                                .px_2()
                                .rounded_full()
                                .bg(theme.surface.overlay)
                                .child(
                                    text(if attachment.local_path.is_none() && fetching {
                                        "Loading\u{2026}".into()
                                    } else {
                                        duration
                                    })
                                    .tabular_nums()
                                    .text_size(theme.type_scale.caption)
                                    .text_color(theme.text.primary),
                                ),
                        ),
                )
                .into_any_element();
            match tail.filter(|_| poster_tail.is_none()) {
                Some(side) => with_tail(block, side, theme.surface.raised),
                None => block,
            }
        }
        Kind::Audio => {
            let playing = media.playing.as_deref() == Some(attachment.guid.as_str());
            let duration = attachment.duration_ms.map(format_duration).unwrap_or_else(|| "Audio message".to_owned());
            let toggle = OpenAttachment(reference.clone());
            let transcribe = Transcribe(reference);
            let transcript = media.transcripts.get(&attachment.guid).cloned();
            let fill = if message.from_me { theme.accent } else { theme.surface.raised_hover };
            let pill = h_flex()
                .id(id)
                .gap_2()
                .h(px(40.))
                .px_3()
                .rounded_full()
                .bg(fill)
                .context_menu(menu)
                .child(
                    icon_button(format!("play-{}", attachment.guid), if playing { Icon::Pause } else { Icon::Play })
                        .tooltip(if playing { "Stop" } else { "Play" })
                        .on_click(move |_, window, cx| crate::ui::dispatch(window, cx, toggle.clone())),
                )
                .child(text(duration).tabular_nums().text_color(if message.from_me {
                    theme.text.on_accent
                } else {
                    theme.text.primary
                }));
            let (line, tone) = match &transcript {
                Some(Translation::Loading) => ("Transcribing\u{2026}".to_owned(), LabelTone::Secondary),
                Some(Translation::Error(error)) => (error.clone(), LabelTone::Danger),
                Some(Translation::Text(text)) => (text.clone(), LabelTone::Primary),
                None => (String::new(), LabelTone::Primary),
            };
            let pill = match tail {
                Some(side) => with_tail(pill.into_any_element(), side, fill),
                None => pill.into_any_element(),
            };
            v_flex()
                .gap_1()
                .when(message.from_me, |col| col.items_end())
                .when(!message.from_me, |col| col.items_start())
                .child(pill)
                .when(assistant && transcript.is_none(), |col| {
                    col.child(
                        link(format!("transcribe-{}", attachment.guid), "Transcribe")
                            .keep_internal()
                            .on_click(move |_, window, cx| crate::ui::dispatch(window, cx, transcribe.clone())),
                    )
                })
                .when(!line.is_empty(), |col| col.child(label(line).tone(tone).wraps().selectable()))
                .into_any_element()
        }
        Kind::File => {
            // Opens in Quick Look, as in Messages; the menu still has Open.
            let open = ShowAttachment(reference);
            let block = h_flex()
                .id(id)
                .gap_2()
                .max_w(px(MEDIA_MAX_WIDTH as f32))
                .px_3()
                .py_2()
                .rounded_xl()
                .bg(theme.surface.raised_hover)
                .on_tap(move |_, window, cx| crate::ui::dispatch(window, cx, open.clone()))
                .context_menu(menu)
                .child(icon(Icon::File, 20.).text_color(theme.text.secondary))
                .child(
                    v_flex().min_w_0().child(text(attachment.name.clone()).font_weight(FontWeight::SEMIBOLD)).child(
                        text(format_bytes(attachment.bytes))
                            .text_size(theme.type_scale.caption)
                            .text_color(theme.text.secondary),
                    ),
                )
                .into_any_element();
            match tail {
                Some(side) => with_tail(block, side, theme.surface.raised_hover),
                None => block,
            }
        }
    }
}

fn placeholder(w: f32, h: f32, label_text: String, theme: &Theme) -> AnyElement {
    div()
        .w(px(w))
        .h(px(h))
        .rounded_lg()
        .bg(theme.surface.raised_hover)
        .flex()
        .items_center()
        .justify_center()
        .px_3()
        .child(text(label_text).text_size(theme.type_scale.caption).text_color(theme.text.secondary))
        .into_any_element()
}

fn photo_grid(data: &MessageRowData, photos: &[Attachment], media: &Media, theme: &Theme) -> AnyElement {
    let shown = photos.len().min(GRID_MAX_TILES);
    let more = photos.len().saturating_sub(GRID_MAX_TILES);
    let tiles = photos.iter().take(shown).enumerate().map(|(i, photo)| {
        let reference = AttachmentRef { at: at(data), attachment: photo.guid.clone() };
        let show = ShowAttachment(reference);
        let picture = match photo.local_path.as_ref().and_then(|p| ui::image_source(&p.to_string_lossy())) {
            Some(source) => image(source).alt(photo.name.clone()).size(px(GRID_SIDE)).rounded_md().into_any_element(),
            None => placeholder(
                GRID_SIDE,
                GRID_SIDE,
                if media.failed.contains(&photo.guid) { "Could not load.".into() } else { "Loading\u{2026}".into() },
                theme,
            ),
        };
        div()
            .id(format!("tile-{}", photo.guid))
            .size(px(GRID_SIDE))
            .on_tap(move |_, window, cx| crate::ui::dispatch(window, cx, show.clone()))
            .child(picture)
            .when(i + 1 == shown && more > 0, |tile| {
                tile.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .bg(theme.scrim)
                        .child(text(format!("+{more}")).text_size(px(22.)).text_color(theme.text.on_accent)),
                )
            })
    });
    div().flex().flex_row().flex_wrap().gap(px(2.)).w(px(GRID_SIDE * 2. + 2.)).children(tiles).into_any_element()
}

fn link_card(href: &str, host: &str, preview: Option<&messages_core::UrlPreview>, theme: &Theme) -> AnyElement {
    let open = OpenLink(href.to_owned());
    let title = preview.and_then(|p| p.title.clone()).unwrap_or_else(|| href.to_owned());
    let site = preview.and_then(|p| p.site_name.clone()).unwrap_or_else(|| host.to_owned());
    let picture = preview.and_then(|p| p.image_path.as_deref()).and_then(ui::image_source);
    v_flex()
        .id(format!("link-{href}"))
        .w(px(LINK_CARD_WIDTH))
        .rounded_xl()
        .bg(theme.surface.raised_hover)
        .on_tap(move |_, window, cx| crate::ui::dispatch(window, cx, open.clone()))
        .when_some(picture, |card, picture| {
            card.child(image(picture).w(px(LINK_CARD_WIDTH)).h(px(150.)).rounded_t_xl())
        })
        .child(
            v_flex()
                .px_3()
                .py_2()
                .gap_0p5()
                .child(text(title).font_weight(FontWeight::SEMIBOLD))
                .child(text(site).text_size(theme.type_scale.caption).text_color(theme.text.secondary)),
        )
        .into_any_element()
}

/// The whole row: the sender's name and avatar in a group, the quoted message, the blocks with
/// the tail on the last one at a run's end, reactions, translation, replies and the receipt.
pub fn render(data: &MessageRowData, props: &RowProps, media: &Media, theme: &Theme) -> AnyElement {
    let message = &data.message;
    let from_me = message.from_me;
    let side = if from_me { BubbleSide::Sent } else { BubbleSide::Received };
    let blocks = blocks_of(message);
    let groups = tapback_groups(&message.tapbacks);
    let time: SharedString = format_time(message.date).into();
    let failed = failed(message);
    let show_avatar = props.is_group && !from_me;
    // My messages that went out as SMS or RCS fill green, as in Messages.
    let sms = from_me && message.service != Service::IMessage;
    let last_block = blocks_of(message).len().saturating_sub(1);
    let tail_here = |index: usize| index == last_block && data.position.ends_run();

    let menu_data = data.clone();
    let menu_props = props.clone();
    let row_menu = move |menu: ContextMenu, _: &mut Window, _: &mut App| message_menu(menu, &menu_data, &menu_props);

    let mut column: Vec<AnyElement> = Vec::new();
    if let Some(quote) = props.quote.as_ref().filter(|_| data.show_quote) {
        let jump = JumpTo(MessageRef { chat: props.primary.clone(), message: quote.target.clone() });
        column.push(
            v_flex()
                .id(format!("quote-{}", data.key))
                .pl_2()
                .mb_1()
                .border_l_2()
                .border_color(theme.border.strong)
                .on_tap(move |_, window, cx| crate::ui::dispatch(window, cx, jump.clone()))
                .child(
                    text(quote.who.clone())
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(theme.type_scale.caption)
                        .text_color(theme.text.secondary),
                )
                .child(
                    text(ui::clamp_preview(&quote.body, 120))
                        .text_size(theme.type_scale.caption)
                        .text_color(theme.text.secondary),
                )
                .into_any_element(),
        );
    }

    if emoji_only(message) {
        column.push(
            chat_bubble(message.text.trim().to_owned(), side)
                .id(format!("bubble-{}", data.key))
                .reactions(reactions(&groups))
                .time(time.clone())
                .into_any_element(),
        );
    } else {
        let last = blocks.len().saturating_sub(1);
        // Reactions sit on the first text bubble; a message without one gets its own pills.
        let first_text = blocks.iter().position(|b| matches!(b, Block::Text { .. }));
        if first_text.is_none() && !groups.is_empty() {
            column.push(
                h_flex()
                    .gap_1()
                    .children(groups.iter().map(|g| {
                        let label = if g.count > 1 { format!("{} {}", g.glyph, g.count) } else { g.glyph.clone() };
                        div()
                            .px_2()
                            .rounded_full()
                            .bg(if g.mine { theme.accent } else { theme.surface.raised_hover })
                            .child(text(label).text_color(if g.mine {
                                theme.text.on_accent
                            } else {
                                theme.text.primary
                            }))
                    }))
                    .into_any_element(),
            );
        }
        for (index, block) in blocks.iter().enumerate() {
            let position = if index == last {
                data.position
            } else {
                match data.position {
                    RunPosition::Single | RunPosition::First => RunPosition::First,
                    RunPosition::Middle | RunPosition::Last => RunPosition::Middle,
                }
            };
            let element = match block {
                Block::Text { subject, body } => {
                    let rich = match subject {
                        Some(subject) => {
                            let mut spans = vec![span(subject.clone()).bold()];
                            if !body.text().is_empty() {
                                spans.push(span("\n"));
                                spans.extend(body.spans().iter().cloned());
                            }
                            RichText::new(spans)
                        }
                        None => body.clone(),
                    };
                    let mut bubble = chat_bubble(String::new(), side)
                        .rich(rich)
                        .id(format!("bubble-{}-{index}", data.key))
                        .position(position)
                        .time(time.clone());
                    if Some(index) == first_text {
                        bubble = bubble.reactions(reactions(&groups));
                    }
                    if sms {
                        bubble = bubble.role(FillRole::Success);
                    }
                    bubble.into_any_element()
                }
                // Every other block that ends a run carries the tail too, in its own fill or cut
                // from its picture.
                // The grid's corner is a picture, not a fill, so a coloured lobe would not continue
                // it; it stays without a tail.
                Block::Photos(photos) => photo_grid(data, photos, media, theme),
                Block::Attachment(attachment) => attachment_block(
                    data,
                    attachment,
                    media,
                    props.assistant,
                    tail_here(index).then_some(side),
                    theme,
                ),
                Block::Link { href, host, matches } => {
                    let card = link_card(href, host, matches.then_some(()).and(message.url_preview.as_ref()), theme);
                    if tail_here(index) { with_tail(card, side, theme.surface.raised_hover) } else { card }
                }
                Block::Preview => {
                    let preview = message.url_preview.as_ref();
                    let href = preview.map(|p| p.url.clone()).unwrap_or_default();
                    let card = link_card(&href, &host_of(&href), preview, theme);
                    if tail_here(index) { with_tail(card, side, theme.surface.raised_hover) } else { card }
                }
            };
            column.push(element);
        }
    }

    if let Some(translation) = &props.translation {
        let (body, color) = match translation {
            Translation::Loading => ("Translating\u{2026}".to_owned(), theme.text.secondary),
            Translation::Text(text) => (text.clone(), theme.text.secondary),
            Translation::Error(error) => (error.clone(), theme.text.danger),
        };
        column.push(text(body).text_size(theme.type_scale.caption).text_color(color).px_1().into_any_element());
    }
    if let Some(replies) = &props.replies {
        let open = OpenThread(at(data));
        column.push(
            div()
                .id(format!("replies-{}", data.key))
                .px_1()
                .on_tap(move |_, window, cx| crate::ui::dispatch(window, cx, open.clone()))
                .child(link(format!("replies-link-{}", data.key), replies.clone()).keep_internal())
                .into_any_element(),
        );
    }
    let effect = message.effect.as_deref().map(|effect| format!("Sent with {}", effect_name(effect)));
    if message.date_edited.is_some() || effect.is_some() || data.receipt.is_some() {
        let caption = |s: String, color: Hsla| text(s).text_size(theme.type_scale.caption).text_color(color);
        let notify = data.notify && props.capabilities.focus_status;
        let (retry, notify_anyway) = (Retry(at(data)), NotifyAnyway(at(data)));
        column.push(
            h_flex()
                .id(format!("footer-{}", data.key))
                .gap_1()
                .px_1()
                .when(from_me, |row| row.justify_end())
                .when(message.date_edited.is_some(), |row| row.child(caption("Edited".into(), theme.text.tertiary)))
                .when_some(effect, |row, effect| row.child(caption(effect, theme.text.tertiary)))
                .when_some(data.receipt.clone(), |row, receipt| {
                    row.child(
                        caption(receipt.text, if receipt.failed { theme.text.danger } else { theme.text.secondary })
                            .id(format!("receipt-{}", data.key))
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                })
                .when(failed, |row| {
                    row.child(
                        link(format!("retry-{}", data.key), "Try Again")
                            .on_click(move |_, window, cx| crate::ui::dispatch(window, cx, retry.clone())),
                    )
                })
                .when(notify, |row| {
                    row.child(
                        link(format!("notify-{}", data.key), "Notify Anyway")
                            .text_size(theme.type_scale.caption)
                            .on_click(move |_, window, cx| crate::ui::dispatch(window, cx, notify_anyway.clone())),
                    )
                })
                .into_any_element(),
        );
    }

    let sender =
        (data.show_sender).then(|| message.sender.as_ref().map(|s| handle_name(s).to_owned())).flatten().map(|name| {
            text(name).text_size(theme.type_scale.caption).text_color(theme.text.secondary).ml(px(if show_avatar {
                AVATAR_COLUMN + 8. + 12.
            } else {
                12.
            }))
        });
    let avatar_slot = show_avatar.then(|| match (&message.sender, data.position.ends_run()) {
        (Some(sender), true) => ui::avatar_of(sender, AVATAR_COLUMN).into_any_element(),
        _ => div().w(px(AVATAR_COLUMN)).flex_none().into_any_element(),
    });
    let gap = if data.position.starts_run() { 10. } else { 2. };
    let can_react = props.capabilities.reactions && !failed;
    let open_picker = OpenTapbacks(at(data));
    let mine = message.tapbacks.iter().find(|t| t.from_me).map(|t| t.kind);
    let picker = (props.picker && can_react).then(|| {
        let buttons = messages_core::TapbackKind::NAMED.into_iter().map(|kind| {
            let react = React { at: at(data), kind };
            let glyph = messages_core::tapback_glyph(kind, None);
            button(format!("tapback-{}", kind.as_str()), glyph)
                .tooltip(tapback_name(kind))
                .when(mine == Some(kind), |b| b.primary())
                .on_click(move |_, window, cx| {
                    crate::ui::dispatch(window, cx, react.clone());
                    crate::ui::dispatch(window, cx, CloseTapbacks);
                })
        });
        popover(format!("tapbacks-{}", data.key))
            .placement(Placement::Above)
            .p_1()
            .on_dismiss(|_, window, cx| crate::ui::dispatch(window, cx, CloseTapbacks))
            .child(h_flex().gap_1().children(buttons))
    });
    v_flex()
        .id(format!("row-{}", data.key))
        .w_full()
        .px_4()
        .pt(px(gap))
        .when(from_me, |col| col.items_end())
        .when(props.highlighted, |col| col.bg(theme.selection))
        .context_menu(row_menu)
        .when(can_react, |col| {
            col.on_double_tap(move |_: &DoubleTapEvent, window, cx| {
                crate::ui::dispatch(window, cx, open_picker.clone())
            })
        })
        .children(picker)
        .children(sender)
        .child(
            h_flex()
                .w_full()
                .items_end()
                .gap_2()
                .when(from_me, |row| row.justify_end())
                .children(avatar_slot)
                .when(from_me, |row| row.child(time_column(&time, true, props.hovered, theme)))
                .child(
                    v_flex()
                        .max_w(relative(0.62))
                        .min_w_0()
                        .gap_1()
                        .when(from_me, |col| col.items_end())
                        .when(!from_me, |col| col.items_start())
                        .children(column),
                )
                .when(!from_me, |row| row.child(time_column(&time, false, props.hovered, theme))),
        )
        .into_any_element()
}

/// The message's time on the bubble's inner side, there only while the row is hovered. The
/// column keeps its width either way, so bubbles never shift under the pointer.
fn time_column(time: &SharedString, align_right: bool, shown: bool, theme: &Theme) -> Div {
    div()
        .w(px(TIME_COLUMN))
        .flex_none()
        .pb(px(5.))
        .when(align_right, |col| col.flex().justify_end())
        .when(!shown, |col| col.opacity(0.))
        .child(
            text(time.clone())
                .tabular_nums()
                .text_size(theme.type_scale.caption)
                .text_color(theme.text.secondary)
                .whitespace_nowrap(),
        )
}

/// The typing bubble: three dots, `lit` the bright one, stepping while someone types.
pub fn typing_row(lit: usize, who: Option<&messages_core::Handle>, is_group: bool, theme: &Theme) -> AnyElement {
    h_flex()
        .id("typing")
        .w_full()
        .px_4()
        .pt(px(10.))
        .items_end()
        .gap_2()
        .when(is_group, |row| {
            row.child(match who {
                Some(who) => ui::avatar_of(who, AVATAR_COLUMN).into_any_element(),
                None => div().w(px(AVATAR_COLUMN)).into_any_element(),
            })
        })
        .child(h_flex().gap(px(5.)).h(px(34.)).px_3().rounded(px(18.)).bg(theme.surface.raised_hover).children(
            (0..3).map(|index| {
                div().size(px(7.)).rounded_full().bg(theme.text.secondary).opacity(if index == lit { 1. } else { 0.35 })
            }),
        ))
        .into_any_element()
}

/// A date separator or an event, centred in small type; `strong` bytes are the day.
pub fn caption_row(key: &str, caption: &str, strong: usize, theme: &Theme) -> AnyElement {
    let strong = strong.min(caption.len());
    let (day, rest) = caption.split_at(strong);
    h_flex()
        .id(format!("caption-{key}"))
        .w_full()
        .justify_center()
        .pt_4()
        .pb_2()
        .gap_1()
        .when(!day.is_empty(), |row| {
            row.child(
                text(day.to_owned())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(theme.type_scale.caption)
                    .text_color(theme.text.secondary),
            )
        })
        // Secondary, not tertiary: the time is read, and tertiary falls under 4.5:1 on GPUI's
        // and matugen's surfaces.
        .child(text(rest.trim().to_owned()).text_size(theme.type_scale.caption).text_color(theme.text.secondary))
        .into_any_element()
}

/// The day half of a separator: "Today" of "Today 9:41 AM".
pub fn separator_day(label: &str) -> &str {
    match label.find(" at ") {
        Some(end) => &label[..end],
        None => label.split_once(' ').map_or(label, |(day, _)| day),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_day_half_of_a_separator_is_what_gets_the_weight() {
        assert_eq!(separator_day("Today 9:41 AM"), "Today");
        assert_eq!(separator_day("Monday 08:15"), "Monday");
        assert_eq!(separator_day("Sep 2, 2025 at 4:30 PM"), "Sep 2, 2025");
    }

    #[test]
    fn links_become_cards_between_text() {
        let message = Message { text: "look https://example.com/a here".into(), ..Message::default() };
        let blocks = blocks_of(&message);
        assert_eq!(blocks.len(), 3);
        assert!(matches!(&blocks[1], Block::Link { host, .. } if host == "example.com"));
    }

    #[test]
    fn urls_compare_without_protocol_slash_or_case() {
        assert!(same_url("https://Example.com/a/", "http://example.com/a"));
        assert!(!same_url("https://example.com/a", "https://example.com/b"));
    }

    #[test]
    fn durations_read_as_minutes_and_seconds() {
        assert_eq!(format_duration(61_400.), "1:01");
    }

    #[test]
    fn media_boxes_fit_the_cap_and_fall_back_to_four_by_three() {
        let tall = Attachment { width: Some(1000), height: Some(4000), ..attachment() };
        let (w, h) = media_dims(&tall);
        assert!(w <= MEDIA_MAX_WIDTH as f32 && h <= MEDIA_MAX_HEIGHT as f32);
        assert_eq!(media_dims(&attachment()), (320., 240.));
    }

    fn attachment() -> Attachment {
        Attachment {
            guid: "a".into(),
            name: "a.jpg".into(),
            mime: "image/jpeg".into(),
            bytes: 1,
            width: None,
            height: None,
            measured: false,
            is_sticker: false,
            local_path: None,
            hidden: false,
            duration_ms: None,
        }
    }
}
