//! Fixtures and a transport that answers from them, for `MESSAGES_DEMO=1`
//! and the UI tests. Sends get a canned reply.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use crate::model::{
    Attachment, Chat, Contact, FocusStatus, GroupEvent, Handle, Message, MessagePart, Millis, Reaction, RichRun, ScheduledMessage, ServerInfo, Service, Tapback,
    TapbackKind, TextEffect, UrlPreview,
};
use crate::open::{TextSegment, split_links};
use crate::store::{mime_for_path, now_ms};
use crate::transport::*;

const MIN: Millis = 60_000;
const HOUR: Millis = 60 * MIN;
const DAY: Millis = 24 * HOUR;

// The audio pill needs a real file to hand to the system player. macOS ships
// one; on Linux the demo has no clip and the pill just shows its duration.
const SYSTEM_SOUND: &str = "/System/Library/Sounds/Submarine.aiff";

fn svg_url(svg: &str) -> String {
    format!("data:image/svg+xml;base64,{}", base64::engine::general_purpose::STANDARD.encode(svg))
}

fn avatar(hue_a: u32, hue_b: u32, initials: &str) -> String {
    svg_url(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240" viewBox="0 0 240 240"><defs><linearGradient id="a" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="hsl({hue_a} 70% 55%)"/><stop offset="1" stop-color="hsl({hue_b} 55% 30%)"/></linearGradient></defs><rect width="240" height="240" rx="120" fill="url(#a)"/><text x="120" y="146" font-family="sans-serif" font-size="88" font-weight="600" text-anchor="middle" fill="white" opacity="0.92">{initials}</text></svg>"#
    ))
}

fn photo(hue_a: u32, hue_b: u32, label: &str) -> String {
    svg_url(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="800" viewBox="0 0 1200 800"><defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="hsl({hue_a} 70% 55%)"/><stop offset="1" stop-color="hsl({hue_b} 60% 25%)"/></linearGradient></defs><rect width="1200" height="800" fill="url(#g)"/><circle cx="880" cy="250" r="110" fill="hsl(45 95% 80%)" opacity="0.9"/><path d="M0 620 L260 470 L420 560 L640 400 L860 540 L1200 430 L1200 800 L0 800Z" fill="hsl({hue_b} 50% 16%)"/><text x="40" y="760" font-family="sans-serif" font-size="36" fill="white" opacity="0.6">{label}</text></svg>"#
    ))
}

fn sticker_face() -> String {
    svg_url(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="200" viewBox="0 0 240 200"><rect x="8" y="8" width="224" height="184" rx="40" fill="hsl(280 68% 58%)"/><circle cx="88" cy="82" r="13" fill="white"/><circle cx="152" cy="82" r="13" fill="white"/><path d="M72 120 Q120 166 168 120" stroke="white" stroke-width="13" fill="none" stroke-linecap="round"/></svg>"#,
    )
}

fn photo_src(guid: &str) -> Option<String> {
    Some(match guid {
        "demo-att-1" => photo(200, 260, "Point Reyes"),
        "demo-att-2" => photo(20, 340, "Golden hour"),
        "demo-att-3" => photo(140, 200, "Trailhead"),
        "demo-att-4" => photo(28, 300, "Roof terrace"),
        "demo-att-5" => svg_url(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="600" height="1300" viewBox="0 0 600 1300"><rect width="600" height="1300" fill="#2f5d8a"/><rect x="40" y="40" width="520" height="1220" rx="40" fill="#123"/><text x="60" y="140" font-family="sans-serif" font-size="48" fill="white">Tall screenshot</text><text x="60" y="1220" font-family="sans-serif" font-size="40" fill="#9cf">bottom edge</text></svg>"##,
        ),
        "demo-att-9" | "demo-att-10" | "demo-att-11" => svg_url(&format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="1170" height="2532" viewBox="0 0 1170 2532"><rect width="1170" height="2532" fill="#101418"/><rect x="60" y="180" width="1050" height="160" rx="40" fill="#1f2a33"/><rect x="60" y="2280" width="1050" height="140" rx="70" fill="#1f2a33"/><text x="90" y="1300" font-family="sans-serif" font-size="90" fill="#9cc">{guid}</text></svg>"##
        )),
        "demo-att-6" => photo(190, 240, "Rooftops"),
        "demo-att-7" => photo(330, 20, "Plaza Mayor"),
        "demo-att-8" => photo(80, 140, "Tapas"),
        "demo-sticker-1" => sticker_face(),
        _ => return None,
    })
}

struct People {
    alex: Handle,
    priya: Handle,
    jordan: Handle,
    mom: Handle,
    dad: Handle,
    sam: Handle,
    nadia: Handle,
    ben: Handle,
    chloe: Handle,
    unknown: Handle,
}

fn handle(address: &str, service: Service, name: Option<&str>, avatar: Option<String>) -> Handle {
    Handle { address: address.into(), service, name: name.map(Into::into), avatar }
}

/// Alex shares a location, so the details panel has a map to show.
pub fn friends() -> Vec<crate::findmy::FriendLocation> {
    vec![crate::findmy::FriendLocation {
        id: "demo-friend-alex".into(),
        name: Some("Alex Rivera".into()),
        addresses: vec!["+14155550134".into()],
        latitude: 37.7955,
        longitude: -122.3937,
        accuracy: Some(35.),
        timestamp: now_ms() - 4 * 60_000,
        label: Some("Ferry Building, San Francisco".into()),
        is_sharing: true,
    }]
}

fn people() -> People {
    People {
        alex: handle("+14155550134", Service::IMessage, Some("Alex Rivera"), Some(avatar(200, 260, "AR"))),
        priya: handle("priya.natarajan@icloud.com", Service::IMessage, Some("Priya Natarajan"), Some(avatar(320, 20, "PN"))),
        jordan: handle("+14155550188", Service::Sms, Some("Jordan Lee"), None),
        mom: handle("+14155550101", Service::IMessage, Some("Mom"), None),
        dad: handle("+14155550102", Service::IMessage, Some("Dad"), None),
        sam: handle("+14155550103", Service::IMessage, Some("Sam"), None),
        nadia: handle("+34612345678", Service::IMessage, Some("Nadia Haddad"), None),
        ben: handle("+14155550170", Service::IMessage, Some("Ben Okafor"), None),
        chloe: handle("chloe@martin.design", Service::IMessage, Some("Chloe Martin"), None),
        unknown: handle("+14155550199", Service::IMessage, None, None),
    }
}

impl People {
    fn all(&self) -> [&Handle; 10] {
        [&self.alex, &self.priya, &self.jordan, &self.mom, &self.dad, &self.sam, &self.nadia, &self.ben, &self.chloe, &self.unknown]
    }
}

fn attachment(guid: &str, name: &str, width: u32, height: u32) -> Attachment {
    Attachment {
        guid: guid.into(),
        name: name.into(),
        mime: "image/svg+xml".into(),
        bytes: 812_344,
        width: Some(width),
        height: Some(height),
        measured: false,
        is_sticker: false,
        local_path: photo_src(guid).map(PathBuf::from),
        hidden: false,
        duration_ms: None,
    }
}

/// A 12-frame looping GIF written once into the cache, so the demo shows an
/// animated attachment without shipping a binary fixture.
fn wave_gif() -> Option<PathBuf> {
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame, Rgba, RgbaImage};

    let path = crate::config::cache_dir().join("demo").join("wave.gif");
    if path.exists() {
        return Some(path);
    }
    std::fs::create_dir_all(path.parent()?).ok()?;
    let tmp = path.with_extension("gif.tmp");
    let file = std::fs::File::create(&tmp).ok()?;
    let mut encoder = GifEncoder::new(file);
    encoder.set_repeat(Repeat::Infinite).ok()?;
    let (size, frames) = (120u32, 12u32);
    for index in 0..frames {
        let angle = index as f32 / frames as f32 * std::f32::consts::TAU;
        let (cx, cy) = (60.0 + 34.0 * angle.cos(), 60.0 + 34.0 * angle.sin());
        let image = RgbaImage::from_fn(size, size, |x, y| {
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            if dx * dx + dy * dy <= 18.0 * 18.0 { Rgba([255, 204, 0, 255]) } else { Rgba([40, 44, 52, 255]) }
        });
        encoder.encode_frame(Frame::from_parts(image, 0, 0, Delay::from_numer_denom_ms(100, 1))).ok()?;
    }
    drop(encoder);
    std::fs::rename(&tmp, &path).ok()?;
    Some(path)
}

fn wave(guid: &str) -> Attachment {
    Attachment {
        guid: guid.into(),
        name: "wave.gif".into(),
        mime: "image/gif".into(),
        bytes: 9_600,
        width: Some(120),
        height: Some(120),
        measured: false,
        is_sticker: false,
        local_path: wave_gif(),
        hidden: false,
        duration_ms: None,
    }
}

fn sticker(guid: &str) -> Attachment {
    Attachment { name: "sticker.png".into(), bytes: 24_110, width: Some(240), height: Some(200), is_sticker: true, ..attachment(guid, "", 0, 0) }
}

fn voice_note(guid: &str, seconds: f64) -> Attachment {
    Attachment {
        guid: guid.into(),
        name: "Audio Message.caf".into(),
        mime: "audio/x-caf".into(),
        bytes: 61_820,
        width: None,
        height: None,
        measured: false,
        is_sticker: false,
        local_path: Path::new(SYSTEM_SOUND).exists().then(|| PathBuf::from(SYSTEM_SOUND)),
        hidden: false,
        duration_ms: Some(seconds * 1000.0),
    }
}

fn run(text: &str) -> RichRun {
    RichRun { text: text.into(), ..RichRun::default() }
}

/// One fixture row: `ago` before now, from `from` (None is me), plus whatever else it carries.
struct Row {
    ago: Millis,
    from: Option<Handle>,
    message: Message,
}

fn row(text: &str, ago: Millis, from: Option<&Handle>) -> Row {
    Row { ago, from: from.cloned(), message: Message { text: text.into(), ..Message::default() } }
}

impl Row {
    fn with(mut self, edit: impl FnOnce(&mut Message)) -> Self {
        edit(&mut self.message);
        self
    }
}

struct Seed {
    guid: &'static str,
    identifier: &'static str,
    service: Service,
    is_group: bool,
    display_name: Option<&'static str>,
    icon: Option<String>,
    participants: Vec<Handle>,
    rows: Vec<Row>,
}

fn tapback(guid: &str, kind: TapbackKind, sender: Option<&Handle>) -> Tapback {
    Tapback { guid: guid.into(), kind, emoji: None, from_me: sender.is_none(), sender: sender.cloned() }
}

fn seeds(p: &People, now: Millis) -> Vec<Seed> {
    let alex = Some(&p.alex);
    let priya = Some(&p.priya);
    let ben = Some(&p.ben);
    let chloe = Some(&p.chloe);
    let nadia = Some(&p.nadia);
    let sam = Some(&p.sam);
    let mom = Some(&p.mom);
    vec![
        Seed {
            guid: "iMessage;-;+14155550134",
            identifier: "+14155550134",
            service: Service::IMessage,
            is_group: false,
            display_name: None,
            icon: None,
            participants: vec![p.alex.clone()],
            rows: vec![
                row("did you end up trying the vulkan build on the thinkpad?", 3 * HOUR + 22 * MIN, alex),
                row("Yeah, it boots. Scrolling a 10k row list at 120 fps on integrated graphics.", 3 * HOUR + 20 * MIN, None),
                row("ok that is genuinely wild", 3 * HOUR + 19 * MIN, alex),
                row("Tapbacks work too, react to this one", 3 * HOUR + 18 * MIN, None).with(|m| m.tapbacks = vec![tapback("demo-tb-1", TapbackKind::Laugh, alex)]),
                row("coffee at 4? the place on valencia", 41 * MIN, alex),
                row("Works for me. Grabbing the laptop.", 39 * MIN, None).with(|m| {
                    m.date_delivered = Some(now - 39 * MIN);
                    m.date_read = Some(now - 38 * MIN);
                }),
                row("", 30 * MIN, None).with(|m| m.attachments = vec![attachment("demo-att-11", "IMG_2203.PNG", 1170, 2532)]),
                row("it says applied and a new chat still says 2 days", 29 * MIN, None),
                row("", 28 * MIN, None).with(|m| {
                    m.attachments = vec![attachment("demo-att-9", "IMG_2201.PNG", 1170, 2532), attachment("demo-att-10", "IMG_2202.PNG", 1170, 2532)]
                }),
                row("the tall screenshots must not run under these bubbles", 27 * MIN, None),
                row("the buttons are wrong and the layout is broken", 26 * MIN, None),
                row("bring the charger this time 🔌", 12 * MIN, alex).with(|m| m.reply_to = Some("demo-msg-0006".into())),
                row("", 11 * MIN, alex).with(|m| m.attachments = vec![wave("demo-gif-1")]),
            ],
        },
        Seed {
            guid: "iMessage;+;chat240119384759",
            identifier: "chat240119384759",
            service: Service::IMessage,
            is_group: true,
            display_name: Some("Family"),
            icon: Some(avatar(20, 340, "F")),
            participants: vec![p.mom.clone(), p.dad.clone(), p.sam.clone()],
            rows: vec![
                row("", 2 * DAY + 5 * HOUR, mom).with(|m| m.group_event = Some(GroupEvent::Rename { title: "Family".into() })),
                row("Sunday lunch is at ours, 1pm. Bring the good bread.", 2 * DAY + 4 * HOUR, mom),
                row("On it 🥖", 2 * DAY + 3 * HOUR + 50 * MIN, None)
                    .with(|m| m.tapbacks = vec![tapback("demo-tb-2", TapbackKind::Love, mom), tapback("demo-tb-3", TapbackKind::Like, Some(&p.dad))]),
                row("Look at this from the hike", DAY + 2 * HOUR, sam).with(|m| m.attachments = vec![attachment("demo-att-1", "IMG_4021.HEIC", 1200, 800)]),
                row("Gorgeous!! Which trail?", DAY + HOUR + 55 * MIN, mom),
                row("Tomales Point, we saw elk", DAY + HOUR + 40 * MIN, sam),
                row("Can someone grab dad from the airport friday", 5 * HOUR, sam),
                row("I can. Flight number?", 4 * HOUR + 58 * MIN, None).with(|m| m.date_delivered = Some(now - 4 * HOUR + 57 * MIN)),
            ],
        },
        Seed {
            guid: "iMessage;-;priya.natarajan@icloud.com",
            identifier: "priya.natarajan@icloud.com",
            service: Service::IMessage,
            is_group: false,
            display_name: None,
            icon: None,
            participants: vec![p.priya.clone()],
            rows: vec![
                row("Sent the deck over, let me know what you think about slide 9", DAY + 6 * HOUR, priya),
                row("Slide 9 is the strongest one. Lead with it.", DAY + 5 * HOUR + 30 * MIN, None).with(|m| {
                    m.date_delivered = Some(now - DAY);
                    m.date_read = Some(now - DAY);
                }),
                row("Reordered. Also fixed the typo you did not mention 😅", 20 * HOUR, priya).with(|m| m.date_edited = Some(now - 19 * HOUR)),
                row("", 19 * HOUR, priya).with(|m| m.attachments = vec![sticker("demo-sticker-1")]),
            ],
        },
        Seed {
            guid: "SMS;-;+14155550188",
            identifier: "+14155550188",
            service: Service::Sms,
            is_group: false,
            display_name: None,
            icon: None,
            participants: vec![p.jordan.clone()],
            rows: vec![
                row("Hey its Jordan from the climbing gym, still up for thursday?", 2 * DAY + 3 * HOUR, Some(&p.jordan)),
                row("Yes! 7pm at Dogpatch?", 2 * DAY + 2 * HOUR + 45 * MIN, None).with(|m| m.date_delivered = Some(now - 2 * DAY)),
                row("Perfect see you there", 2 * DAY + 2 * HOUR + 30 * MIN, Some(&p.jordan)),
            ],
        },
        Seed {
            guid: "iMessage;+;chat881204957120",
            identifier: "chat881204957120",
            service: Service::IMessage,
            is_group: true,
            display_name: Some("Design crit"),
            icon: None,
            participants: vec![p.chloe.clone(), p.ben.clone(), p.nadia.clone()],
            rows: vec![
                row("New composer states, tear it apart", 6 * DAY, chloe).with(|m| m.attachments = vec![attachment("demo-att-2", "composer-states.png", 1200, 800)]),
                row("The send button reads as disabled even when it is not. Try the filled circle.", 6 * DAY - 20 * MIN, ben),
                row("Agree with Ben. Also the placeholder contrast is under 3:1.", 6 * DAY - 15 * MIN, None),
                row("fixed both, thanks 🙏", 5 * DAY, chloe).with(|m| m.tapbacks = vec![tapback("demo-tb-4", TapbackKind::Like, None)]),
                row("The spacing is close but the last frame is off by a hair, and the old radius is gone", 3 * HOUR, ben).with(|m| {
                    m.parts = Some(vec![MessagePart::Text {
                        runs: vec![
                            run("The spacing is "),
                            RichRun { bold: true, ..run("close") },
                            run(" but the last frame is "),
                            RichRun { italic: true, ..run("off by a hair") },
                            run(", and the "),
                            RichRun { strike: true, ..run("old radius") },
                            run(" is "),
                            RichRun { underline: true, ..run("gone") },
                        ],
                    }])
                }),
                row("You should look at the fold, it drifts two pixels there", 2 * HOUR + 50 * MIN, chloe).with(|m| {
                    m.parts = Some(vec![MessagePart::Text {
                        runs: vec![RichRun { mention: Some("you@icloud.com".into()), ..run("You") }, run(" should look at the fold, it drifts two pixels there")],
                    }])
                }),
                row("shipped 🎉", 2 * HOUR + 40 * MIN, None).with(|m| {
                    m.parts = Some(vec![MessagePart::Text { runs: vec![RichRun { effect: Some(TextEffect::Big), ..run("shipped") }, run(" 🎉")] }]);
                    m.date_delivered = Some(now - 2 * HOUR - 39 * MIN);
                }),
            ],
        },
        Seed {
            guid: "iMessage;-;+14155550199",
            identifier: "+14155550199",
            service: Service::IMessage,
            is_group: false,
            display_name: None,
            icon: None,
            participants: vec![p.unknown.clone()],
            rows: vec![row("Hi, this is Morgan from the bike shop. Your wheel is ready for pickup.", 26 * MIN, Some(&p.unknown))],
        },
        Seed {
            guid: "iMessage;-;+34612345678",
            identifier: "+34612345678",
            service: Service::IMessage,
            is_group: false,
            display_name: None,
            icon: None,
            participants: vec![p.nadia.clone()],
            rows: vec![
                row("Landed in Madrid. It is 38 degrees.", 3 * DAY + 4 * HOUR, nadia).with(|m| m.attachments = vec![attachment("demo-att-3", "IMG_0911.HEIC", 1200, 800)]),
                // Like the live server: no dimensions, and the file arrives after the row is on screen.
                row("", 3 * DAY + 3 * HOUR + 40 * MIN, None).with(|m| {
                    m.attachments = vec![Attachment {
                        mime: "image/png".into(),
                        bytes: 1_204_000,
                        width: Some(0),
                        height: Some(0),
                        local_path: None,
                        ..attachment("demo-att-5", "IMG_0912.PNG", 0, 0)
                    }]
                }),
                row("That is the booking, keep it somewhere", 3 * DAY + 3 * HOUR + 39 * MIN, None),
                row("Drink water. Send tapas.", 3 * DAY + 3 * HOUR, None).with(|m| {
                    m.date_delivered = Some(now - 3 * DAY);
                    m.date_read = Some(now - 3 * DAY);
                }),
                row("The roof terrace at sunset\nAnd the same spot at 7am, nobody about", 2 * DAY + 6 * HOUR, nadia).with(|m| {
                    m.attachments = vec![attachment("demo-att-4", "IMG_1042.HEIC", 1200, 800)];
                    m.parts = Some(vec![
                        MessagePart::Text { runs: vec![run("The roof terrace at sunset")] },
                        MessagePart::Attachment { guid: "demo-att-4".into() },
                        MessagePart::Text { runs: vec![run("And the same spot at 7am, nobody about")] },
                    ]);
                }),
                row("Three more from the roof", 2 * DAY + 5 * HOUR + 30 * MIN, nadia).with(|m| {
                    m.attachments = vec![
                        attachment("demo-att-6", "IMG_1043.HEIC", 1200, 800),
                        attachment("demo-att-7", "IMG_1044.HEIC", 1200, 800),
                        attachment("demo-att-8", "IMG_1045.HEIC", 1200, 800),
                    ]
                }),
                row("", 2 * DAY + 5 * HOUR, nadia).with(|m| {
                    m.is_audio = true;
                    m.attachments = vec![voice_note("demo-audio-1", 12.0)];
                }),
            ],
        },
        Seed {
            guid: "iMessage;-;+14155550170",
            identifier: "+14155550170",
            service: Service::IMessage,
            is_group: false,
            display_name: None,
            icon: None,
            participants: vec![p.ben.clone()],
            rows: vec![
                row("PR is up, no rush", 4 * DAY + 2 * HOUR, ben),
                row("Reviewing tonight", 4 * DAY + HOUR, None).with(|m| {
                    m.date_delivered = Some(now - 4 * DAY);
                    m.delivered_quietly = Some(true);
                }),
                row("https://docs.bluebubbles.app/private-api/installation", 3 * DAY + 8 * HOUR, ben).with(|m| {
                    m.balloon_bundle_id = Some("com.apple.messages.URLBalloonProvider".into());
                    m.url_preview = Some(UrlPreview {
                        url: "https://docs.bluebubbles.app/private-api/installation".into(),
                        title: Some("Private API Installation, and why it needs SIP disabled".into()),
                        site_name: Some("BlueBubbles Docs".into()),
                        ..UrlPreview::default()
                    });
                }),
            ],
        },
        Seed {
            guid: "iMessage;-;chloe@martin.design",
            identifier: "chloe@martin.design",
            service: Service::IMessage,
            is_group: false,
            display_name: None,
            icon: None,
            participants: vec![p.chloe.clone()],
            rows: vec![row("Do you still have that Inter alternative you mentioned? The one with the tabular figures", 9 * DAY, chloe)],
        },
    ]
}

fn replies(chat_guid: &str) -> &'static [&'static str] {
    match chat_guid {
        "iMessage;-;+14155550134" => &["ha, deal", "see you at 4 then", "the wifi there is terrible btw, tether"],
        "iMessage;+;chat240119384759" => &["UA 1287, lands 6:40", "thank you!!", "I will text when I land"],
        "iMessage;-;priya.natarajan@icloud.com" => &["perfect, shipping it", "you are the best"],
        "SMS;-;+14155550188" => &["👍", "Sounds good"],
        _ => &[],
    }
}

fn matches_attachment_filter(attachment: &Attachment, filter: AttachmentFilter) -> bool {
    let image = attachment.mime.starts_with("image/");
    let video = attachment.mime.starts_with("video/");
    match filter {
        AttachmentFilter::Image => image,
        AttachmentFilter::Video => video,
        AttachmentFilter::File => !image && !video,
    }
}

#[derive(Default)]
struct DemoState {
    listeners: Vec<mpsc::UnboundedSender<TransportEvent>>,
    chats: HashMap<String, Chat>,
    messages: HashMap<String, Vec<Message>>,
    timers: Vec<AbortHandle>,
    reply_index: HashMap<String, usize>,
    scheduled: HashMap<String, ScheduledMessage>,
    scheduled_timers: HashMap<String, AbortHandle>,
    scheduled_seq: u32,
    seq: u32,
}

struct DemoInner {
    people: People,
    state: Mutex<DemoState>,
}

pub struct DemoTransport {
    inner: Arc<DemoInner>,
}

impl Default for DemoTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl DemoTransport {
    pub fn new() -> Self {
        let people = people();
        let now = now_ms();
        let mut state = DemoState::default();
        // Fixtures reference guids by number (reply_to), so every instance numbers from one.
        for seed in seeds(&people, now) {
            let list: Vec<Message> = seed
                .rows
                .into_iter()
                .map(|row| {
                    state.seq += 1;
                    Message {
                        guid: format!("demo-msg-{:04}", state.seq),
                        chat_guid: seed.guid.into(),
                        from_me: row.from.is_none(),
                        sender: row.from,
                        date: now - row.ago,
                        service: seed.service,
                        ..row.message
                    }
                })
                .collect();
            let last = list.last().cloned();
            let unread = last.as_ref().is_some_and(|last| !last.from_me && last.date_read.is_none() && seed.guid.ends_with("0199"));
            state.chats.insert(
                seed.guid.into(),
                Chat {
                    guid: seed.guid.into(),
                    identifier: seed.identifier.into(),
                    service: seed.service,
                    is_group: seed.is_group,
                    display_name: seed.display_name.map(Into::into),
                    icon: seed.icon,
                    participants: seed.participants,
                    unread,
                    last_activity: last.as_ref().map_or(0, |last| last.date),
                    last_message: last,
                    ..Chat::default()
                },
            );
            state.messages.insert(seed.guid.into(), list);
        }
        Self { inner: Arc::new(DemoInner { people, state: Mutex::new(state) }) }
    }
}

impl DemoInner {
    fn emit(&self, event: TransportEvent) {
        self.state.lock().listeners.retain(|tx| tx.send(event.clone()).is_ok());
    }

    fn next_guid(&self) -> String {
        let mut state = self.state.lock();
        state.seq += 1;
        format!("demo-msg-{:04}", state.seq)
    }

    fn later(self: &Arc<Self>, ms: u64, run: impl FnOnce(&Arc<DemoInner>) + Send + 'static) {
        let inner = self.clone();
        let task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            run(&inner);
        });
        self.state.lock().timers.push(task.abort_handle());
    }

    fn chat(&self, chat_guid: &str) -> TransportResult<Chat> {
        self.state.lock().chats.get(chat_guid).cloned().ok_or_else(|| TransportError::Network(format!("No chat {chat_guid}")))
    }

    fn push(&self, message: Message, update_chat: bool) {
        {
            let mut state = self.state.lock();
            let list = state.messages.entry(message.chat_guid.clone()).or_default();
            match list.iter().position(|item| item.guid == message.guid) {
                Some(index) => list[index] = message.clone(),
                None => list.push(message.clone()),
            }
            if update_chat && message.reaction.is_none() {
                if let Some(chat) = state.chats.get_mut(&message.chat_guid) {
                    chat.last_activity = chat.last_activity.max(message.date);
                    chat.last_message = Some(message.clone());
                }
            }
        }
        self.emit(TransportEvent::Message(message));
    }

    fn schedule_reply(self: &Arc<Self>, chat_guid: &str) {
        let pool = replies(chat_guid);
        let Ok(chat) = self.chat(chat_guid) else { return };
        if pool.is_empty() {
            return;
        }
        let index = {
            let mut state = self.state.lock();
            let entry = state.reply_index.entry(chat_guid.into()).or_insert(0);
            *entry += 1;
            *entry - 1
        };
        let text = pool[index % pool.len()];
        let from = chat.participants.first().cloned();
        let guid = chat_guid.to_owned();
        self.later(1400, move |inner| inner.emit(TransportEvent::Typing { chat_guid: guid, typing: true }));
        let guid = chat_guid.to_owned();
        self.later(3600, move |inner| {
            inner.emit(TransportEvent::Typing { chat_guid: guid.clone(), typing: false });
            let message = Message {
                guid: inner.next_guid(),
                chat_guid: guid,
                text: text.into(),
                sender: from,
                date: now_ms(),
                service: chat.service,
                ..Message::default()
            };
            inner.push(message, true);
        });
    }

    fn deliver(self: &Arc<Self>, message: &Message) {
        let delivered = message.clone();
        self.later(500, move |inner| inner.push(Message { date_delivered: Some(now_ms()), ..delivered }, false));
        if message.chat_guid.ends_with("0134") {
            let read = message.clone();
            self.later(2200, move |inner| {
                let now = now_ms();
                inner.push(Message { date_delivered: Some(now - 1700), date_read: Some(now), ..read }, false)
            });
        }
    }

    async fn send_text(self: &Arc<Self>, chat_guid: &str, text: &str, options: SendTextOptions) -> TransportResult<Message> {
        let chat = self.chat(chat_guid)?;
        let message = Message {
            guid: self.next_guid(),
            temp_guid: options.temp_guid,
            chat_guid: chat_guid.into(),
            text: text.into(),
            subject: options.subject,
            from_me: true,
            date: now_ms(),
            service: chat.service,
            reply_to: options.reply_to,
            effect: options.effect,
            ..Message::default()
        };
        tokio::time::sleep(Duration::from_millis(180)).await;
        self.push(message.clone(), true);
        self.deliver(&message);
        self.schedule_reply(chat_guid);
        Ok(message)
    }

    fn group_event(&self, chat: &Chat, event: GroupEvent) -> Message {
        Message {
            guid: self.next_guid(),
            chat_guid: chat.guid.clone(),
            from_me: true,
            date: now_ms(),
            service: chat.service,
            group_event: Some(event),
            ..Message::default()
        }
    }

    fn replace_chat(&self, chat: Chat) {
        self.state.lock().chats.insert(chat.guid.clone(), chat.clone());
        self.emit(TransportEvent::Chat(chat));
    }
}

#[async_trait]
impl Transport for DemoTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Demo
    }

    async fn connect(&self) -> TransportResult<ServerInfo> {
        self.inner.emit(TransportEvent::Connection { status: ConnectionStatus::Connecting, error: None });
        let info = ServerInfo {
            version: "demo".into(),
            macos_version: Some("15.6".into()),
            private_api: true,
            helper_connected: true,
            icloud_account: Some("you@icloud.com".into()),
        };
        self.inner.later(50, |inner| inner.emit(TransportEvent::Connection { status: ConnectionStatus::Online, error: None }));
        let contacts = self.list_contacts().await?;
        self.inner.emit(TransportEvent::Contacts(contacts));
        Ok(info)
    }

    async fn disconnect(&self) {
        let mut state = self.inner.state.lock();
        for timer in state.timers.drain(..) {
            timer.abort();
        }
    }

    fn subscribe(&self) -> mpsc::UnboundedReceiver<TransportEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.inner.state.lock().listeners.push(tx);
        rx
    }

    fn seed_contacts(&self, _contacts: &[Contact]) {}

    async fn list_chats(&self, _options: ListChatsOptions) -> TransportResult<Page<Chat>> {
        let mut items: Vec<Chat> = self.inner.state.lock().chats.values().cloned().collect();
        items.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
        Ok(Page { items, has_more: false })
    }

    async fn get_chat(&self, chat_guid: &str) -> TransportResult<Chat> {
        self.inner.chat(chat_guid)
    }

    async fn load_messages(&self, chat_guid: &str, options: LoadMessagesOptions) -> TransportResult<Page<Message>> {
        let state = self.inner.state.lock();
        let all: Vec<&Message> = state
            .messages
            .get(chat_guid)
            .map(|list| list.iter().filter(|message| options.before.is_none_or(|before| message.date < before)).collect())
            .unwrap_or_default();
        let start = all.len().saturating_sub(options.limit as usize);
        let items: Vec<Message> = all[start..].iter().map(|message| (*message).clone()).collect();
        Ok(Page { has_more: items.len() < all.len(), items })
    }

    async fn search_messages(&self, query: &str, filters: &SearchFilters) -> TransportResult<Vec<Message>> {
        let needle = query.trim().to_lowercase();
        let state = self.inner.state.lock();
        let mut out: Vec<Message> = Vec::new();
        for (chat_guid, list) in &state.messages {
            if filters.chat_guid.as_ref().is_some_and(|wanted| wanted != chat_guid) {
                continue;
            }
            if filters.chat_guids.as_ref().is_some_and(|wanted| !wanted.contains(chat_guid)) {
                continue;
            }
            for message in list {
                // Exclusive: the reconcile sweep passes the last-synced date and wants only what came after it.
                if filters.after.is_some_and(|after| message.date <= after) {
                    continue;
                }
                if filters.before.is_some_and(|before| message.date > before) {
                    continue;
                }
                if filters.from_me && !message.from_me {
                    continue;
                }
                if !filters.senders.is_empty() && !message.sender.as_ref().is_some_and(|sender| filters.senders.contains(&sender.address)) {
                    continue;
                }
                if let Some(filter) = filters.attachments {
                    if !message.attachments.iter().any(|item| matches_attachment_filter(item, filter)) {
                        continue;
                    }
                }
                if filters.links && message.url_preview.is_none() && !split_links(&message.text).iter().any(|segment| matches!(segment, TextSegment::Link { .. })) {
                    continue;
                }
                if !needle.is_empty() && !message.text.to_lowercase().contains(&needle) {
                    continue;
                }
                out.push(message.clone());
            }
        }
        out.sort_by(|a, b| b.date.cmp(&a.date));
        out.truncate(filters.limit.unwrap_or(50) as usize);
        Ok(out)
    }

    async fn list_contacts(&self) -> TransportResult<Vec<Contact>> {
        Ok(self
            .inner
            .people
            .all()
            .into_iter()
            .filter_map(|handle| {
                let name = handle.name.clone()?;
                Some(Contact { id: handle.address.clone(), name, addresses: vec![handle.address.clone()], avatar: handle.avatar.clone() })
            })
            .collect())
    }

    async fn send_text(&self, chat_guid: &str, text: &str, options: SendTextOptions) -> TransportResult<Message> {
        self.inner.send_text(chat_guid, text, options).await
    }

    async fn send_attachment(&self, chat_guid: &str, path: &Path, options: SendAttachmentOptions) -> TransportResult<Message> {
        let chat = self.inner.chat(chat_guid)?;
        let name = options
            .name
            .clone()
            .or_else(|| path.file_name().map(|name| name.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "attachment".into());
        let bytes = tokio::fs::metadata(path).await.map(|meta| meta.len()).unwrap_or(0);
        let guid = self.inner.next_guid();
        let seq = self.inner.state.lock().seq;
        let message = Message {
            guid,
            temp_guid: options.temp_guid,
            chat_guid: chat_guid.into(),
            from_me: true,
            date: now_ms(),
            service: chat.service,
            attachments: vec![Attachment {
                guid: format!("demo-upload-{seq}"),
                name,
                mime: mime_for_path(path),
                bytes,
                width: None,
                height: None,
                measured: false,
                is_sticker: false,
                local_path: Some(path.to_owned()),
                hidden: false,
                duration_ms: None,
            }],
            is_audio: options.is_audio,
            ..Message::default()
        };
        tokio::time::sleep(Duration::from_millis(400)).await;
        self.inner.push(message.clone(), true);
        self.inner.deliver(&message);
        Ok(message)
    }

    async fn attachment_path(&self, attachment_guid: &str, _options: AttachmentPathOptions) -> TransportResult<PathBuf> {
        photo_src(attachment_guid)
            .map(PathBuf::from)
            .ok_or_else(|| TransportError::Network(format!("Attachment {attachment_guid} is not in the demo set")))
    }

    async fn create_chat(&self, addresses: &[String], first_message: &str, service: Option<Service>) -> TransportResult<Chat> {
        let service = service.unwrap_or_default();
        let first = addresses.first().cloned().unwrap_or_else(|| "unknown".into());
        let prefix = match service {
            Service::IMessage => "iMessage",
            Service::Sms => "SMS",
            Service::Rcs => "RCS",
        };
        let is_group = addresses.len() > 1;
        let chat_guid = if is_group { format!("{prefix};+;chat{}", now_ms()) } else { format!("{prefix};-;{first}") };
        let participants = addresses
            .iter()
            .map(|address| {
                self.inner
                    .people
                    .all()
                    .into_iter()
                    .find(|person| &person.address == address)
                    .cloned()
                    .unwrap_or_else(|| Handle { address: address.clone(), service, name: None, avatar: None })
            })
            .collect();
        let chat = Chat {
            guid: chat_guid.clone(),
            identifier: if is_group { chat_guid.clone() } else { first },
            service,
            is_group,
            participants,
            last_activity: now_ms(),
            ..Chat::default()
        };
        {
            let mut state = self.inner.state.lock();
            state.chats.insert(chat_guid.clone(), chat.clone());
            state.messages.insert(chat_guid.clone(), Vec::new());
        }
        self.inner.emit(TransportEvent::Chat(chat.clone()));
        if !first_message.is_empty() {
            self.inner.send_text(&chat_guid, first_message, SendTextOptions::default()).await?;
        }
        Ok(self.inner.chat(&chat_guid).unwrap_or(chat))
    }

    async fn mark_read(&self, chat_guid: &str) -> TransportResult<()> {
        self.inner.emit(TransportEvent::Read { chat_guid: chat_guid.into(), read: true });
        Ok(())
    }

    async fn delete_chat(&self, chat_guid: &str) -> TransportResult<()> {
        {
            let mut state = self.inner.state.lock();
            state.chats.remove(chat_guid);
            state.messages.remove(chat_guid);
        }
        self.inner.emit(TransportEvent::ChatRemoved { chat_guid: chat_guid.into() });
        Ok(())
    }

    async fn schedule_text(&self, chat_guid: &str, text: &str, send_at: Millis) -> TransportResult<ScheduledMessage> {
        let id = {
            let mut state = self.inner.state.lock();
            state.scheduled_seq += 1;
            format!("demo-scheduled-{}", state.scheduled_seq)
        };
        let message = ScheduledMessage { id: id.clone(), chat_guid: chat_guid.into(), text: text.into(), send_at };
        self.inner.state.lock().scheduled.insert(id.clone(), message.clone());
        let inner = self.inner.clone();
        let (guid, body, key) = (chat_guid.to_owned(), text.to_owned(), id.clone());
        let wait = Duration::from_millis(u64::try_from(send_at - now_ms()).unwrap_or(0));
        let task = tokio::spawn(async move {
            tokio::time::sleep(wait).await;
            {
                let mut state = inner.state.lock();
                state.scheduled.remove(&key);
                state.scheduled_timers.remove(&key);
            }
            let _ = inner.send_text(&guid, &body, SendTextOptions::default()).await;
        });
        let mut state = self.inner.state.lock();
        state.scheduled_timers.insert(id, task.abort_handle());
        state.timers.push(task.abort_handle());
        Ok(message)
    }

    async fn list_scheduled(&self) -> TransportResult<Vec<ScheduledMessage>> {
        let mut list: Vec<ScheduledMessage> = self.inner.state.lock().scheduled.values().cloned().collect();
        list.sort_by_key(|item| item.send_at);
        Ok(list)
    }

    async fn cancel_scheduled(&self, id: &str) -> TransportResult<()> {
        let mut state = self.inner.state.lock();
        if let Some(timer) = state.scheduled_timers.remove(id) {
            timer.abort();
        }
        state.scheduled.remove(id);
        Ok(())
    }

    async fn react(&self, chat_guid: &str, message_guid: &str, kind: TapbackKind, options: ReactOptions) -> TransportResult<()> {
        let chat = self.inner.chat(chat_guid)?;
        let reaction = Message {
            guid: self.inner.next_guid(),
            chat_guid: chat_guid.into(),
            from_me: true,
            date: now_ms(),
            service: chat.service,
            reaction: Some(Reaction { target_guid: message_guid.into(), kind, emoji: options.emoji, removed: options.remove }),
            ..Message::default()
        };
        self.inner.later(250, move |inner| inner.push(reaction, false));
        Ok(())
    }

    async fn set_typing(&self, _chat_guid: &str, _typing: bool) -> TransportResult<()> {
        Ok(())
    }

    async fn mark_unread(&self, chat_guid: &str) -> TransportResult<()> {
        self.inner.emit(TransportEvent::Read { chat_guid: chat_guid.into(), read: false });
        Ok(())
    }

    async fn edit_message(&self, chat_guid: &str, message_guid: &str, text: &str, _options: EditOptions) -> TransportResult<Message> {
        let target = self.find(chat_guid, message_guid)?;
        let updated = Message { text: text.into(), date_edited: Some(now_ms()), ..target };
        self.inner.push(updated.clone(), false);
        Ok(updated)
    }

    async fn unsend_message(&self, chat_guid: &str, message_guid: &str, _part_index: Option<u32>) -> TransportResult<()> {
        let target = self.find(chat_guid, message_guid)?;
        self.inner.push(Message { date_retracted: Some(now_ms()), ..target }, false);
        Ok(())
    }

    async fn rename_group(&self, chat_guid: &str, name: &str) -> TransportResult<()> {
        let chat = self.inner.chat(chat_guid)?;
        let next = Chat { display_name: Some(name.into()), ..chat.clone() };
        self.inner.state.lock().chats.insert(chat_guid.into(), next.clone());
        self.inner.push(self.inner.group_event(&chat, GroupEvent::Rename { title: name.into() }), true);
        self.inner.replace_chat(next);
        Ok(())
    }

    async fn add_participant(&self, chat_guid: &str, address: &str) -> TransportResult<()> {
        let chat = self.inner.chat(chat_guid)?;
        let who = Handle { address: address.into(), service: chat.service, name: None, avatar: None };
        let mut next = chat.clone();
        next.participants.push(who.clone());
        self.inner.state.lock().chats.insert(chat_guid.into(), next.clone());
        self.inner.push(self.inner.group_event(&chat, GroupEvent::Join { who: Some(who) }), true);
        self.inner.replace_chat(next);
        Ok(())
    }

    async fn remove_participant(&self, chat_guid: &str, address: &str) -> TransportResult<()> {
        let chat = self.inner.chat(chat_guid)?;
        let who = chat.participants.iter().find(|item| item.address == address).cloned();
        let mut next = chat.clone();
        next.participants.retain(|item| item.address != address);
        self.inner.state.lock().chats.insert(chat_guid.into(), next.clone());
        self.inner.push(self.inner.group_event(&chat, GroupEvent::Leave { who }), true);
        self.inner.replace_chat(next);
        Ok(())
    }

    async fn leave_group(&self, chat_guid: &str) -> TransportResult<()> {
        let chat = self.inner.chat(chat_guid)?;
        self.inner.push(self.inner.group_event(&chat, GroupEvent::Leave { who: None }), true);
        Ok(())
    }

    async fn set_group_icon(&self, _chat_guid: &str, _path: &Path) -> TransportResult<()> {
        Ok(())
    }

    async fn focus_status(&self, address: &str) -> TransportResult<FocusStatus> {
        Ok(if address == self.inner.people.ben.address { FocusStatus::Silenced } else { FocusStatus::None })
    }

    async fn notify_silenced(&self, _chat_guid: &str, _message_guid: &str) -> TransportResult<()> {
        Ok(())
    }

    async fn create_facetime_link(&self) -> TransportResult<String> {
        Ok("https://facetime.apple.com/join#v=1&p=demo".into())
    }

    async fn answer_facetime(&self, _call_uuid: &str) -> TransportResult<String> {
        tokio::time::sleep(Duration::from_millis(1200)).await;
        Ok("https://facetime.apple.com/join#v=1&p=demo-call".into())
    }

    async fn leave_facetime(&self, _call_uuid: &str) -> TransportResult<()> {
        Ok(())
    }
}

impl DemoTransport {
    fn find(&self, chat_guid: &str, message_guid: &str) -> TransportResult<Message> {
        self.inner
            .state
            .lock()
            .messages
            .get(chat_guid)
            .and_then(|list| list.iter().find(|item| item.guid == message_guid).cloned())
            .ok_or_else(|| TransportError::Network("Message not found".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn serves_the_fixtures() {
        let demo = DemoTransport::new();
        let chats = demo.list_chats(ListChatsOptions::default()).await.unwrap().items;
        assert_eq!(chats.len(), 9);
        assert_eq!(chats[0].guid, "iMessage;-;+14155550134");
        let alex = demo.load_messages("iMessage;-;+14155550134", LoadMessagesOptions { limit: 50, before: None }).await.unwrap();
        assert_eq!(alex.items.len(), 13);
        assert_eq!(alex.items[12].attachments[0].mime, "image/gif");
        assert_eq!(alex.items[11].reply_to.as_deref(), Some("demo-msg-0006"));
        assert_eq!(alex.items[5].guid, "demo-msg-0006");
        assert!(chats.iter().find(|chat| chat.guid.ends_with("0199")).unwrap().unread);
        assert_eq!(demo.focus_status("+14155550170").await.unwrap(), FocusStatus::Silenced);
        let contacts = demo.list_contacts().await.unwrap();
        assert_eq!(contacts.len(), 9);
    }

    #[tokio::test(start_paused = true)]
    async fn a_send_is_echoed_delivered_and_answered() {
        let demo = DemoTransport::new();
        let mut events = demo.subscribe();
        let sent = demo
            .send_text("SMS;-;+14155550188", "hi", SendTextOptions { temp_guid: Some("temp-1".into()), ..SendTextOptions::default() })
            .await
            .unwrap();
        assert_eq!(sent.temp_guid.as_deref(), Some("temp-1"));
        tokio::time::sleep(Duration::from_secs(4)).await;
        let mut seen = Vec::new();
        while let Ok(event) = events.try_recv() {
            seen.push(event);
        }
        assert!(matches!(&seen[0], TransportEvent::Message(message) if message.guid == sent.guid));
        assert!(seen.iter().any(|event| matches!(event, TransportEvent::Message(message) if message.guid == sent.guid && message.date_delivered.is_some())));
        assert!(seen.iter().any(|event| matches!(event, TransportEvent::Message(message) if !message.from_me && message.text == "👍")));
    }

    #[tokio::test]
    async fn sweeps_everything_after_a_date() {
        let demo = DemoTransport::new();
        let now = now_ms();
        let recent = demo.search_messages("", &SearchFilters { after: Some(now - 30 * MIN), ..SearchFilters::default() }).await.unwrap();
        assert!(recent.iter().all(|message| message.date > now - 30 * MIN));
        assert_eq!(recent.len(), 7);
    }
}
