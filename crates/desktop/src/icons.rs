//! Maps the icon names used by the TS reference (`apps/desktop/src/ui/icons.tsx`)
//! to gpui-kit's bundled Lucide set (`gpui_kit::assets::IconName`).
//!
//! `gpui_kit::component::IconName` is a different, much smaller enum: a
//! curated set gpui-component's own widgets use internally (window controls,
//! chevrons). The full bundled Lucide set lives in `gpui_kit::assets` instead.

use gpui_kit::{App, Hsla, IntoElement, Pixels, RenderOnce, Styled, Window, px, svg};
use gpui_kit::assets::IconName as Glyph;

/// Names the rest of the UI asks for, independent of which glyph backs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconName {
    Search,
    Compose,
    Plus,
    Send,
    Info,
    Video,
    Phone,
    ChevronLeft,
    ChevronRight,
    ChevronDown,
    Image,
    Paperclip,
    Close,
    Check,
    Reply,
    Edit,
    Trash,
    Copy,
    More,
    Pin,
    PinOff,
    Mute,
    Unmute,
    Settings,
    Online,
    Offline,
    Refresh,
    Alert,
    Group,
    Person,
    Eye,
    EyeOff,
    MarkRead,
    MarkUnread,
    Effect,
    Sparkles,
    Tapback,
    AddTapback,
    Gif,
    Leave,
    File,
    Audio,
    Lock,
    Unlock,
    Open,
    Download,
    Conversation,
    RemovePerson,
    Schedule,
    Heart,
    Silenced,
}

/// Resolves to the bundled glyph. `Trash` lands on Lucide's single `trash` can
/// (the set has no `trash-2`); `Tapback`/`AddTapback` land on GPUI Kit's own
/// `face-slightly-smiling[-plus]`, the set's stand-ins for Lucide's
/// `smile`/`smile-plus`, which the bundle does not carry.
pub fn glyph(name: IconName) -> Glyph {
    use IconName::*;
    match name {
        Search => Glyph::Search,
        Compose => Glyph::SquarePen,
        Plus => Glyph::Plus,
        Send => Glyph::ArrowUp,
        Info => Glyph::Info,
        Video => Glyph::Video,
        Phone => Glyph::Phone,
        ChevronLeft => Glyph::ChevronLeft,
        ChevronRight => Glyph::ChevronRight,
        ChevronDown => Glyph::ChevronDown,
        Image => Glyph::Image,
        Paperclip => Glyph::Paperclip,
        Close => Glyph::X,
        Check => Glyph::Check,
        Reply => Glyph::Reply,
        Edit => Glyph::Pencil,
        Trash => Glyph::Trash,
        Copy => Glyph::Copy,
        More => Glyph::Ellipsis,
        Pin => Glyph::Pin,
        PinOff => Glyph::PinOff,
        Mute => Glyph::BellOff,
        Unmute => Glyph::Bell,
        Settings => Glyph::Settings,
        Online => Glyph::Wifi,
        Offline => Glyph::WifiOff,
        Refresh => Glyph::RefreshCw,
        Alert => Glyph::CircleAlert,
        Group => Glyph::Users,
        Person => Glyph::User,
        Eye => Glyph::Eye,
        EyeOff => Glyph::EyeOff,
        MarkRead => Glyph::MailOpen,
        MarkUnread => Glyph::Mail,
        Effect => Glyph::Sparkles,
        Sparkles => Glyph::Sparkles,
        Tapback => Glyph::FaceSlightlySmiling,
        AddTapback => Glyph::FaceSlightlySmilingPlus,
        Gif => Glyph::ImagePlay,
        Leave => Glyph::LogOut,
        File => Glyph::File,
        Audio => Glyph::Mic,
        Lock => Glyph::Lock,
        Unlock => Glyph::LockOpen,
        Open => Glyph::ExternalLink,
        Download => Glyph::Download,
        Conversation => Glyph::MessageSquare,
        RemovePerson => Glyph::UserMinus,
        Schedule => Glyph::Clock,
        Heart => Glyph::Heart,
        Silenced => Glyph::Moon,
    }
}

/// Port of `apps/desktop/src/ui/icons.tsx`'s `<Icon>`. TS bakes a stroke
/// weight into the SVG source at 1.5 (2.0 for `strong`); the bundled glyphs
/// here ship at a single fixed stroke, so `strong` is accepted for call-site
/// parity but does not yet change the rendered weight.
#[derive(IntoElement)]
pub struct Icon {
    name: IconName,
    size: Pixels,
    color: Hsla,
    strong: bool,
}

impl Icon {
    pub fn new(name: IconName) -> Self {
        Self { name, size: px(16.), color: Hsla::transparent_black(), strong: false }
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = color;
        self
    }

    pub fn strong(mut self, strong: bool) -> Self {
        self.strong = strong;
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        svg().path(glyph(self.name).path()).flex_shrink_0().size(self.size).text_color(self.color)
    }
}
