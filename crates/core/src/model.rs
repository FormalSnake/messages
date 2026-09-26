//! Field names serialize in camelCase, matching the format already on disk
//! from earlier installs (state.json and config.json).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Epoch milliseconds, the unit for every timestamp in this model.
pub type Millis = i64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Service {
    #[default]
    #[serde(rename = "iMessage")]
    IMessage,
    #[serde(rename = "SMS")]
    Sms,
    #[serde(rename = "RCS")]
    Rcs,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle {
    /// E.164 phone number or email address.
    pub address: String,
    pub service: Service,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Local file path or data URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub id: String,
    pub name: String,
    pub addresses: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TapbackKind {
    Love,
    Like,
    Dislike,
    Laugh,
    Emphasize,
    Question,
    Emoji,
}

impl TapbackKind {
    /// The six named tapbacks in picker and Cmd+1..6 order.
    pub const NAMED: [TapbackKind; 6] = [
        TapbackKind::Love,
        TapbackKind::Like,
        TapbackKind::Dislike,
        TapbackKind::Laugh,
        TapbackKind::Emphasize,
        TapbackKind::Question,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TapbackKind::Love => "love",
            TapbackKind::Like => "like",
            TapbackKind::Dislike => "dislike",
            TapbackKind::Laugh => "laugh",
            TapbackKind::Emphasize => "emphasize",
            TapbackKind::Question => "question",
            TapbackKind::Emoji => "emoji",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tapback {
    /// Guid of the reaction message itself, so a removal can find it.
    pub guid: String,
    pub kind: TapbackKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    pub from_me: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender: Option<Handle>,
}

/// What an attachment the server gave no type for is called. Every consumer reads `mime` as a string.
pub const UNKNOWN_MIME: &str = "application/octet-stream";

fn unknown_mime() -> String {
    UNKNOWN_MIME.to_owned()
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub guid: String,
    pub name: String,
    /// An older cache write left a null mime on some entries; it reads back as UNKNOWN_MIME.
    #[serde(default = "unknown_mime", deserialize_with = "mime_or_unknown")]
    pub mime: String,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// The size was read from the file header, so a re-read must not put the server's EXIF-blind one back.
    #[serde(default, skip_serializing_if = "is_false")]
    pub measured: bool,
    pub is_sticker: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<PathBuf>,
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<f64>,
}

fn mime_or_unknown<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_else(unknown_mime))
}

/// iOS 18 text effects, from __kIMTextEffectAttributeName.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextEffect {
    Big,
    Small,
    Shake,
    Nod,
    Explode,
    Ripple,
    Bloom,
    Jitter,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RichRun {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub strike: bool,
    /// Href from __kIMLinkAttributeName, not from linkifying the text ourselves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// Address of the mentioned handle; the run text is the display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mention: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<TextEffect>,
}

/// A message body split the way Messages splits it, so a photo can sit between two lines.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum MessagePart {
    Text { runs: Vec<RichRun> },
    Attachment { guid: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeliveryState {
    Sending,
    Sent,
    Delivered,
    Read,
    Failed,
}

/// `Unknown` covers a Focus the person does not share and anyone never asked about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FocusStatus {
    Silenced,
    None,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum GroupEvent {
    Rename { title: String },
    Join {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        who: Option<Handle>,
    },
    Leave {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        who: Option<Handle>,
    },
    Photo,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    pub target_guid: String,
    pub kind: TapbackKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    pub removed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UrlPreview {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_attachment_guid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub guid: String,
    /// Client-side guid used while a send is in flight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp_guid: Option<String>,
    pub chat_guid: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub from_me: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender: Option<Handle>,
    pub date: Millis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_delivered: Option<Millis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_read: Option<Millis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_edited: Option<Millis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_retracted: Option<Millis>,
    /// The recipient had a Focus on. Monterey and newer; absent from notification copies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_quietly: Option<bool>,
    /// "Notify Anyway" has already broken through that Focus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notified: Option<bool>,
    pub service: Service,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub tapbacks: Vec<Tapback>,
    /// Set only when the attributed body carries formatting, a mention, a link attribute or attachment placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<MessagePart>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    /// Guid of the message this sticker was placed on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticker_for: Option<String>,
    /// Expressive send style, for example com.apple.MobileSMS.expressivesend.impact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub is_audio: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_event: Option<GroupEvent>,
    /// Reactions arrive as messages; the store folds them into the target's `tapbacks` and never shows them as rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaction: Option<Reaction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balloon_bundle_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url_preview: Option<UrlPreview>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    pub guid: String,
    pub identifier: String,
    /// chat.db's group_id, the id Messages.app's own pin list uses for a group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    pub service: Service,
    pub is_group: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub participants: Vec<Handle>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub muted: bool,
    /// false skips the read receipt; the local unread dot still clears. Absent means true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_receipts: Option<bool>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub unread: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message: Option<Message>,
    pub last_activity: Millis,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledMessage {
    pub id: String,
    pub chat_guid: String,
    pub text: String,
    pub send_at: Millis,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macos_version: Option<String>,
    /// Private API toggle on the server. Needs SIP disabled on the Mac.
    pub private_api: bool,
    /// The helper bundle is injected into Messages.app and talking to the server.
    pub helper_connected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icloud_account: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub reactions: bool,
    pub typing: bool,
    pub read_receipts: bool,
    pub edit: bool,
    pub unsend: bool,
    pub replies: bool,
    pub effects: bool,
    pub group_management: bool,
    pub mark_unread: bool,
    pub facetime: bool,
    pub scheduled_messages: bool,
    /// Reading a person's Focus, and breaking through it, both landed in Monterey.
    pub focus_status: bool,
}

/// Leading integer of `macosVersion`, 0 when unknown.
pub fn macos_major(info: Option<&ServerInfo>) -> u32 {
    let version = info.and_then(|info| info.macos_version.as_deref()).unwrap_or("0");
    let digits: String = version.trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().unwrap_or(0)
}

pub fn capabilities_for(info: Option<&ServerInfo>) -> Capabilities {
    let private_api = info.is_some_and(|info| info.private_api && info.helper_connected);
    let major = macos_major(info);
    // On macOS 26 the helper calls an IMChat edit selector that no longer exists,
    // which crashes Messages.app and drops the helper for 30 s, and its FaceTime
    // helper does not inject at all (bluebubbles-server#776).
    let before_26 = major < 26;
    Capabilities {
        reactions: private_api,
        typing: private_api,
        read_receipts: private_api,
        edit: private_api && before_26,
        unsend: private_api,
        replies: private_api,
        effects: private_api,
        group_management: private_api,
        mark_unread: private_api,
        facetime: private_api && before_26,
        scheduled_messages: info.is_some(),
        focus_status: private_api && major >= 12,
    }
}

pub fn handle_name(handle: &Handle) -> &str {
    match handle.name.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() => name,
        _ => &handle.address,
    }
}

pub fn chat_title(chat: &Chat) -> String {
    if let Some(name) = chat.display_name.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
        return name.to_owned();
    }
    let names: Vec<&str> = chat.participants.iter().map(handle_name).collect();
    match names.len() {
        0 => chat.identifier.clone(),
        1..=3 => names.join(", "),
        n => format!("{} and {} more", names[..3].join(", "), n - 3),
    }
}

pub fn delivery_state(message: &Message) -> DeliveryState {
    if message.error.is_some() {
        return DeliveryState::Failed;
    }
    if message.temp_guid.as_deref() == Some(message.guid.as_str()) {
        return DeliveryState::Sending;
    }
    if message.date_read.is_some() {
        return DeliveryState::Read;
    }
    if message.date_delivered.is_some() {
        return DeliveryState::Delivered;
    }
    DeliveryState::Sent
}

pub fn is_visible_message(message: &Message) -> bool {
    message.reaction.is_none()
}

pub fn tapback_glyph(kind: TapbackKind, emoji: Option<&str>) -> &str {
    match kind {
        TapbackKind::Love => "\u{2764}\u{FE0F}",
        TapbackKind::Like => "\u{1F44D}",
        TapbackKind::Dislike => "\u{1F44E}",
        TapbackKind::Laugh => "\u{1F602}",
        TapbackKind::Emphasize => "\u{203C}\u{FE0F}",
        TapbackKind::Question => "\u{2753}",
        TapbackKind::Emoji => emoji.unwrap_or("\u{2764}\u{FE0F}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(macos: &str, private_api: bool, helper_connected: bool) -> ServerInfo {
        ServerInfo { version: "1.9.9".into(), macos_version: Some(macos.into()), private_api, helper_connected, icloud_account: None }
    }

    #[test]
    fn private_api_features_need_the_api_and_the_helper() {
        let off = capabilities_for(Some(&info("15.1", true, false)));
        assert!(!off.reactions && !off.typing && !off.read_receipts && !off.edit && !off.focus_status);
        assert!(off.scheduled_messages);
        let on = capabilities_for(Some(&info("15.1", true, true)));
        assert!(on.reactions && on.typing && on.read_receipts && on.edit && on.unsend && on.facetime && on.focus_status && on.mark_unread);
    }

    #[test]
    fn edit_and_facetime_are_off_on_macos_26_and_focus_needs_monterey() {
        let tahoe = capabilities_for(Some(&info("26.0.1", true, true)));
        assert!(!tahoe.edit && !tahoe.facetime);
        assert!(tahoe.unsend && tahoe.focus_status);
        let big_sur = capabilities_for(Some(&info("11.7", true, true)));
        assert!(!big_sur.focus_status && big_sur.edit);
        let none = capabilities_for(None);
        assert!(!none.scheduled_messages && !none.reactions);
    }
}
