//! Maps the icon names used across the app to gpui-kit's bundled Lucide set
//! (`gpui_kit::assets::IconName`).
//!
//! `gpui_kit::component::IconName` is a different, much smaller enum: a
//! curated set gpui-component's own widgets use internally (window controls,
//! chevrons). The full bundled Lucide set lives in `gpui_kit::assets` instead.

use std::borrow::Cow;

use gpui_kit::assets::{AllAssets, IconName as Glyph};
use gpui_kit::{App, AssetSource, Hsla, IntoElement, Pixels, RenderOnce, SharedString, Styled, Window, px, svg};

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

/// Prefix of the icon paths `IconAssets` rewrites: it bakes Lucide's stroke
/// down from 2 to 1.5 beside regular copy, keeps 2 for `strong`, and fills
/// the favorite heart.
const VARIANT_PREFIX: &str = "messages-icon/";

/// The bundled asset set, plus stroke and fill variants of its Lucide glyphs
/// under `messages-icon/{regular,strong,filled}/<path>`.
pub struct IconAssets;

impl AssetSource for IconAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        let Some((variant, original)) = path.strip_prefix(VARIANT_PREFIX).and_then(|rest| rest.split_once('/')) else {
            return AllAssets.load(path);
        };
        let Some(source) = AllAssets.load(original)? else { return Ok(None) };
        let source = String::from_utf8_lossy(&source);
        let baked = match variant {
            "strong" => source.into_owned(),
            "filled" => source.replace("fill=\"none\"", "fill=\"currentColor\"").replace("stroke-width=\"2\"", "stroke-width=\"1.5\""),
            _ => source.replace("stroke-width=\"2\"", "stroke-width=\"1.5\""),
        };
        Ok(Some(Cow::Owned(baked.into_bytes())))
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        AllAssets.list(path)
    }
}

/// The icon element (and the favorite heart's filled variant): stroke 1.5,
/// 2 when `strong`.
#[derive(IntoElement)]
pub struct Icon {
    name: IconName,
    size: Pixels,
    color: Hsla,
    strong: bool,
    filled: bool,
}

impl Icon {
    pub fn new(name: IconName) -> Self {
        Self { name, size: px(16.), color: Hsla::transparent_black(), strong: false, filled: false }
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

    pub fn filled(mut self, filled: bool) -> Self {
        self.filled = filled;
        self
    }
}

pub fn icon_path(name: IconName, strong: bool, filled: bool) -> SharedString {
    let variant = if filled { "filled" } else if strong { "strong" } else { "regular" };
    format!("{VARIANT_PREFIX}{variant}/{}", glyph(name).path()).into()
}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        svg().path(icon_path(self.name, self.strong, self.filled)).flex_shrink_0().size(self.size).text_color(self.color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn variants_rewrite_the_lucide_source() {
        let load = |path: SharedString| String::from_utf8(IconAssets.load(&path).unwrap().unwrap().into_owned()).unwrap();
        assert!(load(icon_path(IconName::Heart, false, false)).contains("stroke-width=\"1.5\""));
        assert!(load(icon_path(IconName::Heart, true, false)).contains("stroke-width=\"2\""));
        assert!(load(icon_path(IconName::Heart, false, true)).contains("fill=\"currentColor\""));
    }
}
