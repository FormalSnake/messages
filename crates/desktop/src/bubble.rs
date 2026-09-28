//! One entity per message row, watching `Message(guid)`.
//! Everything the row shows that depends only on the message (blocks, rich
//! text spans, labels, tapback groups) is derived in `Model::new` when the
//! message's `Arc` changes; render lays out what is already there.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::Root;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::format::{format_time, is_emoji_only};
use messages_core::open::{TextSegment, split_links};

use crate::selectable::{order, selectable};
use messages_core::{Attachment, Capabilities, DeliveryState, Message, MessagePart, MessagesStore, RichRun, Service, TextEffect, delivery_state, handle_name};

use crate::app::AppRoot;
use crate::attachments::{self, Kind, Media, TailFill, kind_of, same_url, tail_box};
use crate::bridge::{Bridge, Topic};
use crate::icons::{Icon, IconName};
use crate::menus::{MenuItem, MenuRequest};
use crate::motion::{DURATION_BASE, eased_since};
use crate::theme::{BUBBLE_MAX_WIDTH, Palette, THREAD_INSET, Theme, TypeStyle, font_emoji, radius, spacing, tabular, type_scale, with_alpha};
use crate::thread::Thread;
use crate::thread_rows::{EDIT_WINDOW_MS, MessageRowData, TapbackGroup, UNSEND_WINDOW_MS, bubble_radius, effect_name, now_ms, tapback_groups};

/// Tapbacks hang off the bubble's outer top corner and overlap its radius,
/// never the first line, so the bubble carries this much top margin.
const TAPBACK_LIFT: f32 = 14.;
const GAP_IN_RUN: f32 = 2.;
const GAP_BETWEEN_RUNS: f32 = 10.;
pub const AVATAR_COLUMN: f32 = 28.;
const TIME_COLUMN: f32 = 54.;
/// A tapback landing this long after its row mounted is news, and fades in.
const FRESH_AFTER: Duration = Duration::from_millis(500);
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const ROW_GROUP: &str = "message-row";

const BIG: TypeStyle = TypeStyle { font_size: px(22.), line_height: px(28.), font_weight: 400. };
const SMALL: TypeStyle = TypeStyle { font_size: px(11.), line_height: px(15.), font_weight: 400. };

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quote {
    pub target: String,
    pub who: SharedString,
    pub body: SharedString,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Translation {
    Loading,
    Text(SharedString),
    Error(SharedString),
}

/// What the thread knows about a row that the message alone does not say.
#[derive(Clone, Debug, PartialEq)]
pub struct RowProps {
    /// The conversation's primary guid: drafts, replies and store calls key on it.
    pub primary: String,
    pub is_group: bool,
    pub capabilities: Capabilities,
    pub assistant: bool,
    pub quote: Option<Quote>,
    pub reply_count: usize,
    /// "1 reply" / "N replies", formatted by the thread.
    pub replies: Option<SharedString>,
    pub highlighted: bool,
    pub translation: Option<Translation>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    accent: bool,
}

impl Span {
    fn of(run: &RichRun) -> Self {
        Span { bold: run.bold || run.mention.is_some(), italic: run.italic, underline: run.underline || run.link.is_some(), strike: run.strike, accent: run.mention.is_some() || run.link.is_some() || run.underline }
    }

    fn highlight(self, accent: Hsla, color: Hsla) -> HighlightStyle {
        HighlightStyle {
            color: Some(if self.accent { accent } else { color }),
            font_weight: self.bold.then_some(FontWeight::BOLD),
            font_style: self.italic.then_some(FontStyle::Italic),
            underline: self.underline.then_some(UnderlineStyle { thickness: px(1.), color: Some(accent), wavy: false }),
            strikethrough: self.strike.then_some(StrikethroughStyle { thickness: px(1.), color: Some(color.opacity(0.7)) }),
            ..HighlightStyle::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TextBody {
    Empty,
    Plain(SharedString),
    Rich { text: SharedString, spans: Vec<(Range<usize>, Span)>, size: Option<(Pixels, Pixels)> },
    /// Runs of different sizes cannot share one text layout, so each line is a
    /// wrapping row of words, each word keeping its trailing space.
    Chunks(Vec<Vec<(SharedString, Span, Option<(Pixels, Pixels)>)>>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Photos(Vec<Attachment>),
    Attachment(Attachment),
    Link { href: SharedString, host: SharedString, matches: bool },
    Text { subject: Option<SharedString>, body: TextBody },
    Preview,
}

fn size_of(effect: Option<TextEffect>) -> Option<(Pixels, Pixels)> {
    match effect {
        Some(TextEffect::Big) => Some((BIG.font_size, BIG.line_height)),
        Some(TextEffect::Small) => Some((SMALL.font_size, SMALL.line_height)),
        _ => None,
    }
}

/// The attributed body only marks links someone typed as a link, so bare URLs
/// get the same pass the plain text path gives them.
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

fn words(segment: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut after_space = true;
    for (index, ch) in segment.char_indices() {
        let space = ch.is_whitespace();
        if !space && after_space && index > start {
            out.push(&segment[start..index]);
            start = index;
        }
        after_space = space;
    }
    if start < segment.len() {
        out.push(&segment[start..]);
    }
    out
}

fn rich_body(runs: &[RichRun]) -> TextBody {
    let sized: HashSet<Option<(i32, i32)>> =
        runs.iter().filter(|run| !run.text.trim().is_empty()).map(|run| size_of(run.effect).map(|(size, line)| (f32::from(size) as i32, f32::from(line) as i32))).collect();
    if sized.len() > 1 {
        let mut lines: Vec<Vec<(SharedString, Span, Option<(Pixels, Pixels)>)>> = vec![Vec::new()];
        for run in runs {
            for (index, segment) in run.text.split('\n').enumerate() {
                if index > 0 {
                    lines.push(Vec::new());
                }
                let line = lines.last_mut().expect("one line at least");
                for word in words(segment) {
                    line.push((SharedString::from(word.to_owned()), Span::of(run), size_of(run.effect)));
                }
            }
        }
        return TextBody::Chunks(lines);
    }
    let mut text = String::new();
    let mut spans = Vec::new();
    for run in runs {
        let start = text.len();
        text.push_str(&run.text);
        let span = Span::of(run);
        if span != Span::default() {
            spans.push((start..text.len(), span));
        }
    }
    let size = runs.iter().find(|run| !run.text.trim().is_empty()).and_then(|run| size_of(run.effect));
    if spans.is_empty() && size.is_none() { TextBody::Plain(text.into()) } else { TextBody::Rich { text: text.into(), spans, size } }
}

pub fn emoji_only(message: &Message) -> bool {
    !message.attachments.iter().any(|item| !item.hidden) && is_emoji_only(&message.text, 3)
}

/// The blocks a body paints, in order: the photo grid, the attachments the
/// attributed body did not place, then the parts with every link split out
/// into its own card, and the rich preview last when no link carried it.
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
    let photos: Vec<Attachment> =
        if message.sticker_for.is_some() { Vec::new() } else { visible.iter().filter(|item| item.mime.starts_with("image/") && !item.is_sticker).map(|item| (*item).clone()).collect() };
    let grid: Option<HashSet<String>> = (photos.len() >= 2).then(|| photos.iter().map(|item| item.guid.clone()).collect());
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
        let host = attachments::host_of(&href);
        blocks.push(Block::Link { href: href.into(), host: host.into(), matches });
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
        for segment in split_links(&message.text) {
            match segment {
                TextSegment::Link { href, .. } => push_link(&mut blocks, href),
                TextSegment::Text(value) if !value.trim().is_empty() => blocks.push(Block::Text { subject: None, body: TextBody::Plain(value.into()) }),
                TextSegment::Text(_) => {}
            }
        }
    }

    if let Some(subject) = message.subject.as_ref().filter(|subject| !subject.is_empty()) {
        match blocks.iter_mut().find(|block| matches!(block, Block::Text { .. })) {
            Some(Block::Text { subject: slot, .. }) => *slot = Some(subject.clone().into()),
            _ => blocks.insert(0, Block::Text { subject: Some(subject.clone().into()), body: TextBody::Empty }),
        }
    }
    if preview.is_some() && !preview_rendered {
        blocks.push(Block::Preview);
    }
    blocks
}

/// Everything derived from the message itself.
pub struct Model {
    pub message: Arc<Message>,
    time: SharedString,
    sender_name: Option<SharedString>,
    emoji: Option<SharedString>,
    blocks: Vec<Block>,
    tapbacks: Vec<TapbackGroup>,
    effect: Option<SharedString>,
    links: Vec<String>,
    tapback_counts: Vec<SharedString>,
    pub(crate) labels: HashMap<String, attachments::Labels>,
    /// "+N" on the last grid tile when a message has more photos than tiles.
    pub(crate) more: Option<SharedString>,
}

impl Model {
    pub fn new(message: Arc<Message>) -> Self {
        let tapbacks = tapback_groups(&message.tapbacks);
        let photos = message.attachments.iter().filter(|item| !item.hidden && item.mime.starts_with("image/") && !item.is_sticker).count();
        Model {
            tapback_counts: tapbacks.iter().map(|group| group.count.to_string().into()).collect(),
            labels: message.attachments.iter().map(|item| (item.guid.clone(), attachments::labels_for(item))).collect(),
            more: (message.sticker_for.is_none() && photos > 4).then(|| format!("+{}", photos - 4).into()),
            time: format_time(message.date).into(),
            sender_name: message.sender.as_ref().map(|sender| handle_name(sender).to_owned().into()),
            emoji: emoji_only(&message).then(|| message.text.trim().to_owned().into()),
            blocks: blocks_of(&message),
            tapbacks,
            effect: message.effect.as_deref().map(|effect| format!("Sent with {}", effect_name(effect)).into()),
            links: split_links(&message.text)
                .into_iter()
                .filter_map(|segment| match segment {
                    TextSegment::Link { href, .. } => Some(href),
                    TextSegment::Text(_) => None,
                })
                .collect(),
            message,
        }
    }
}

pub fn open_menu(request: MenuRequest, window: &mut Window, cx: &mut App) {
    let Some(Some(root)) = window.root::<Root>() else { return };
    let Ok(app) = root.read(cx).view().clone().downcast::<AppRoot>() else { return };
    AppRoot::open_menu(&app, request, window, cx);
}

/// What the thread's list needs to lay a row out without rendering it. A
/// cached view takes its size from outside and lays its content out inside
/// that box, so the list hands a settled row its last natural height.
#[derive(Default)]
pub(crate) struct RowLayout {
    /// Width and natural height from the row's last fresh layout.
    pub(crate) measured: Cell<Option<Size<Pixels>>>,
    /// Height the list gave the cached row this frame; None when uncached.
    pub(crate) given: Cell<Option<Pixels>>,
    /// Set when the row's content may have changed height; the next layout
    /// measures the content instead of trusting `measured`.
    pub(crate) stale: Cell<bool>,
}

pub struct MessageRow {
    pub(crate) layout: Rc<RowLayout>,
    pub(crate) store: MessagesStore,
    thread: WeakEntity<Thread>,
    pub(crate) data: MessageRowData,
    pub(crate) props: RowProps,
    pub(crate) model: Model,
    pub(crate) media: Media,
    entered_at: Option<Instant>,
    mounted_at: Instant,
    /// When each tapback glyph first showed, so a pill that lands on an open thread fades in.
    tapback_seen: HashMap<String, Option<Instant>>,
    last_click: Option<Instant>,
}

impl MessageRow {
    pub fn new(store: MessagesStore, thread: WeakEntity<Thread>, data: MessageRowData, props: RowProps, entering: bool, cx: &mut Context<Self>) -> Self {
        { let entity = cx.entity().downgrade().into(); Bridge::watch(cx, Topic::Message(data.message.guid.clone()), entity); }
        let model = Model::new(data.message.clone());
        let tapback_seen = model.tapbacks.iter().map(|group| (group.glyph.clone(), None)).collect();
        let mut row = MessageRow {
            layout: Rc::default(),
            store,
            thread,
            data,
            props,
            model,
            media: Media::default(),
            entered_at: entering.then(Instant::now),
            mounted_at: Instant::now(),
            tapback_seen,
            last_click: None,
        };
        Media::sync(&mut row, cx);
        row
    }

    /// Takes what the thread computed for this row. The row renders in the
    /// same frame as the thread, so no notify is needed here.
    pub fn set(&mut self, data: MessageRowData, props: RowProps, cx: &mut Context<Self>) {
        let changed = !Arc::ptr_eq(&data.message, &self.model.message);
        if changed && data.message.guid != self.model.message.guid {
            let entity: AnyWeakEntity = cx.entity().downgrade().into();
            Bridge::unwatch(cx, &Topic::Message(self.model.message.guid.clone()), &entity);
            Bridge::watch(cx, Topic::Message(data.message.guid.clone()), entity);
        }
        self.data = data;
        self.props = props;
        self.layout.stale.set(true);
        if changed {
            self.model = Model::new(self.data.message.clone());
            Media::sync(self, cx);
        }
        cx.notify();
    }

    fn capabilities(&self) -> Capabilities {
        self.props.capabilities
    }

    fn failed(&self) -> bool {
        delivery_state(&self.model.message) == DeliveryState::Failed
    }

    fn fill(&self, palette: &Palette) -> Hsla {
        if !self.model.message.from_me {
            palette.received
        } else if self.model.message.service == Service::IMessage {
            palette.imessage
        } else {
            palette.sms
        }
    }

    fn tapback_item(&self) -> Option<MenuItem> {
        if !self.capabilities().reactions || self.failed() {
            return None;
        }
        let mine = self.model.message.tapbacks.iter().find(|tapback| tapback.from_me).map(|tapback| tapback.kind);
        let store = self.store.clone();
        let (chat, guid) = (self.props.primary.clone(), self.model.message.guid.clone());
        Some(MenuItem::Tapbacks {
            chat_guid: chat.clone(),
            message_guid: guid.clone(),
            mine,
            on_select: std::rc::Rc::new(move |kind, _, _| {
                let (store2, chat, guid) = (store.clone(), chat.clone(), guid.clone());
                store.spawn(async move { store2.react(&chat, &guid, kind, None).await });
            }),
        })
    }

    fn open_picker(&self, position: Point<Pixels>, window: &mut Window, cx: &mut App) {
        let Some(item) = self.tapback_item() else { return };
        open_menu(MenuRequest::at(point(position.x, position.y - spacing::X2), vec![item]).above().centered(), window, cx);
    }

    fn on_bubble_click(&mut self, event: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        let double = event.click_count() >= 2 || self.last_click.is_some_and(|last| now - last < DOUBLE_CLICK);
        self.last_click = if double { None } else { Some(now) };
        if double {
            self.open_picker(event.position(), window, cx);
        }
    }

    fn open_message_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.model.message.clone();
        let caps = self.capabilities();
        let failed = self.failed();
        let age = now_ms() - message.date;
        let primary = self.props.primary.clone();
        let store = self.store.clone();
        let this = cx.entity().downgrade();
        let mut items: Vec<MenuItem> = self.tapback_item().into_iter().collect();
        if failed {
            let (store, primary, guid) = (store.clone(), primary.clone(), message.guid.clone());
            items.push(MenuItem::item("Try again", move |_, _| store.retry(&primary, &guid)).icon(IconName::Refresh));
        }
        if caps.replies && !failed {
            let (store, primary, guid) = (store.clone(), primary.clone(), message.guid.clone());
            items.push(MenuItem::item("Reply", move |_, _| store.set_replying_to(&primary, Some(&guid))).icon(IconName::Reply));
        }
        if !message.text.is_empty() {
            let text = message.text.clone();
            items.push(MenuItem::item("Copy", move |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))).icon(IconName::Copy));
        }
        if self.props.assistant && !message.text.trim().is_empty() {
            let label = if self.props.translation.is_some() { "Hide translation" } else { "Translate" };
            let (thread, guid, text) = (self.thread.clone(), message.guid.clone(), message.text.clone());
            items.push(
                MenuItem::item(label, move |_, cx| {
                    let _ = thread.update(cx, |thread, cx| thread.toggle_translation(&guid, &text, cx));
                })
                .icon(IconName::Sparkles),
            );
        }
        for href in &self.model.links {
            let href = href.clone();
            items.push(MenuItem::item("Open link", move |_, _| messages_core::open::open_external(&href)).icon(IconName::Open));
        }
        if let Some(file) = message.attachments.iter().find(|item| item.local_path.as_ref().is_some_and(|path| !attachments::is_data_url(path))) {
            let (open, copy) = (file.clone(), file.clone());
            let this_open = this.clone();
            items.push(
                MenuItem::item("Open attachment", move |_, cx| {
                    let _ = this_open.update(cx, |row, cx| attachments::open_attachment(row, &open, cx));
                })
                .icon(IconName::Image),
            );
            let this_copy = this.clone();
            items.push(
                MenuItem::item(if file.mime.starts_with("image/") { "Copy image" } else { "Copy file" }, move |_, cx| {
                    let _ = this_copy.update(cx, |row, cx| attachments::copy_attachment(row, &copy, cx));
                })
                .icon(IconName::Copy),
            );
        }
        if message.from_me && !failed && message.service == Service::IMessage && (caps.edit || caps.unsend) {
            items.push(MenuItem::Separator);
            if caps.edit {
                let (store, primary, guid) = (store.clone(), primary.clone(), message.guid.clone());
                items.push(
                    MenuItem::item("Edit", move |_, _| store.set_editing(&primary, Some(&guid)))
                        .icon(IconName::Edit)
                        .disabled(age > EDIT_WINDOW_MS || !message.attachments.is_empty()),
                );
            }
            if caps.unsend {
                items.push(self.unsend_item(age));
            }
        }
        open_menu(MenuRequest::at(position, items), window, cx);
    }

    fn unsend_item(&self, age: i64) -> MenuItem {
        let (store, primary, guid) = (self.store.clone(), self.props.primary.clone(), self.model.message.guid.clone());
        MenuItem::item("Undo send", move |_, _| {
            let (store2, primary, guid) = (store.clone(), primary.clone(), guid.clone());
            store.spawn(async move { store2.unsend(&primary, &guid).await });
        })
        .icon(IconName::Trash)
        .danger()
        .disabled(age > UNSEND_WINDOW_MS)
    }

    pub fn open_attachment_menu(&mut self, guid: &str, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(attachment) = self.model.message.attachments.iter().find(|item| item.guid == guid).cloned() else { return };
        let message = self.model.message.clone();
        let caps = self.capabilities();
        let this = cx.entity().downgrade();
        let mut items: Vec<MenuItem> = self.tapback_item().into_iter().collect();
        let (open, copy) = (attachment.clone(), attachment.clone());
        let this_open = this.clone();
        items.push(
            MenuItem::item("Open", move |_, cx| {
                let _ = this_open.update(cx, |row, cx| attachments::open_attachment(row, &open, cx));
            })
            .icon(IconName::Open),
        );
        if caps.replies {
            let (store, primary, guid) = (self.store.clone(), self.props.primary.clone(), message.guid.clone());
            items.push(MenuItem::item("Reply", move |_, _| store.set_replying_to(&primary, Some(&guid))).icon(IconName::Reply));
        }
        items.push(
            MenuItem::item(if attachment.mime.starts_with("image/") { "Copy image" } else { "Copy file" }, move |_, cx| {
                let _ = this.update(cx, |row, cx| attachments::copy_attachment(row, &copy, cx));
            })
            .icon(IconName::Copy),
        );
        let name = attachment.name.clone();
        items.push(MenuItem::item("Copy file name", move |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(name.clone()))).icon(IconName::Copy));
        if message.from_me && caps.unsend {
            items.push(MenuItem::Separator);
            items.push(self.unsend_item(now_ms() - message.date));
        }
        open_menu(MenuRequest::at(position, items), window, cx);
    }

    fn open_who(&self, index: usize, position: Point<Pixels>, window: &mut Window, cx: &mut App) {
        let Some(group) = self.model.tapbacks.get(index) else { return };
        let mut items = vec![MenuItem::Header(group.glyph.clone().into())];
        items.extend(group.who.iter().map(|name| MenuItem::Note(name.clone().into())));
        open_menu(MenuRequest::at(position, items).min_width(px(160.)), window, cx);
    }

    /// `block` is the body's index among the message's blocks, for the order
    /// a selection reads across the thread.
    fn text_body(&self, body: &TextBody, block: usize, color: Hsla, palette: &Palette) -> AnyElement {
        let from_me = self.model.message.from_me;
        // Accent on a blue bubble would be the bubble itself, so mentions take white there.
        let accent = if from_me { palette.on_accent } else { palette.accent };
        // The lit range on my own bubble is the accent itself, so it takes a white wash there.
        let selection = if from_me { with_alpha(palette.on_accent, 0x5c) } else { with_alpha(palette.accent, 0x4d) };
        let date = self.model.message.date;
        let guid = self.model.message.guid.clone();
        let id = move |word: usize| ElementId::Name(format!("text-{guid}-{block}-{word}").into());
        match body {
            TextBody::Empty => div().into_any_element(),
            TextBody::Plain(text) => {
                let styled = crate::emoji_font::styled_text(text.clone(), Vec::new(), color);
                let selector = self.model.message.guid.clone();
                div().debug_selector(move || format!("bubble-text-{selector}")).child(selectable(id(0), text.clone(), styled, order(date, block, 0), selection)).into_any_element()
            }
            TextBody::Rich { text, spans, size } => {
                let styled = crate::emoji_font::styled_text(text.clone(), spans.iter().map(|(range, span)| (range.clone(), span.highlight(accent, color))).collect(), color);
                let selector = self.model.message.guid.clone();
                div().debug_selector(move || format!("bubble-text-{selector}")).when_some(*size, |el, (size, line)| el.text_size(size).line_height(line)).child(selectable(id(0), text.clone(), styled, order(date, block, 0), selection)).into_any_element()
            }
            TextBody::Chunks(lines) => {
                let mut word_index = 0usize;
                div()
                    .flex()
                    .flex_col()
                    .children(lines.iter().map(|line| {
                        div().flex().flex_row().flex_wrap().items_end().children(line.iter().map(|(word, span, size)| {
                            let range = 0..word.len();
                            let styled = crate::emoji_font::styled_text(word.clone(), vec![(range, span.highlight(accent, color))], color);
                            let word_order = order(date, block, word_index);
                            let word_id = id(word_index);
                            word_index += 1;
                            div()
                                .when_some(*size, |el, (size, line)| el.text_size(size).line_height(line))
                                .child(selectable(word_id, word.clone(), styled, word_order, selection))
                        }))
                    }))
                    .into_any_element()
            }
        }
    }

    fn render_blocks(&self, palette: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let message = self.model.message.clone();
        let from_me = message.from_me;
        let fill = self.fill(palette);
        let text_color = if from_me { palette.on_accent } else { palette.received_text };
        let show_tail = self.data.position.ends_run();
        let hovered_click = cx.listener(Self::on_bubble_click);
        let hovered_click = std::rc::Rc::new(hovered_click);
        let column = div().flex().flex_col().gap(spacing::X1).min_w(px(0.)).when(from_me, |el| el.items_end()).when(!from_me, |el| el.items_start());

        if let Some(emoji) = self.model.emoji.clone() {
            let click = hovered_click.clone();
            return column
                .id("emoji")
                .on_click(move |event, window, cx| click(event, window, cx))
                .on_mouse_down(MouseButton::Right, cx.listener(|row, event: &MouseDownEvent, window, cx| row.open_message_menu(event.position, window, cx)))
                .child(div().text_size(px(40.)).line_height(px(48.)).text_color(palette.text).when_some(font_emoji(), |el, font| el.font_family(font)).child(emoji))
                .into_any_element();
        }

        let last = self.model.blocks.len().saturating_sub(1);
        let radii = bubble_radius(from_me, self.data.position);
        let top_padding = if self.model.tapbacks.is_empty() { px(7.) } else { px(11.) };
        let children: Vec<AnyElement> = self
            .model
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| {
                let tail_color = |color: Hsla| (show_tail && index == last).then_some(TailFill::Color(color));
                match block {
                    Block::Photos(photos) => attachments::photo_grid(self, photos, tail_color(palette.received), palette, cx),
                    Block::Attachment(attachment) => {
                        let kind = kind_of(&message, attachment);
                        let body = match kind {
                            Kind::Sticker => attachments::sticker(attachment),
                            Kind::Audio => attachments::audio(self, index, attachment, tail_color(fill), fill, palette, cx),
                            Kind::Image => attachments::image(self, index, attachment, tail_color(palette.received), palette, cx),
                            Kind::Video => attachments::video(self, index, attachment, tail_color(palette.received), palette, cx),
                            Kind::File => attachments::file(self, index, attachment, fill, palette, cx),
                        };
                        let own = matches!(kind, Kind::Sticker | Kind::Audio | Kind::Image | Kind::Video);
                        let click = hovered_click.clone();
                        let guid = attachment.guid.clone();
                        let held = div()
                            .id(("held", index))
                            .max_w_full()
                            .on_click(move |event, window, cx| click(event, window, cx))
                            .on_mouse_down(MouseButton::Right, cx.listener(move |row, event: &MouseDownEvent, window, cx| row.open_attachment_menu(&guid, event.position, window, cx)))
                            .child(body);
                        tail_box(from_me, if own { None } else { tail_color(fill) }, palette, held).into_any_element()
                    }
                    Block::Link { href, host, matches } => {
                        let (card, color) =
                            if *matches { (attachments::link_preview(self, index, palette), palette.received) } else { (attachments::link_card(index, href, host, from_me, fill, palette), fill) };
                        tail_box(from_me, tail_color(color), palette, card).into_any_element()
                    }
                    Block::Preview => tail_box(from_me, tail_color(palette.received), palette, attachments::link_preview(self, index, palette)).into_any_element(),
                    Block::Text { subject, body } => {
                        let click = hovered_click.clone();
                        let bubble = div()
                            .id(("text", index))
                            .px(px(12.))
                            .pt(top_padding)
                            .pb(px(7.))
                            .rounded_tl(radii.top_left)
                            .rounded_tr(radii.top_right)
                            .rounded_bl(radii.bottom_left)
                            .rounded_br(radii.bottom_right)
                            .bg(fill)
                            .max_w_full()
                            .text_size(type_scale::BUBBLE.font_size)
                            .line_height(type_scale::BUBBLE.line_height)
                            .text_color(text_color)
                            .on_click(move |event, window, cx| click(event, window, cx))
                            .on_mouse_down(MouseButton::Right, cx.listener(|row, event: &MouseDownEvent, window, cx| row.open_message_menu(event.position, window, cx)))
                            .when_some(subject.clone(), |el, subject| el.child(div().font_weight(FontWeight::BOLD).child(subject)))
                            .child(self.text_body(body, index, text_color, palette));
                        tail_box(from_me, tail_color(fill), palette, bubble).into_any_element()
                    }
                }
            })
            .collect();
        column.children(children).into_any_element()
    }

    fn render_tapbacks(&mut self, window: &mut Window, palette: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.model.tapbacks.is_empty() {
            return None;
        }
        let from_me = self.model.message.from_me;
        let fresh = self.mounted_at.elapsed() > FRESH_AFTER;
        let mut animating = false;
        let pills: Vec<AnyElement> = self
            .model
            .tapbacks
            .iter()
            .enumerate()
            .map(|(index, group)| {
                let seen = *self.tapback_seen.entry(group.glyph.clone()).or_insert_with(|| fresh.then(Instant::now));
                let opacity = seen.and_then(|seen| eased_since(seen, DURATION_BASE, cx));
                animating |= opacity.is_some();
                let fg = if group.mine { palette.on_accent } else { palette.text };
                div()
                    .id(("tapback", index))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(2.))
                    .h(px(24.))
                    .px(px(if group.count > 1 { 7. } else { 5. }))
                    .rounded(px(12.))
                    .bg(if group.mine { palette.tapback_mine } else { palette.tapback })
                    .border_2()
                    .border_color(palette.canvas)
                    .when(index > 0, |el| el.ml(px(-6.)))
                    .opacity(opacity.unwrap_or(1.))
                    .on_mouse_down(MouseButton::Right, cx.listener(move |row, event: &MouseDownEvent, window, cx| row.open_who(index, event.position, window, cx)))
                    .child(div().text_size(px(12.)).line_height(px(16.)).text_color(palette.text).when_some(font_emoji(), |el, font| el.font_family(font)).child(group.glyph.clone()))
                    .when(group.count > 1, |el| {
                        el.child(div().text_size(type_scale::MICRO.font_size).line_height(type_scale::MICRO.line_height).font_weight(FontWeight::SEMIBOLD).text_color(fg).child(self.model.tapback_counts[index].clone()))
                    })
                    .into_any_element()
            })
            .collect();
        if animating {
            window.request_animation_frame();
        }
        let overlay = div().absolute().top(px(-TAPBACK_LIFT)).flex().items_center();
        let overlay = if from_me { overlay.right(px(-2.)).flex_row() } else { overlay.left(px(-2.)).flex_row_reverse() };
        Some(overlay.children(pills).into_any_element())
    }
}

impl Render for MessageRow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::trace::render("MessageRow");
        let palette = Theme::get(cx);
        let message = self.model.message.clone();
        let from_me = message.from_me;
        let position = self.data.position;
        let show_avatar = self.props.is_group && !from_me;
        let quoted = message.reply_to.is_some() && self.data.show_quote;
        let has_tapbacks = !self.model.tapbacks.is_empty();
        let failed = self.failed();

        let row_opacity = self.entered_at.and_then(|started| eased_since(started, DURATION_BASE, cx));
        if row_opacity.is_some() {
            window.request_animation_frame();
        } else {
            self.entered_at = None;
        }

        let time_label = self.model.time.clone();
        let time = |align_right: bool| {
            div()
                .w(px(TIME_COLUMN))
                .flex_shrink_0()
                .pb(px(5.))
                .whitespace_nowrap()
                .text_size(type_scale::MICRO.font_size)
                .line_height(type_scale::MICRO.line_height)
                .font_features(tabular())
                .text_color(palette.tertiary)
                .when(align_right, |el| el.text_right())
                .opacity(0.)
                .group_hover(ROW_GROUP, |style| style.opacity(1.))
                .child(time_label.clone())
        };

        let quote = self.props.quote.clone().filter(|_| quoted).map(|quote| {
            let thread = self.thread.clone();
            let target = quote.target.clone();
            div()
                .id("quote")
                .flex()
                .flex_row()
                .gap(spacing::X2)
                .mb(spacing::X1)
                .max_w_full()
                .cursor_pointer()
                .hover(|style| style.opacity(0.8))
                .on_click(move |_, _, cx| {
                    let _ = thread.update(cx, |thread, cx| thread.jump_to(&target, cx));
                })
                .child(div().w(px(2.)).rounded(px(1.)).flex_shrink_0().bg(if from_me { palette.imessage } else { palette.tertiary }))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_shrink(1.)
                        .min_w(px(0.))
                        .child(div().text_size(type_scale::MICRO.font_size).line_height(type_scale::MICRO.line_height).font_weight(FontWeight::SEMIBOLD).text_color(palette.secondary).child(quote.who))
                        .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).line_clamp(2).child(quote.body)),
                )
        });

        let blocks = self.render_blocks(&palette, cx);
        let tapbacks = self.render_tapbacks(window, &palette, cx);

        let translation = self.props.translation.clone().map(|translation| {
            let (text, color): (SharedString, Hsla) = match translation {
                Translation::Loading => ("Translating\u{2026}".into(), palette.secondary),
                Translation::Text(text) => (text, palette.secondary),
                Translation::Error(error) => (error, palette.danger),
            };
            div().pt(px(3.)).px(spacing::X1).max_w_full().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(color).child(text)
        });

        let replies = self.props.replies.clone().map(|label| {
            let thread = self.thread.clone();
            let guid = message.guid.clone();
            div()
                .id("replies")
                .pt(spacing::X1)
                .px(spacing::X1)
                .cursor_pointer()
                .hover(|style| style.opacity(0.8))
                .on_click(move |_, _, cx| {
                    let _ = thread.update(cx, |thread, cx| thread.open_reply_thread(&guid, cx));
                })
                .child(div().text_size(type_scale::MICRO.font_size).line_height(type_scale::MICRO.line_height).font_weight(FontWeight::SEMIBOLD).text_color(palette.accent).child(label))
        });

        let receipt = self.data.receipt.clone();
        let notify = self.data.notify && self.capabilities().focus_status;
        let footer = (message.date_edited.is_some() || self.model.effect.is_some() || receipt.is_some()).then(|| {
            let micro = |text: SharedString, color: Hsla| div().text_size(type_scale::MICRO.font_size).line_height(type_scale::MICRO.line_height).text_color(color).child(text);
            let store = self.store.clone();
            let (primary, guid) = (self.props.primary.clone(), message.guid.clone());
            let retry = (store.clone(), primary.clone(), guid.clone());
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap(spacing::X1)
                .pt(px(3.))
                .px(spacing::X1)
                .when(from_me, |el| el.justify_end())
                .when(message.date_edited.is_some(), |el| el.child(micro("Edited".into(), palette.tertiary)))
                .when_some(self.model.effect.clone(), |el, effect| el.child(micro(effect, palette.tertiary)))
                .when(failed, |el| el.child(Icon::new(IconName::Alert).size(px(12.)).color(palette.danger)))
                .when_some(receipt, |el, receipt| {
                    el.child(micro(receipt.text.into(), if receipt.failed { palette.danger } else { palette.secondary }).font_weight(FontWeight::SEMIBOLD).font_features(tabular()))
                })
                .when(failed, |el| {
                    el.child(
                        div()
                            .id("retry")
                            .cursor_pointer()
                            .hover(|style| style.opacity(0.8))
                            .on_click(move |_, _, _| retry.0.retry(&retry.1, &retry.2))
                            .child(micro("Try again".into(), palette.accent).font_weight(FontWeight::SEMIBOLD)),
                    )
                })
                .when(notify, |el| {
                    el.child(
                        div()
                            .id("notify")
                            .cursor_pointer()
                            .hover(|style| style.opacity(0.8))
                            .on_click(move |_, _, _| {
                                let (store2, primary, guid) = (store.clone(), primary.clone(), guid.clone());
                                store.spawn(async move { store2.notify_silenced(&primary, &guid).await });
                            })
                            .child(micro("Notify Anyway".into(), palette.accent).font_weight(FontWeight::SEMIBOLD)),
                    )
                })
        });

        let align = |el: Div| if from_me { el.items_end() } else { el.items_start() };
        let content = align(div().flex().flex_col().max_w(relative(0.62)).min_w(px(0.))).child(
            align(div().flex().flex_col().max_w(BUBBLE_MAX_WIDTH).min_w(px(0.)))
                .children(quote)
                .child(align(div().relative().max_w_full().flex().flex_col().when(has_tapbacks && !quoted, |el| el.mt(px(TAPBACK_LIFT)))).child(blocks).children(tapbacks))
                .children(translation)
                .children(replies)
                .children(footer),
        );

        let sender = (self.data.show_sender).then(|| self.model.sender_name.clone()).flatten().map(|name| {
            let inset = if show_avatar { AVATAR_COLUMN + 8. } else { 0. } + 12.;
            div()
                .pl(px(inset))
                .pb(px(3.))
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(type_scale::MICRO.font_size)
                .line_height(type_scale::MICRO.line_height)
                .text_color(palette.secondary)
                .child(name)
        });

        let avatar = show_avatar.then(|| {
            div()
                .w(px(AVATAR_COLUMN))
                .flex_shrink_0()
                .flex()
                .items_end()
                .when(position.ends_run(), |el| el.child(crate::primitives::avatar(message.sender.as_ref(), None, px(AVATAR_COLUMN), cx)))
        });

        let layout = self.layout.clone();
        let measure = canvas(
            move |bounds, window, _| {
                if let Some(given) = layout.given.get() {
                    if (given - bounds.size.height).abs() > px(0.5) {
                        // The content outgrew the cached box: repaint with the new height.
                        layout.stale.set(true);
                        window.request_animation_frame();
                    }
                }
                layout.measured.set(Some(bounds.size));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();

        div()
            .id("message-row")
            .group(ROW_GROUP)
            .relative()
            .child(measure)
            .flex()
            .flex_col()
            .w_full()
            .px(THREAD_INSET)
            .pt(px(if position.starts_run() { GAP_BETWEEN_RUNS } else { GAP_IN_RUN }))
            .rounded(radius::ROW)
            .when(self.props.highlighted, |el| el.bg(palette.selected_soft))
            .when_some(row_opacity, |el, opacity| el.opacity(opacity))
            .children(sender)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(spacing::X2)
                    .w_full()
                    .when(from_me, |el| el.justify_end())
                    .children(avatar)
                    .when(from_me, |el| el.child(time(true)))
                    .child(content)
                    .when(!from_me, |el| el.child(time(false))),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // gpui_kit's glob exports its own `test` attribute, which would shadow the std one.
    use std::prelude::v1::test;
    use messages_core::UrlPreview;

    fn attachment(guid: &str, mime: &str) -> Attachment {
        Attachment {
            guid: guid.into(),
            name: format!("{guid}.bin"),
            mime: mime.into(),
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

    fn kinds(blocks: &[Block]) -> Vec<&'static str> {
        blocks
            .iter()
            .map(|block| match block {
                Block::Photos(_) => "photos",
                Block::Attachment(_) => "attachment",
                Block::Link { .. } => "link",
                Block::Text { .. } => "text",
                Block::Preview => "preview",
            })
            .collect()
    }

    #[test]
    fn links_split_out_of_text_and_the_matching_one_carries_the_preview() {
        let message = Message {
            text: "look at https://example.com/a/ and tell me".into(),
            url_preview: Some(UrlPreview { url: "http://EXAMPLE.com/a".into(), ..UrlPreview::default() }),
            ..Message::default()
        };
        let blocks = blocks_of(&message);
        assert_eq!(kinds(&blocks), vec!["text", "link", "text"]);
        assert!(matches!(&blocks[1], Block::Link { matches: true, host, .. } if host.as_ref() == "example.com"));
    }

    #[test]
    fn an_unmatched_preview_goes_last() {
        let message = Message { text: "hi".into(), url_preview: Some(UrlPreview { url: "https://other.org".into(), ..UrlPreview::default() }), ..Message::default() };
        assert_eq!(kinds(&blocks_of(&message)), vec!["text", "preview"]);
    }

    #[test]
    fn two_photos_share_a_grid_ahead_of_other_attachments() {
        let message = Message { attachments: vec![attachment("a", "image/jpeg"), attachment("f", "application/pdf"), attachment("b", "image/png")], ..Message::default() };
        let blocks = blocks_of(&message);
        assert_eq!(kinds(&blocks), vec!["photos", "attachment"]);
        assert!(matches!(&blocks[0], Block::Photos(photos) if photos.len() == 2));
    }

    #[test]
    fn emoji_only_messages_have_no_blocks_and_a_subject_heads_the_first_text() {
        let emoji = Message { text: "\u{1F389}\u{1F389}".into(), ..Message::default() };
        assert!(emoji_only(&emoji));
        assert!(blocks_of(&emoji).is_empty());
        let subject = Message { subject: Some("Re: plans".into()), text: "sure".into(), ..Message::default() };
        assert!(matches!(&blocks_of(&subject)[0], Block::Text { subject: Some(s), .. } if s.as_ref() == "Re: plans"));
        let only_subject = Message { subject: Some("Hello".into()), ..Message::default() };
        assert!(matches!(&blocks_of(&only_subject)[0], Block::Text { body: TextBody::Empty, .. }));
    }

    #[test]
    fn rich_parts_keep_spans_and_sizes() {
        let runs = vec![RichRun { text: "bold".into(), bold: true, ..RichRun::default() }, RichRun { text: " plain".into(), ..RichRun::default() }];
        match rich_body(&runs) {
            TextBody::Rich { text, spans, size } => {
                assert_eq!(text.as_ref(), "bold plain");
                assert_eq!(spans.len(), 1);
                assert_eq!(spans[0].0, 0..4);
                assert_eq!(size, None);
            }
            other => panic!("expected rich text, got {other:?}"),
        }
        let mixed = vec![RichRun { text: "shipped".into(), effect: Some(TextEffect::Big), ..RichRun::default() }, RichRun { text: " now".into(), ..RichRun::default() }];
        assert!(matches!(rich_body(&mixed), TextBody::Chunks(_)));
        let all_big = vec![RichRun { text: "shipped".into(), effect: Some(TextEffect::Big), ..RichRun::default() }];
        assert!(matches!(rich_body(&all_big), TextBody::Rich { size: Some(_), .. }));
    }

    #[test]
    fn words_keep_their_trailing_space() {
        assert_eq!(words("one two  three"), vec!["one ", "two  ", "three"]);
        assert_eq!(words("  lead"), vec!["  ", "lead"]);
    }
}
