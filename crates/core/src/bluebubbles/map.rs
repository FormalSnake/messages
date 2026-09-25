//! Port of packages/core/src/bluebubbles/map.ts: raw server JSON to the model.
//! Stays synchronous; the transport precomputes paths, services and icons.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::model::{
    Attachment, Chat, Contact, GroupEvent, Handle, Message, MessagePart, Reaction, RichRun, ServerInfo, Service, TapbackKind, TextEffect,
    UrlPreview, UNKNOWN_MIME,
};

/// One bad field type must not fail a whole page, so null and any JSON number read as the default.
fn int_or_zero<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(Value::Number(number)) => number.as_i64().or_else(|| number.as_f64().map(|value| value as i64)).unwrap_or(0),
        Some(Value::Bool(flag)) => i64::from(flag),
        _ => 0,
    })
}

fn opt_int<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<i64>, D::Error> {
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(Value::Number(number)) => number.as_i64().or_else(|| number.as_f64().map(|value| value as i64)),
        _ => None,
    })
}

fn opt_u32<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<u32>, D::Error> {
    Ok(opt_int(deserializer)?.and_then(|value| u32::try_from(value).ok()))
}

fn u64_or_zero<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    Ok(u64::try_from(int_or_zero(deserializer)?).unwrap_or(0))
}

fn or_default<'de, D: serde::Deserializer<'de>, T: Deserialize<'de> + Default>(deserializer: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// A string field some builds send as something else; anything but a string reads as absent.
fn opt_string<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(Value::String(text)) => Some(text),
        _ => None,
    })
}

fn opt_bool<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<bool>, D::Error> {
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(Value::Bool(flag)) => Some(flag),
        Some(Value::Number(number)) => Some(number.as_f64() != Some(0.0)),
        _ => None,
    })
}

fn bool_or_false<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Ok(opt_bool(deserializer)?.unwrap_or(false))
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawHandle {
    #[serde(default, rename = "originalROWID", deserialize_with = "int_or_zero")]
    pub original_rowid: i64,
    #[serde(default, deserialize_with = "or_default")]
    pub address: String,
    #[serde(default, deserialize_with = "or_default")]
    pub service: String,
    #[serde(default, deserialize_with = "opt_string")]
    pub country: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    pub uncanonicalized_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RawAttachmentMetadata {
    /// Seconds, from macOS mdls; only set on audio attachments.
    #[serde(default, deserialize_with = "opt_f64")]
    pub duration: Option<f64>,
}

fn opt_f64<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(Value::Number(number)) => number.as_f64(),
        _ => None,
    })
}

fn opt_metadata<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<RawAttachmentMetadata>, D::Error> {
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(value @ Value::Object(_)) => RawAttachmentMetadata::deserialize(value).ok(),
        _ => None,
    })
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawAttachment {
    #[serde(default, rename = "originalROWID", deserialize_with = "int_or_zero")]
    pub original_rowid: i64,
    pub guid: String,
    #[serde(default, deserialize_with = "opt_u32")]
    pub height: Option<u32>,
    #[serde(default, deserialize_with = "opt_u32")]
    pub width: Option<u32>,
    #[serde(default, deserialize_with = "opt_string")]
    pub uti: Option<String>,
    /// None for an attachment chat.db has no type for, e.g. the brand logo on an RCS business message.
    #[serde(default, deserialize_with = "opt_string")]
    pub mime_type: Option<String>,
    #[serde(default, deserialize_with = "u64_or_zero")]
    pub total_bytes: u64,
    #[serde(default, deserialize_with = "opt_string")]
    pub transfer_name: Option<String>,
    #[serde(default, deserialize_with = "opt_bool")]
    pub is_sticker: Option<bool>,
    #[serde(default, deserialize_with = "opt_bool")]
    pub hide_attachment: Option<bool>,
    #[serde(default, deserialize_with = "opt_bool")]
    pub has_live_photo: Option<bool>,
    #[serde(default, deserialize_with = "opt_metadata")]
    pub metadata: Option<RawAttachmentMetadata>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawChat {
    #[serde(default, rename = "originalROWID", deserialize_with = "int_or_zero")]
    pub original_rowid: i64,
    pub guid: String,
    #[serde(default)]
    pub participants: Option<Vec<RawHandle>>,
    #[serde(default)]
    pub last_message: Option<Box<RawMessage>>,
    /// 43 is a group.
    #[serde(default, deserialize_with = "int_or_zero")]
    pub style: i64,
    #[serde(default, deserialize_with = "or_default")]
    pub chat_identifier: String,
    #[serde(default, deserialize_with = "bool_or_false")]
    pub is_archived: bool,
    #[serde(default, deserialize_with = "opt_string")]
    pub display_name: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    pub group_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawMessage {
    #[serde(default, rename = "originalROWID", deserialize_with = "int_or_zero")]
    pub original_rowid: i64,
    #[serde(default, deserialize_with = "opt_string")]
    pub temp_guid: Option<String>,
    pub guid: String,
    #[serde(default, deserialize_with = "opt_string")]
    pub text: Option<String>,
    #[serde(default)]
    pub attributed_body: Option<Value>,
    #[serde(default)]
    pub handle: Option<RawHandle>,
    #[serde(default, deserialize_with = "int_or_zero")]
    pub handle_id: i64,
    #[serde(default)]
    pub chats: Option<Vec<RawChat>>,
    #[serde(default)]
    pub attachments: Option<Vec<RawAttachment>>,
    #[serde(default, deserialize_with = "opt_string")]
    pub subject: Option<String>,
    #[serde(default, deserialize_with = "int_or_zero")]
    pub error: i64,
    #[serde(default, deserialize_with = "int_or_zero")]
    pub date_created: i64,
    #[serde(default, deserialize_with = "opt_int")]
    pub date_read: Option<i64>,
    #[serde(default, deserialize_with = "opt_int")]
    pub date_delivered: Option<i64>,
    #[serde(default, deserialize_with = "bool_or_false")]
    pub is_from_me: bool,
    #[serde(default, deserialize_with = "opt_bool")]
    pub is_audio_message: Option<bool>,
    #[serde(default, deserialize_with = "int_or_zero")]
    pub item_type: i64,
    #[serde(default, deserialize_with = "opt_string")]
    pub group_title: Option<String>,
    #[serde(default, deserialize_with = "int_or_zero")]
    pub group_action_type: i64,
    #[serde(default, deserialize_with = "opt_string")]
    pub balloon_bundle_id: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    pub associated_message_guid: Option<String>,
    /// A decoded name ("love", "-like", "sticker") or the raw chat.db integer as a string:
    /// "1000" is also a sticker, "2000"-"2005"/"3000"-"3005" the six tapbacks added/removed,
    /// any other 2xxx/3xxx a macOS 15+ custom emoji tapback the server never decodes.
    #[serde(default, deserialize_with = "string_or_number")]
    pub associated_message_type: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    pub expressive_send_style_id: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    pub thread_originator_guid: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub thread_originator_part: Option<String>,
    #[serde(default, deserialize_with = "opt_int")]
    pub date_retracted: Option<i64>,
    #[serde(default, deserialize_with = "opt_int")]
    pub date_edited: Option<i64>,
    /// Monterey and newer, and left out of the notification serializer.
    #[serde(default, deserialize_with = "opt_bool")]
    pub was_delivered_quietly: Option<bool>,
    #[serde(default, deserialize_with = "opt_bool")]
    pub did_notify_recipient: Option<bool>,
    #[serde(default, deserialize_with = "opt_int")]
    pub part_count: Option<i64>,
    #[serde(default)]
    pub payload_data: Option<Value>,
}

fn string_or_number<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(Value::String(text)) => Some(text),
        Some(Value::Number(number)) => Some(number.to_string()),
        _ => None,
    })
}

#[derive(Clone, Debug, Deserialize)]
pub struct RawServerInfo {
    #[serde(default, deserialize_with = "or_default")]
    pub os_version: String,
    #[serde(default, deserialize_with = "or_default")]
    pub server_version: String,
    #[serde(default, deserialize_with = "bool_or_false")]
    pub private_api: bool,
    #[serde(default, deserialize_with = "bool_or_false")]
    pub helper_connected: bool,
    #[serde(default, deserialize_with = "opt_string")]
    pub detected_icloud: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RawContactAddress {
    #[serde(default, deserialize_with = "opt_string")]
    pub address: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawContact {
    #[serde(default)]
    pub id: Value,
    #[serde(default, deserialize_with = "opt_string")]
    pub first_name: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    pub last_name: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    pub display_name: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    pub nickname: Option<String>,
    #[serde(default, deserialize_with = "or_default")]
    pub phone_numbers: Vec<RawContactAddress>,
    #[serde(default, deserialize_with = "or_default")]
    pub emails: Vec<RawContactAddress>,
    /// Bare base64 JPEG, no data: prefix.
    #[serde(default, deserialize_with = "opt_string")]
    pub avatar: Option<String>,
}

impl RawContact {
    /// `String(raw.id)`: the id as the TS client wrote it into file names and state.json.
    pub fn id_string(&self) -> String {
        match &self.id {
            Value::String(text) => text.clone(),
            Value::Number(number) => number.as_i64().map(|id| id.to_string()).unwrap_or_else(|| number.to_string()),
            other => other.to_string(),
        }
    }
}

#[derive(Default)]
pub struct MapOptions<'a> {
    pub contacts: Option<&'a ContactIndex>,
    /// Attachment guid to local path, precomputed by the transport.
    pub attachment_paths: Option<&'a HashMap<String, PathBuf>>,
    /// Service for a message with no handle (sent by me).
    pub chat_service: Option<Service>,
    /// Local path to the group photo.
    pub chat_icon: Option<String>,
}

fn hex_value(byte: u8) -> Option<u16> {
    (byte as char).to_digit(16).map(|digit| digit as u16)
}

/// The code unit of a `\uXXXX` escape starting at `at` (the backslash), if there is one.
fn escape_at(body: &[u8], at: usize) -> Option<u16> {
    if body.get(at) != Some(&b'\\') || body.get(at + 1) != Some(&b'u') {
        return None;
    }
    let digits = body.get(at + 2..at + 6)?;
    digits.iter().try_fold(0u16, |acc, &byte| Some(acc << 4 | hex_value(byte)?))
}

/// serde_json rejects a lone UTF-16 surrogate escape and the whole page with it,
/// and the server sends them on live data. Rewrites each lone `\uD8xx`-`\uDFxx`
/// escape in a response body to `�` before parsing. Borrowed when there is none.
pub fn repair_lone_surrogates(body: &[u8]) -> Cow<'_, [u8]> {
    const REPLACEMENT: &[u8] = b"\\uFFFD";
    let mut out: Option<Vec<u8>> = None;
    let mut copied = 0;
    let mut index = 0;
    while index < body.len() {
        if body[index] != b'\\' {
            index += 1;
            continue;
        }
        let Some(unit) = escape_at(body, index) else {
            // Any other escape is two bytes; skipping both keeps `\\u` from reading as an escape.
            index += 2;
            continue;
        };
        let lone = match unit {
            0xd800..=0xdbff => match escape_at(body, index + 6) {
                Some(0xdc00..=0xdfff) => {
                    index += 12;
                    continue;
                }
                _ => true,
            },
            0xdc00..=0xdfff => true,
            _ => false,
        };
        if lone {
            let buffer = out.get_or_insert_with(|| Vec::with_capacity(body.len()));
            buffer.extend_from_slice(&body[copied..index]);
            buffer.extend_from_slice(REPLACEMENT);
            copied = index + 6;
        }
        index += 6;
    }
    match out {
        None => Cow::Borrowed(body),
        Some(mut buffer) => {
            buffer.extend_from_slice(&body[copied..]);
            Cow::Owned(buffer)
        }
    }
}

/// Parses a server body, lone surrogates repaired first.
pub fn parse_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> serde_json::Result<T> {
    serde_json::from_slice(&repair_lone_surrogates(body))
}

fn to_service(service: &str) -> Service {
    match service {
        "SMS" => Service::Sms,
        "RCS" => Service::Rcs,
        _ => Service::IMessage,
    }
}

fn js_trim_start(text: &str) -> &str {
    text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

fn js_trim_end(text: &str) -> &str {
    text.trim_end_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

/// Messages stores U+FFFC where an attachment sits, so an attachment-only message's text is a lone placeholder.
fn clean_text(value: &str) -> String {
    js_trim_end(js_trim_start(&value.replace('\u{fffc}', ""))).to_owned()
}

fn prefix_service(chat_guid: &str) -> Option<Service> {
    match chat_guid.split(';').next() {
        Some("iMessage") => Some(Service::IMessage),
        Some("SMS") => Some(Service::Sms),
        Some("RCS") => Some(Service::Rcs),
        _ => None,
    }
}

fn service_from_participants(participants: Option<&[RawHandle]>) -> Service {
    let services: HashSet<Service> = participants.unwrap_or_default().iter().map(|handle| to_service(&handle.service)).collect();
    [Service::IMessage, Service::Rcs, Service::Sms]
        .into_iter()
        .find(|service| services.contains(service))
        .unwrap_or(Service::IMessage)
}

/// Pre-macOS 26 guids carry the service; on macOS 26 every chat guid is `any;-;...`
/// and the service has to come from the participants instead.
fn chat_service(chat_guid: &str, participants: Option<&[RawHandle]>) -> Service {
    prefix_service(chat_guid).unwrap_or_else(|| service_from_participants(participants))
}

/// chat.db stores reaction and reply targets as `p:<part>/<guid>` or `bp:<guid>`.
fn strip_guid_prefix(guid: &str) -> &str {
    if let Some(rest) = guid.strip_prefix("p:") {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && rest.as_bytes().get(digits) == Some(&b'/') {
            return &rest[digits + 1..];
        }
    }
    guid.strip_prefix("bp:").unwrap_or(guid)
}

fn named_reaction(name: &str) -> Option<TapbackKind> {
    TapbackKind::NAMED.into_iter().find(|kind| kind.as_str() == name)
}

/// A custom emoji tapback (macOS 15+) is only visible in the text ("Reacted 🔥 to ..."),
/// worded per locale, so the first emoji cluster is taken rather than the words around it.
fn extract_emoji(text: &str) -> Option<&str> {
    static EMOJI_CLUSTER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"\p{Extended_Pictographic}(?:\x{FE0F}|\p{Emoji_Modifier})?(?:\x{200D}\p{Extended_Pictographic}(?:\x{FE0F}|\p{Emoji_Modifier})?)*",
        )
        .unwrap()
    });
    EMOJI_CLUSTER.find(text).map(|found| found.as_str())
}

/// JS `Number(text)` for the integer strings this field carries; anything else is NaN.
fn js_number(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Some(0.0);
    }
    trimmed.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn parse_associated_message(raw: &RawMessage) -> (Option<Reaction>, Option<String>) {
    let (Some(kind), Some(target)) = (raw.associated_message_type.as_deref(), raw.associated_message_guid.as_deref()) else {
        return (None, None);
    };
    if kind.is_empty() || target.is_empty() {
        return (None, None);
    }
    let target_guid = strip_guid_prefix(target).to_owned();
    if kind == "sticker" || kind == "1000" {
        return (None, Some(target_guid));
    }
    let removed = kind.starts_with('-');
    let name = if removed { &kind[1..] } else { kind };
    if let Some(named) = named_reaction(name) {
        return (Some(Reaction { target_guid, kind: named, emoji: None, removed }), None);
    }
    let Some(numeric) = js_number(name) else {
        return (None, None);
    };
    let (base, removed) = if (2000.0..3000.0).contains(&numeric) {
        (2000.0, false)
    } else if (3000.0..4000.0).contains(&numeric) {
        (3000.0, true)
    } else {
        return (None, None);
    };
    let offset = numeric - base;
    let named = (offset.fract() == 0.0).then(|| TapbackKind::NAMED.get(offset as usize).copied()).flatten();
    let reaction = match named {
        Some(kind) => Reaction { target_guid, kind, emoji: None, removed },
        None => Reaction {
            target_guid,
            kind: TapbackKind::Emoji,
            emoji: Some(extract_emoji(raw.text.as_deref().unwrap_or("")).unwrap_or("\u{2764}\u{FE0F}").to_owned()),
            removed,
        },
    };
    (Some(reaction), None)
}

fn parse_group_event(raw: &RawMessage, who: Option<&Handle>) -> Option<GroupEvent> {
    let who = who.cloned();
    match raw.item_type {
        1 if raw.group_action_type == 1 => Some(GroupEvent::Leave { who }),
        1 => Some(GroupEvent::Join { who }),
        2 => Some(GroupEvent::Rename { title: raw.group_title.clone().unwrap_or_default() }),
        3 if raw.group_action_type == 1 => Some(GroupEvent::Photo),
        3 => Some(GroupEvent::Leave { who }),
        _ => None,
    }
}

fn uid_ref(value: &Value) -> Option<usize> {
    let object = value.as_object()?;
    if object.len() != 1 {
        return None;
    }
    let uid = object.get("UID")?.as_f64()?;
    Some(if uid >= 0.0 && uid.fract() == 0.0 { uid as usize } else { usize::MAX })
}

/// Resolves an NSKeyedArchiver plist (already JSON) into plain values:
/// `{UID: n}` followed into `$objects`, `"$null"` to null, `$class` dropped,
/// NSURL/NSString wrappers collapsed to their string and NSArray/NSDictionary
/// to a plain array/object.
pub fn decode_keyed_archive(archive: &Value) -> Value {
    struct Walker<'a> {
        objects: &'a [Value],
        in_progress: HashSet<usize>,
    }

    impl Walker<'_> {
        /// None is JS `undefined`: a cycle or a reference past the end.
        fn walk(&mut self, value: Option<&Value>) -> Option<Value> {
            let value = value?;
            if let Some(index) = uid_ref(value) {
                if !self.in_progress.insert(index) {
                    return None;
                }
                let objects = self.objects;
                let resolved = self.walk(objects.get(index));
                self.in_progress.remove(&index);
                return resolved;
            }
            match value {
                Value::String(text) if text == "$null" => Some(Value::Null),
                Value::Array(items) => Some(Value::Array(items.iter().map(|item| self.walk(Some(item)).unwrap_or(Value::Null)).collect())),
                Value::Object(object) => {
                    if let Some(inner) = object.get("NS.string") {
                        return self.walk(Some(inner));
                    }
                    if let Some(inner) = object.get("NS.relative") {
                        return self.walk(Some(inner));
                    }
                    let list = |this: &mut Self, key: &str| -> Vec<Option<Value>> {
                        object.get(key).and_then(Value::as_array).map(|items| items.iter().map(|item| this.walk(Some(item))).collect()).unwrap_or_default()
                    };
                    if object.contains_key("NS.keys") && object.contains_key("NS.objects") {
                        let keys = list(self, "NS.keys");
                        let values = list(self, "NS.objects");
                        let mut out = Map::new();
                        for (key, value) in keys.into_iter().zip(values) {
                            if let (Some(Value::String(key)), Some(value)) = (key, value) {
                                out.insert(key, value);
                            }
                        }
                        return Some(Value::Object(out));
                    }
                    if object.contains_key("NS.objects") {
                        return Some(Value::Array(list(self, "NS.objects").into_iter().map(Option::unwrap_or_default).collect()));
                    }
                    let mut out = Map::new();
                    for (key, value) in object {
                        if key == "$class" {
                            continue;
                        }
                        if let Some(value) = self.walk(Some(value)) {
                            out.insert(key.clone(), value);
                        }
                    }
                    Some(Value::Object(out))
                }
                other => Some(other.clone()),
            }
        }
    }

    let objects = archive.get("$objects").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let root = archive.get("$top").and_then(|top| top.get("root"));
    Walker { objects, in_progress: HashSet::new() }.walk(root).unwrap_or(Value::Null)
}

fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
        Value::String(text) => !text.is_empty(),
        _ => true,
    }
}

fn non_null<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|value| !value.is_null())
}

/// `payloadData` is a one-element array holding the NSKeyedArchiver plist. The
/// preview image is not in the archive; it is one of the message's own
/// attachments, named by `richLinkImageAttachmentSubstituteIndex`.
pub fn to_url_preview(payload_data: &Value, attachments: &[RawAttachment]) -> Option<UrlPreview> {
    let archive = match payload_data {
        Value::Array(items) => items.first()?,
        other => other,
    };
    if !js_truthy(archive) {
        return None;
    }
    let decoded = decode_keyed_archive(archive);
    let metadata = non_null(&decoded, "richLinkMetadata")?;
    let url = non_null(metadata, "originalURL").or_else(|| non_null(metadata, "URL"))?.as_str().filter(|url| !url.is_empty())?;
    let substitute = |key: &str| non_null(metadata, key).and_then(|image| non_null(image, "richLinkImageAttachmentSubstituteIndex"));
    let image_attachment_guid = substitute("image")
        .or_else(|| substitute("icon"))
        .and_then(Value::as_f64)
        .filter(|index| *index >= 0.0 && index.fract() == 0.0)
        .and_then(|index| attachments.get(index as usize))
        .map(|attachment| attachment.guid.clone());
    let text = |key: &str| metadata.get(key).and_then(Value::as_str).map(str::to_owned);
    Some(UrlPreview {
        url: url.to_owned(),
        title: text("title"),
        summary: text("summary"),
        image_path: None,
        image_attachment_guid,
        site_name: text("siteName"),
    })
}

fn text_effect(code: f64) -> Option<TextEffect> {
    Some(match code as i64 {
        _ if code.fract() != 0.0 => return None,
        4 => TextEffect::Ripple,
        5 => TextEffect::Big,
        6 => TextEffect::Bloom,
        8 => TextEffect::Nod,
        9 => TextEffect::Shake,
        10 => TextEffect::Jitter,
        11 => TextEffect::Small,
        12 => TextEffect::Explode,
        _ => return None,
    })
}

/// The formatting attributes arrive as the integer 1; older bodies use a string.
fn is_on(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Number(number)) => number.as_f64() == Some(1.0),
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(text)) => text == "1",
        _ => false,
    }
}

/// NSURL survives the server's archive decode as a plain string, but not always.
fn to_link(value: Option<&Value>) -> Option<String> {
    let text = match value? {
        Value::String(text) => text,
        Value::Object(object) => object.get("NS.relative")?.as_str()?,
        _ => return None,
    };
    (!text.is_empty()).then(|| text.to_owned())
}

fn style_of(attributes: &Map<String, Value>, text: String) -> RichRun {
    RichRun {
        text,
        bold: is_on(attributes.get("__kIMTextBoldAttributeName")),
        italic: is_on(attributes.get("__kIMTextItalicAttributeName")),
        underline: is_on(attributes.get("__kIMTextUnderlineAttributeName")),
        strike: is_on(attributes.get("__kIMTextStrikethroughAttributeName")),
        link: to_link(attributes.get("__kIMLinkAttributeName")),
        mention: attributes.get("__kIMMentionConfirmedMention").and_then(Value::as_str).filter(|mention| !mention.is_empty()).map(str::to_owned),
        effect: attributes.get("__kIMTextEffectAttributeName").and_then(Value::as_f64).and_then(text_effect),
    }
}

fn is_plain(run: &RichRun) -> bool {
    !run.bold && !run.italic && !run.underline && !run.strike && run.link.is_none() && run.mention.is_none() && run.effect.is_none()
}

fn same_style(a: &RichRun, b: &RichRun) -> bool {
    a.bold == b.bold && a.italic == b.italic && a.underline == b.underline && a.strike == b.strike && a.link == b.link && a.mention == b.mention && a.effect == b.effect
}

fn merge_runs(runs: Vec<RichRun>) -> Vec<RichRun> {
    let mut out: Vec<RichRun> = Vec::new();
    for run in runs {
        match out.last_mut() {
            Some(previous) if same_style(previous, &run) => previous.text.push_str(&run.text),
            _ => out.push(run),
        }
    }
    // Trimmed like `text`, so the newline Messages leaves around an attachment does not open a blank line.
    if let Some(first) = out.first_mut() {
        first.text = js_trim_start(&first.text).to_owned();
    }
    if let Some(last) = out.last_mut() {
        last.text = js_trim_end(&last.text).to_owned();
    }
    out.retain(|run| !run.text.is_empty());
    out
}

/// None for a body that says nothing `text` does not already say. Ranges are UTF-16 code units.
pub fn to_parts(attributed_body: &Value, attachments: &[RawAttachment]) -> Option<Vec<MessagePart>> {
    let body = match attributed_body {
        Value::Array(items) => items.first()?,
        other => other,
    };
    let source: Vec<u16> = body.get("string")?.as_str()?.encode_utf16().collect();
    let runs = body.get("runs")?.as_array().filter(|runs| !runs.is_empty())?;

    let known: HashSet<&str> = attachments.iter().map(|attachment| attachment.guid.as_str()).collect();
    let mut parts = Vec::new();
    let mut pending: Vec<RichRun> = Vec::new();
    let mut pending_part: Option<f64> = None;
    let empty = Map::new();

    let flush = |pending: &mut Vec<RichRun>, pending_part: &mut Option<f64>, parts: &mut Vec<MessagePart>| {
        if pending.is_empty() {
            return;
        }
        let merged = merge_runs(std::mem::take(pending));
        if !merged.is_empty() {
            parts.push(MessagePart::Text { runs: merged });
        }
        *pending_part = None;
    };

    for run in runs {
        let attributes = run.get("attributes").and_then(Value::as_object).unwrap_or(&empty);
        let range = run.get("range");
        let number = |index: usize| range.and_then(|range| range.get(index)).and_then(Value::as_f64);
        let (Some(start), Some(length)) = (number(0), number(1)) else {
            continue;
        };

        if let Some(transfer) = attributes.get("__kIMFileTransferGUIDAttributeName").and_then(Value::as_str) {
            // An attachment the query did not ask for cannot be rendered; drop its placeholder.
            if !known.contains(transfer) {
                continue;
            }
            flush(&mut pending, &mut pending_part, &mut parts);
            parts.push(MessagePart::Attachment { guid: transfer.to_owned() });
            continue;
        }

        let from = (start.max(0.0) as usize).min(source.len());
        let to = ((start + length).max(0.0) as usize).clamp(from, source.len());
        let text = String::from_utf16_lossy(&source[from..to]).replace('\u{fffc}', "");
        if text.is_empty() {
            continue;
        }

        if let Some(part_index) = attributes.get("__kIMMessagePartAttributeName").and_then(Value::as_f64) {
            if pending_part.is_some_and(|pending| pending != part_index) {
                flush(&mut pending, &mut pending_part, &mut parts);
            }
            pending_part = Some(part_index);
        }
        pending.push(style_of(attributes, text));
    }
    flush(&mut pending, &mut pending_part, &mut parts);

    match parts.as_slice() {
        [] => None,
        [MessagePart::Text { runs }] if runs.iter().all(is_plain) => None,
        _ => Some(parts),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadPlan {
    /// `original=true` on the download: everything but HEIC/HEIF/TIFF and CAF, and always for stickers.
    pub original: bool,
    /// Cache entry extension including the dot: `.jpg` for HEIC/TIFF, `.m4a` for CAF, `.png` for stickers.
    pub extension: String,
    pub sticker: bool,
}

/// Node's `path.extname`: from the last dot of the last segment, empty for a dotfile or no dot.
fn extname(name: &str) -> &str {
    let base = name.rsplit('/').next().unwrap_or(name);
    match base.rfind('.') {
        Some(index) if base[..index].bytes().any(|byte| byte != b'.') => &base[index..],
        _ => "",
    }
}

/// `GET /attachment/:guid/download` converts HEIC/HEIF/TIFF to JPEG and CAF to
/// AAC only without `original=true`; everything else is byte-identical either
/// way. A sticker is the exception: sips flattens its alpha plane onto black,
/// so the original is fetched and converted client-side, and the entry is
/// `.png` whatever the bytes turn out to be.
pub fn download_plan(name: Option<&str>, mime: Option<&str>, sticker: bool) -> DownloadPlan {
    if sticker {
        return DownloadPlan { original: true, extension: ".png".to_owned(), sticker: true };
    }
    let ext = name.map(|name| extname(name).to_lowercase()).unwrap_or_default();
    let is_image = mime.is_some_and(|mime| mime.starts_with("image/"));
    if matches!(ext.as_str(), ".heic" | ".heif" | ".tif" | ".tiff") {
        return DownloadPlan { original: false, extension: ".jpg".to_owned(), sticker: false };
    }
    if mime == Some("audio/x-caf") || ext == ".caf" {
        return DownloadPlan { original: false, extension: ".m4a".to_owned(), sticker: false };
    }
    DownloadPlan { original: !is_image, extension: ext, sticker: false }
}

pub fn to_handle(raw: &RawHandle, contacts: Option<&ContactIndex>) -> Handle {
    Handle {
        address: raw.address.clone(),
        service: to_service(&raw.service),
        name: contacts.and_then(|contacts| contacts.resolve(&raw.address)).map(str::to_owned),
        avatar: contacts.and_then(|contacts| contacts.avatar(&raw.address)).map(str::to_owned),
    }
}

pub fn to_attachment(raw: &RawAttachment, local_path: Option<PathBuf>) -> Attachment {
    Attachment {
        guid: raw.guid.clone(),
        name: raw.transfer_name.clone().unwrap_or_default(),
        mime: raw.mime_type.clone().unwrap_or_else(|| UNKNOWN_MIME.to_owned()),
        bytes: raw.total_bytes,
        width: raw.width,
        height: raw.height,
        measured: false,
        is_sticker: raw.is_sticker.unwrap_or(false),
        local_path,
        hidden: raw.hide_attachment.unwrap_or(false),
        duration_ms: raw.metadata.as_ref().and_then(|metadata| metadata.duration).map(|seconds| seconds * 1000.0),
    }
}

pub fn to_message(raw: &RawMessage, chat_guid: Option<&str>, options: &MapOptions<'_>) -> Message {
    let resolved_chat_guid = raw
        .chats
        .as_ref()
        .and_then(|chats| chats.first())
        .map(|chat| chat.guid.clone())
        .or_else(|| chat_guid.map(str::to_owned))
        .unwrap_or_default();
    let sender = raw.handle.as_ref().map(|handle| to_handle(handle, options.contacts));
    // A message I sent has no handle, so its service comes from the chat, then the guid prefix.
    let service = sender
        .as_ref()
        .map(|sender| sender.service)
        .or(options.chat_service)
        .or_else(|| prefix_service(&resolved_chat_guid))
        .unwrap_or(Service::IMessage);
    let (reaction, sticker_for) = parse_associated_message(raw);
    let subject = raw.subject.as_deref().map(clean_text).filter(|subject| !subject.is_empty());
    let attachments = raw.attachments.as_deref().unwrap_or_default();
    let url_preview = (raw.balloon_bundle_id.as_deref() == Some("com.apple.messages.URLBalloonProvider"))
        .then(|| raw.payload_data.as_ref().and_then(|payload| to_url_preview(payload, attachments)))
        .flatten();

    Message {
        guid: raw.guid.clone(),
        temp_guid: raw.temp_guid.clone(),
        chat_guid: resolved_chat_guid,
        text: clean_text(raw.text.as_deref().unwrap_or("")),
        subject,
        from_me: raw.is_from_me,
        date: raw.date_created,
        date_delivered: raw.date_delivered,
        date_read: raw.date_read,
        date_edited: raw.date_edited,
        date_retracted: raw.date_retracted,
        delivered_quietly: raw.was_delivered_quietly.filter(|quiet| *quiet),
        notified: raw.did_notify_recipient.filter(|notified| *notified),
        service,
        attachments: attachments
            .iter()
            .map(|attachment| to_attachment(attachment, options.attachment_paths.and_then(|paths| paths.get(&attachment.guid)).cloned()))
            .collect(),
        tapbacks: Vec::new(),
        parts: raw.attributed_body.as_ref().and_then(|body| to_parts(body, attachments)),
        reply_to: raw.thread_originator_guid.as_deref().filter(|guid| !guid.is_empty()).map(|guid| strip_guid_prefix(guid).to_owned()),
        sticker_for,
        effect: raw.expressive_send_style_id.clone(),
        error: (raw.error != 0).then(|| format!("Not delivered (error {})", raw.error)),
        is_audio: raw.is_audio_message.unwrap_or(false),
        group_event: if raw.item_type != 0 { parse_group_event(raw, sender.as_ref()) } else { None },
        sender,
        reaction,
        balloon_bundle_id: raw.balloon_bundle_id.clone(),
        url_preview,
    }
}

pub fn to_chat(raw: &RawChat, options: &MapOptions<'_>) -> Chat {
    // On macOS 26 the chat row no longer says which service a conversation
    // uses; the latest message's handle does (for sent DMs, the recipient's).
    let service = match raw.last_message.as_ref().and_then(|message| message.handle.as_ref()).filter(|handle| !handle.service.is_empty()) {
        Some(handle) => to_service(&handle.service),
        None => chat_service(&raw.guid, raw.participants.as_deref()),
    };
    let last_message = raw.last_message.as_ref().map(|message| {
        let options = MapOptions {
            contacts: options.contacts,
            attachment_paths: options.attachment_paths,
            chat_service: Some(service),
            chat_icon: None,
        };
        to_message(message, Some(&raw.guid), &options)
    });
    Chat {
        guid: raw.guid.clone(),
        identifier: raw.chat_identifier.clone(),
        group_id: raw.group_id.clone().filter(|id| !id.is_empty()),
        service,
        is_group: raw.style == 43,
        display_name: raw.display_name.clone().filter(|name| !name.is_empty()),
        icon: options.chat_icon.clone(),
        participants: raw.participants.as_deref().unwrap_or_default().iter().map(|handle| to_handle(handle, options.contacts)).collect(),
        pinned: false,
        muted: false,
        read_receipts: None,
        archived: raw.is_archived,
        // chat.db has no reliable per-chat unread column; an unread incoming last message stands in.
        unread: last_message.as_ref().is_some_and(|message| !message.from_me && message.date_read.is_none()),
        last_activity: last_message.as_ref().map(|message| message.date).unwrap_or(0),
        last_message,
    }
}

pub fn to_server_info(raw: &RawServerInfo) -> ServerInfo {
    ServerInfo {
        version: raw.server_version.clone(),
        macos_version: Some(raw.os_version.clone()),
        private_api: raw.private_api,
        helper_connected: raw.helper_connected,
        icloud_account: raw.detected_icloud.clone().filter(|account| !account.is_empty()),
    }
}

/// Without `avatar_path` the base64 becomes a `data:image/jpeg;base64,` URL.
pub fn to_contact(raw: &RawContact, avatar_path: Option<String>) -> Contact {
    let addresses: Vec<String> = raw
        .phone_numbers
        .iter()
        .chain(&raw.emails)
        .filter_map(|entry| entry.address.clone())
        .filter(|address| !address.is_empty())
        .collect();
    let present = |value: &Option<String>| value.clone().filter(|value| !value.is_empty());
    let name = present(&raw.display_name).or_else(|| match (present(&raw.first_name), present(&raw.last_name)) {
        (Some(first), Some(last)) => Some(format!("{first} {last}")),
        (Some(first), None) => Some(first),
        _ => present(&raw.nickname),
    });
    Contact {
        id: raw.id_string(),
        name: name.or_else(|| addresses.first().cloned()).unwrap_or_default(),
        // The server gives no mime hint; Apple Contacts thumbnails are JPEG in practice.
        avatar: avatar_path.or_else(|| present(&raw.avatar).map(|avatar| format!("data:image/jpeg;base64,{avatar}"))),
        addresses,
    }
}

fn normalize_digits(address: &str) -> String {
    address.chars().filter(char::is_ascii_digit).collect()
}

#[derive(Clone, Debug)]
struct ContactEntry {
    name: String,
    avatar: Option<String>,
}

/// Name and avatar by address: exact digits, then the last nine digits, emails case-insensitive.
#[derive(Clone, Debug, Default)]
pub struct ContactIndex {
    entries: Vec<ContactEntry>,
    by_digits: HashMap<String, usize>,
    by_last9: HashMap<String, usize>,
    by_email: HashMap<String, usize>,
}

fn last9(digits: &str) -> &str {
    &digits[digits.len() - 9..]
}

impl ContactIndex {
    pub fn new(contacts: &[Contact]) -> Self {
        let mut index = ContactIndex::default();
        for contact in contacts {
            let entry = index.entries.len();
            index.entries.push(ContactEntry { name: contact.name.clone(), avatar: contact.avatar.clone() });
            for address in &contact.addresses {
                if address.contains('@') {
                    index.by_email.insert(address.to_lowercase(), entry);
                    continue;
                }
                let digits = normalize_digits(address);
                if digits.is_empty() {
                    continue;
                }
                // Contacts often store a number without its country code while
                // chat.db has it; the last nine digits survive that and a trunk zero.
                if digits.len() >= 9 {
                    index.by_last9.insert(last9(&digits).to_owned(), entry);
                }
                index.by_digits.insert(digits, entry);
            }
        }
        index
    }

    fn lookup(&self, address: &str) -> Option<&ContactEntry> {
        let entry = if address.contains('@') {
            self.by_email.get(&address.to_lowercase())
        } else {
            let digits = normalize_digits(address);
            if digits.is_empty() {
                return None;
            }
            self.by_digits.get(&digits).or_else(|| if digits.len() >= 9 { self.by_last9.get(last9(&digits)) } else { None })
        };
        entry.map(|&index| &self.entries[index])
    }

    pub fn resolve(&self, address: &str) -> Option<&str> {
        self.lookup(address).map(|entry| entry.name.as_str())
    }

    pub fn avatar(&self, address: &str) -> Option<&str> {
        self.lookup(address).and_then(|entry| entry.avatar.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DM_GUID: &str = "iMessage;-;+15555550100";
    const GROUP_GUID: &str = "iMessage;+;chat123456789";

    fn raw_handle(overrides: Value) -> Value {
        merge(json!({ "originalROWID": 1, "address": "+15555550100", "service": "iMessage" }), overrides)
    }

    fn merge(mut base: Value, overrides: Value) -> Value {
        if let (Some(base), Value::Object(overrides)) = (base.as_object_mut(), overrides) {
            base.extend(overrides);
        }
        base
    }

    fn raw_message(overrides: Value) -> RawMessage {
        let base = json!({
            "originalROWID": 100,
            "guid": "message-guid-1",
            "text": "hello there",
            "handle": raw_handle(json!({})),
            "handleId": 1,
            "subject": "",
            "error": 0,
            "dateCreated": 1_700_000_000_000i64,
            "dateRead": null,
            "dateDelivered": null,
            "isFromMe": false,
            "isArchived": false,
            "itemType": 0,
            "groupTitle": null,
            "groupActionType": 0,
            "balloonBundleId": null,
            "associatedMessageGuid": null,
            "associatedMessageType": null,
            "expressiveSendStyleId": null,
        });
        serde_json::from_value(merge(base, overrides)).unwrap()
    }

    fn raw_chat(value: Value) -> RawChat {
        serde_json::from_value(value).unwrap()
    }

    fn raw_attachments(value: Value) -> Vec<RawAttachment> {
        serde_json::from_value(value).unwrap()
    }

    fn contact(id: &str, name: &str, addresses: &[&str], avatar: Option<&str>) -> Contact {
        Contact {
            id: id.to_owned(),
            name: name.to_owned(),
            addresses: addresses.iter().map(|address| address.to_string()).collect(),
            avatar: avatar.map(str::to_owned),
        }
    }

    fn message(raw: &RawMessage, chat_guid: Option<&str>) -> Message {
        to_message(raw, chat_guid, &MapOptions::default())
    }

    // toMessage

    #[test]
    fn maps_a_plain_text_message() {
        let raw = raw_message(json!({
            "chats": [{ "originalROWID": 1, "guid": DM_GUID, "style": 45, "chatIdentifier": "+15555550100", "isArchived": false, "displayName": "" }],
        }));
        let contacts = ContactIndex::new(&[]);
        let message = to_message(&raw, None, &MapOptions { contacts: Some(&contacts), ..Default::default() });
        assert_eq!(message.guid, "message-guid-1");
        assert_eq!(message.chat_guid, DM_GUID);
        assert_eq!(message.text, "hello there");
        assert!(!message.from_me);
        assert_eq!(message.service, Service::IMessage);
        assert_eq!(message.sender.as_ref().unwrap().address, "+15555550100");
        assert_eq!(message.reaction, None);
        assert_eq!(message.group_event, None);
        assert!(message.tapbacks.is_empty());
    }

    #[test]
    fn falls_back_to_the_caller_provided_chat_guid_when_chats_is_absent() {
        assert_eq!(message(&raw_message(json!({})), Some(DM_GUID)).chat_guid, DM_GUID);
    }

    #[test]
    fn maps_a_tapback_add() {
        let raw = raw_message(json!({ "guid": "reaction-guid-1", "text": "", "associatedMessageGuid": "p:0/message-guid-1", "associatedMessageType": "love" }));
        assert_eq!(
            message(&raw, Some(DM_GUID)).reaction,
            Some(Reaction { target_guid: "message-guid-1".into(), kind: TapbackKind::Love, emoji: None, removed: false })
        );
    }

    #[test]
    fn maps_a_tapback_removal_with_the_bp_guid_prefix() {
        let raw = raw_message(json!({ "guid": "reaction-guid-2", "text": "", "associatedMessageGuid": "bp:message-guid-1", "associatedMessageType": "-love" }));
        assert_eq!(
            message(&raw, Some(DM_GUID)).reaction,
            Some(Reaction { target_guid: "message-guid-1".into(), kind: TapbackKind::Love, emoji: None, removed: true })
        );
    }

    #[test]
    fn maps_a_reply_via_thread_originator_guid_stripping_the_part_prefix() {
        let raw = raw_message(json!({ "guid": "reply-guid-1", "text": "sounds good", "threadOriginatorGuid": "p:0/original-guid-1" }));
        assert_eq!(message(&raw, Some(DM_GUID)).reply_to.as_deref(), Some("original-guid-1"));
    }

    #[test]
    fn maps_a_group_rename_event() {
        let raw = raw_message(json!({
            "guid": "rename-guid-1",
            "text": "",
            "handle": raw_handle(json!({ "address": "+15555550101" })),
            "itemType": 2,
            "groupTitle": "Weekend Trip",
            "chats": [{ "originalROWID": 2, "guid": GROUP_GUID, "style": 43, "chatIdentifier": "chat123456789", "isArchived": false, "displayName": "Weekend Trip" }],
        }));
        let message = message(&raw, None);
        assert_eq!(message.group_event, Some(GroupEvent::Rename { title: "Weekend Trip".into() }));
        assert_eq!(message.chat_guid, GROUP_GUID);
    }

    #[test]
    fn maps_a_participant_left_event_to_a_leave_group_event() {
        let raw = raw_message(json!({
            "guid": "leave-guid-1",
            "text": "",
            "handle": raw_handle(json!({ "address": "+15555550102" })),
            "itemType": 1,
            "groupActionType": 1,
        }));
        match message(&raw, Some(GROUP_GUID)).group_event {
            Some(GroupEvent::Leave { who }) => assert_eq!(who.unwrap().address, "+15555550102"),
            other => panic!("expected a leave, got {other:?}"),
        }
    }

    #[test]
    fn gives_an_attachment_the_server_has_no_type_for_one_anyway() {
        let raw = raw_message(json!({
            "guid": "rcs-brand-logo",
            "text": "Your bill is ready",
            "attachments": [{ "originalROWID": 9, "guid": "brand-logo-guid", "uti": "public.data", "mimeType": null, "totalBytes": 40_150, "transferName": "BrandLogoImage" }],
        }));
        assert_eq!(message(&raw, Some(DM_GUID)).attachments[0].mime, "application/octet-stream");
    }

    #[test]
    fn maps_an_attachment_message() {
        let raw = raw_message(json!({
            "guid": "attachment-guid-1",
            "text": "",
            "attachments": [{ "originalROWID": 5, "guid": "att-guid-1", "uti": "public.jpeg", "mimeType": "image/jpeg", "totalBytes": 204_800, "transferName": "IMG_0001.jpeg", "width": 100, "height": 100 }],
        }));
        let paths = HashMap::from([("att-guid-1".to_owned(), PathBuf::from("/attachments/att-guid-1.jpeg"))]);
        let message = to_message(&raw, Some(DM_GUID), &MapOptions { attachment_paths: Some(&paths), ..Default::default() });
        assert_eq!(message.attachments.len(), 1);
        let attachment = &message.attachments[0];
        assert_eq!(attachment.guid, "att-guid-1");
        assert_eq!(attachment.name, "IMG_0001.jpeg");
        assert_eq!(attachment.mime, "image/jpeg");
        assert_eq!(attachment.bytes, 204_800);
        assert!(!attachment.is_sticker);
        assert_eq!(attachment.local_path.as_deref(), Some(std::path::Path::new("/attachments/att-guid-1.jpeg")));
    }

    #[test]
    fn resolves_the_sender_name_from_the_contact_index() {
        let contacts = ContactIndex::new(&[contact("1", "Alex Rivera", &["+1 (555) 555-0100"], None)]);
        let raw = raw_message(json!({ "handle": raw_handle(json!({ "address": "+15555550100" })) }));
        let message = to_message(&raw, Some(DM_GUID), &MapOptions { contacts: Some(&contacts), ..Default::default() });
        assert_eq!(message.sender.unwrap().name.as_deref(), Some("Alex Rivera"));
    }

    #[test]
    fn reports_a_delivery_error_as_a_human_string() {
        assert_eq!(message(&raw_message(json!({ "error": 22 })), Some(DM_GUID)).error.as_deref(), Some("Not delivered (error 22)"));
    }

    // toChat

    #[test]
    fn maps_a_dm_chat() {
        let chat = to_chat(
            &raw_chat(json!({ "originalROWID": 1, "guid": DM_GUID, "style": 45, "chatIdentifier": "+15555550100", "isArchived": false, "displayName": "", "participants": [raw_handle(json!({}))] })),
            &MapOptions::default(),
        );
        assert!(!chat.is_group);
        assert_eq!(chat.service, Service::IMessage);
        assert_eq!(chat.participants.len(), 1);
        assert_eq!(chat.display_name, None);
    }

    #[test]
    fn maps_a_group_chat() {
        let chat = to_chat(
            &raw_chat(json!({
                "originalROWID": 2, "guid": GROUP_GUID, "style": 43, "chatIdentifier": "chat123456789", "isArchived": false, "displayName": "Weekend Trip",
                "participants": [raw_handle(json!({ "address": "+15555550100" })), raw_handle(json!({ "address": "+15555550101" }))],
            })),
            &MapOptions::default(),
        );
        assert!(chat.is_group);
        assert_eq!(chat.display_name.as_deref(), Some("Weekend Trip"));
        assert_eq!(chat.participants.len(), 2);
    }

    fn chat_with_last(message: Value) -> RawChat {
        raw_chat(json!({ "originalROWID": 3, "guid": DM_GUID, "style": 45, "chatIdentifier": "+15555550100", "isArchived": false, "displayName": "", "lastMessage": message }))
    }

    fn raw_message_value(overrides: Value) -> Value {
        let raw = raw_message(overrides);
        json!({ "guid": raw.guid, "text": raw.text, "handle": raw_handle(json!({})), "dateCreated": raw.date_created, "dateRead": raw.date_read, "isFromMe": raw.is_from_me })
    }

    #[test]
    fn marks_a_chat_unread_when_the_last_message_is_incoming_and_unread() {
        let chat = to_chat(&chat_with_last(raw_message_value(json!({ "dateRead": null, "isFromMe": false }))), &MapOptions::default());
        assert!(chat.unread);
        assert_eq!(Some(chat.last_activity), chat.last_message.map(|message| message.date));
    }

    #[test]
    fn marks_a_chat_read_when_the_last_message_is_our_own() {
        let chat = to_chat(&chat_with_last(raw_message_value(json!({ "dateRead": null, "isFromMe": true }))), &MapOptions::default());
        assert!(!chat.unread);
    }

    // toServerInfo

    #[test]
    fn maps_the_server_metadata_response() {
        let raw: RawServerInfo = serde_json::from_value(json!({
            "os_version": "14.5", "server_version": "1.9.4", "private_api": true, "helper_connected": true, "detected_icloud": "user@icloud.com",
        }))
        .unwrap();
        assert_eq!(
            to_server_info(&raw),
            ServerInfo {
                version: "1.9.4".into(),
                macos_version: Some("14.5".into()),
                private_api: true,
                helper_connected: true,
                icloud_account: Some("user@icloud.com".into()),
            }
        );
    }

    #[test]
    fn omits_an_empty_icloud_account() {
        let raw: RawServerInfo = serde_json::from_value(json!({
            "os_version": "14.5", "server_version": "1.9.4", "private_api": false, "helper_connected": false, "detected_icloud": "",
        }))
        .unwrap();
        assert_eq!(to_server_info(&raw).icloud_account, None);
    }

    // toHandle

    fn handle(value: Value) -> RawHandle {
        serde_json::from_value(raw_handle(value)).unwrap()
    }

    #[test]
    fn maps_service_and_resolves_a_contact_name() {
        let contacts = ContactIndex::new(&[contact("1", "Sam Lee", &["sam@example.com"], None)]);
        let handle = to_handle(&handle(json!({ "address": "SAM@Example.com", "service": "iMessage" })), Some(&contacts));
        assert_eq!(handle.service, Service::IMessage);
        assert_eq!(handle.name.as_deref(), Some("Sam Lee"));
    }

    #[test]
    fn maps_sms_and_rcs_services() {
        assert_eq!(to_handle(&handle(json!({ "service": "SMS" })), None).service, Service::Sms);
        assert_eq!(to_handle(&handle(json!({ "service": "RCS" })), None).service, Service::Rcs);
    }

    // toContact

    fn raw_contact(value: Value) -> RawContact {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn maps_phone_numbers_and_emails_into_a_flat_address_list() {
        let contact = to_contact(
            &raw_contact(json!({
                "id": 42, "firstName": "Jordan", "lastName": "Blake",
                "phoneNumbers": [{ "address": "+15555550100", "id": "1" }], "emails": [{ "address": "jordan@example.com", "id": "2" }], "avatar": "",
            })),
            None,
        );
        assert_eq!(contact.id, "42");
        assert_eq!(contact.name, "Jordan Blake");
        assert_eq!(contact.addresses, vec!["+15555550100", "jordan@example.com"]);
        assert_eq!(contact.avatar, None);
    }

    #[test]
    fn builds_a_data_url_from_a_base64_avatar() {
        let raw = raw_contact(json!({ "id": "1", "displayName": "Riley", "phoneNumbers": [], "emails": [], "avatar": "aGVsbG8=" }));
        assert_eq!(to_contact(&raw, None).avatar.as_deref(), Some("data:image/jpeg;base64,aGVsbG8="));
    }

    #[test]
    fn prefers_a_given_avatar_path_over_the_base64_payload() {
        let raw = raw_contact(json!({ "id": "1", "displayName": "Riley", "phoneNumbers": [], "emails": [], "avatar": "aGVsbG8=" }));
        assert_eq!(to_contact(&raw, Some("/cache/avatars/contact-1.jpg".into())).avatar.as_deref(), Some("/cache/avatars/contact-1.jpg"));
    }

    // ContactIndex

    #[test]
    fn normalizes_phone_numbers_across_formatting_differences() {
        let contacts = ContactIndex::new(&[contact("1", "Casey", &["+1 (555) 555-0100"], None)]);
        assert_eq!(contacts.resolve("5555550100"), Some("Casey"));
        assert_eq!(contacts.resolve("15555550100"), Some("Casey"));
        assert_eq!(contacts.resolve("+15555550100"), Some("Casey"));
    }

    #[test]
    fn lowercases_emails() {
        let contacts = ContactIndex::new(&[contact("1", "Drew", &["Drew@Example.com"], None)]);
        assert_eq!(contacts.resolve("drew@example.com"), Some("Drew"));
    }

    #[test]
    fn returns_none_for_an_unknown_address() {
        let contacts = ContactIndex::new(&[contact("1", "Casey", &["+15555550100"], None)]);
        assert_eq!(contacts.resolve("+15555559999"), None);
    }

    #[test]
    fn resolves_an_avatar_with_the_same_address_normalisation_as_the_name() {
        let contacts = ContactIndex::new(&[
            contact("1", "Casey", &["+1 (555) 555-0100"], Some("/cache/avatars/contact-1.jpg")),
            contact("2", "Drew", &["drew@example.com"], None),
        ]);
        assert_eq!(contacts.avatar("5555550100"), Some("/cache/avatars/contact-1.jpg"));
        assert_eq!(contacts.avatar("drew@example.com"), None);
        assert_eq!(contacts.avatar("+15555559999"), None);
    }

    #[test]
    fn resolves_a_plus34_handle_against_a_contact_stored_with_nine_local_digits() {
        let index = ContactIndex::new(&[contact("1", "Marta", &["612 34 56 78"], None)]);
        assert_eq!(index.resolve("+34612345678"), Some("Marta"));
        assert_eq!(index.resolve("0034612345678"), Some("Marta"));
        assert_eq!(index.resolve("+34600000000"), None);
    }

    // chat and message service resolution

    #[test]
    fn derives_an_any_chat_service_from_participants_when_none_are_imessage() {
        let raw = raw_chat(json!({
            "originalROWID": 20, "guid": "any;-;+15555550199", "style": 45, "chatIdentifier": "+15555550199", "isArchived": false, "displayName": "",
            "participants": [raw_handle(json!({ "address": "+15555550199", "service": "SMS" }))],
        }));
        assert_eq!(to_chat(&raw, &MapOptions::default()).service, Service::Sms);
    }

    #[test]
    fn prefers_imessage_over_rcs_and_sms_participants_for_an_any_chat() {
        let raw = raw_chat(json!({
            "originalROWID": 21, "guid": "any;+;chat9999", "style": 43, "chatIdentifier": "chat9999", "isArchived": false, "displayName": "",
            "participants": [raw_handle(json!({ "address": "+1", "service": "RCS" })), raw_handle(json!({ "address": "+2", "service": "iMessage" }))],
        }));
        assert_eq!(to_chat(&raw, &MapOptions::default()).service, Service::IMessage);
    }

    #[test]
    fn falls_back_to_imessage_for_an_any_chat_with_no_participants() {
        let raw = raw_chat(json!({ "originalROWID": 22, "guid": "any;-;+1", "style": 45, "chatIdentifier": "+1", "isArchived": false, "displayName": "" }));
        assert_eq!(to_chat(&raw, &MapOptions::default()).service, Service::IMessage);
    }

    #[test]
    fn takes_an_any_chat_service_from_the_last_messages_handle() {
        let raw = raw_chat(json!({
            "guid": "any;-;+15555550199", "style": 45, "chatIdentifier": "+15555550199",
            "participants": [raw_handle(json!({ "service": "iMessage" }))],
            "lastMessage": { "guid": "m", "text": "hi", "isFromMe": true, "handle": raw_handle(json!({ "service": "RCS" })) },
        }));
        let chat = to_chat(&raw, &MapOptions::default());
        assert_eq!(chat.service, Service::Rcs);
        assert_eq!(chat.last_message.unwrap().service, Service::Rcs);
    }

    #[test]
    fn uses_the_chat_service_option_for_a_sent_message_with_no_handle() {
        let raw = raw_message(json!({ "handle": null, "isFromMe": true }));
        let message = to_message(&raw, Some("any;-;+15555550199"), &MapOptions { chat_service: Some(Service::Rcs), ..Default::default() });
        assert_eq!(message.service, Service::Rcs);
    }

    #[test]
    fn falls_back_to_the_guid_prefix_for_a_sent_message_with_no_chat_service_option() {
        let raw = raw_message(json!({ "handle": null, "isFromMe": true }));
        assert_eq!(message(&raw, Some("SMS;-;+15555550199")).service, Service::Sms);
    }

    // downloadPlan

    fn plan(original: bool, extension: &str, sticker: bool) -> DownloadPlan {
        DownloadPlan { original, extension: extension.to_owned(), sticker }
    }

    #[test]
    fn requests_a_converted_jpeg_for_a_heic_attachment() {
        assert_eq!(download_plan(Some("IMG_0001.HEIC"), Some("image/heic"), false), plan(false, ".jpg", false));
    }

    #[test]
    fn requests_a_converted_m4a_for_a_caf_audio_attachment() {
        assert_eq!(download_plan(Some("Audio Message.caf"), Some("audio/x-caf"), false), plan(false, ".m4a", false));
    }

    #[test]
    fn keeps_a_plain_png_as_is() {
        assert_eq!(download_plan(Some("photo.png"), Some("image/png"), false), plan(false, ".png", false));
    }

    #[test]
    fn requests_the_original_for_a_video_attachment() {
        assert_eq!(download_plan(Some("clip.mp4"), Some("video/mp4"), false), plan(true, ".mp4", false));
    }

    #[test]
    fn fetches_the_original_of_a_sticker_and_caches_it_as_png() {
        assert_eq!(download_plan(Some("08D7E2F1.heic.jpeg"), Some("image/jpeg"), true), plan(true, ".png", true));
    }

    #[test]
    fn handles_an_attachment_with_no_name_or_mime() {
        assert_eq!(download_plan(None, None, false), plan(true, "", false));
    }

    #[test]
    fn extname_matches_node() {
        assert_eq!(extname("a.tar.gz"), ".gz");
        assert_eq!(extname(".bashrc"), "");
        assert_eq!(extname("noext"), "");
        assert_eq!(extname("dir.d/file"), "");
    }

    // decodeKeyedArchive and toUrlPreview

    /// Built from the real payloadData sample, with an added summary and image substitute index.
    fn rich_link_archive() -> Value {
        json!({
            "$version": 100000,
            "$archiver": "NSKeyedArchiver",
            "$top": { "root": { "UID": 1 } },
            "$objects": [
                "$null",
                { "richLinkIsPlaceholder": false, "richLinkMetadata": { "UID": 2 }, "$class": { "UID": 12 } },
                { "$class": { "UID": 13 }, "originalURL": { "UID": 3 }, "title": { "UID": 7 }, "summary": { "UID": 8 }, "siteName": { "UID": 9 }, "image": { "UID": 10 } },
                { "NS.base": { "UID": 0 }, "$class": { "UID": 5 }, "NS.relative": { "UID": 4 } },
                "https://example.com/article",
                { "$classname": "NSURL", "$classes": ["NSURL", "NSObject"] },
                null,
                "A great article",
                "The article summary.",
                "Example Site",
                { "richLinkImageAttachmentSubstituteIndex": 0, "$class": { "UID": 11 }, "MIMEType": "image/png" },
                { "$classname": "RLImageMetadata", "$classes": ["RLImageMetadata", "NSObject"] },
                { "$classname": "NSDictionary" },
                { "$classname": "NSRichLinkMetadata" },
            ],
        })
    }

    #[test]
    fn resolves_uid_references_drops_class_and_collapses_nsurl_wrappers() {
        assert_eq!(
            decode_keyed_archive(&rich_link_archive()),
            json!({
                "richLinkIsPlaceholder": false,
                "richLinkMetadata": {
                    "originalURL": "https://example.com/article",
                    "title": "A great article",
                    "summary": "The article summary.",
                    "siteName": "Example Site",
                    "image": { "richLinkImageAttachmentSubstituteIndex": 0, "MIMEType": "image/png" },
                },
            })
        );
    }

    #[test]
    fn decode_keyed_archive_survives_a_cycle() {
        let archive = json!({ "$top": { "root": { "UID": 1 } }, "$objects": ["$null", { "self": { "UID": 1 }, "name": "x" }] });
        assert_eq!(decode_keyed_archive(&archive), json!({ "name": "x" }));
    }

    #[test]
    fn extracts_url_title_summary_and_the_substitute_image_guid() {
        let attachments = raw_attachments(json!([{ "originalROWID": 1, "guid": "preview-image-guid", "uti": "dyn.age8u", "mimeType": "", "totalBytes": 0, "transferName": "", "hideAttachment": true }]));
        let preview = to_url_preview(&json!([rich_link_archive()]), &attachments).unwrap();
        assert_eq!(preview.url, "https://example.com/article");
        assert_eq!(preview.title.as_deref(), Some("A great article"));
        assert_eq!(preview.summary.as_deref(), Some("The article summary."));
        assert_eq!(preview.site_name.as_deref(), Some("Example Site"));
        assert_eq!(preview.image_attachment_guid.as_deref(), Some("preview-image-guid"));
    }

    #[test]
    fn sets_url_preview_on_a_message_with_the_url_balloon_bundle_id() {
        let raw = raw_message(json!({ "balloonBundleId": "com.apple.messages.URLBalloonProvider", "payloadData": [rich_link_archive()] }));
        let message = message(&raw, Some(DM_GUID));
        assert_eq!(message.url_preview.unwrap().url, "https://example.com/article");
        assert_eq!(message.text, "hello there");
    }

    // stickers

    #[test]
    fn maps_a_sticker_as_sticker_for_not_a_reaction_keeping_its_attachment() {
        let raw = raw_message(json!({
            "guid": "sticker-guid-1", "text": "", "associatedMessageGuid": "p:0/message-guid-1", "associatedMessageType": "sticker",
            "attachments": [{ "originalROWID": 9, "guid": "sticker-att-1", "uti": "com.apple.sticker", "mimeType": "image/png", "totalBytes": 1024, "transferName": "sticker.png", "isSticker": true }],
        }));
        let message = message(&raw, Some(DM_GUID));
        assert_eq!(message.reaction, None);
        assert_eq!(message.sticker_for.as_deref(), Some("message-guid-1"));
        assert_eq!(message.attachments.len(), 1);
    }

    #[test]
    fn maps_the_numeric_1000_form_the_same_way() {
        let raw = raw_message(json!({ "associatedMessageGuid": "bp:message-guid-1", "associatedMessageType": "1000" }));
        assert_eq!(message(&raw, Some(DM_GUID)).sticker_for.as_deref(), Some("message-guid-1"));
    }

    #[test]
    fn reads_a_numeric_associated_message_type() {
        let raw = raw_message(json!({ "associatedMessageGuid": "bp:message-guid-1", "associatedMessageType": 2001 }));
        assert_eq!(message(&raw, Some(DM_GUID)).reaction.unwrap().kind, TapbackKind::Like);
    }

    // custom emoji tapbacks

    #[test]
    fn maps_a_known_numeric_add_type_to_its_named_kind() {
        let raw = raw_message(json!({ "associatedMessageGuid": "p:0/message-guid-1", "associatedMessageType": "2003" }));
        assert_eq!(
            message(&raw, Some(DM_GUID)).reaction,
            Some(Reaction { target_guid: "message-guid-1".into(), kind: TapbackKind::Laugh, emoji: None, removed: false })
        );
    }

    #[test]
    fn extracts_the_emoji_from_the_message_text_on_an_unrecognized_add_type() {
        let raw = raw_message(json!({ "text": "Reacted 🔥 to “hello there”", "associatedMessageGuid": "p:0/message-guid-1", "associatedMessageType": "2006" }));
        assert_eq!(
            message(&raw, Some(DM_GUID)).reaction,
            Some(Reaction { target_guid: "message-guid-1".into(), kind: TapbackKind::Emoji, emoji: Some("🔥".into()), removed: false })
        );
    }

    #[test]
    fn extracts_the_emoji_from_the_message_text_on_an_unrecognized_remove_type() {
        let raw = raw_message(json!({ "text": "Removed a 🔥 reaction from “hello there”", "associatedMessageGuid": "p:0/message-guid-1", "associatedMessageType": "3006" }));
        assert_eq!(
            message(&raw, Some(DM_GUID)).reaction,
            Some(Reaction { target_guid: "message-guid-1".into(), kind: TapbackKind::Emoji, emoji: Some("🔥".into()), removed: true })
        );
    }

    #[test]
    fn extracts_a_zwj_emoji_cluster_whole() {
        let raw = raw_message(json!({ "text": "Reacted 👩🏽‍💻 to “x”", "associatedMessageGuid": "p:0/m", "associatedMessageType": "2006" }));
        assert_eq!(message(&raw, Some(DM_GUID)).reaction.unwrap().emoji.as_deref(), Some("👩🏽‍💻"));
    }

    #[test]
    fn falls_back_to_a_heart_when_no_emoji_is_found_in_the_text() {
        let raw = raw_message(json!({ "text": "Reacted to “hello there”", "associatedMessageGuid": "p:0/message-guid-1", "associatedMessageType": "2006" }));
        assert_eq!(message(&raw, Some(DM_GUID)).reaction.unwrap().emoji.as_deref(), Some("❤️"));
    }

    // attachment metadata

    #[test]
    fn maps_duration_ms_from_seconds_and_the_hidden_flag() {
        let raw = raw_message(json!({
            "attachments": [{
                "originalROWID": 6, "guid": "audio-att-1", "uti": "com.apple.coreaudio-format", "mimeType": "audio/x-caf", "totalBytes": 40_000,
                "transferName": "Audio Message.caf", "hideAttachment": true, "metadata": { "duration": 12.5, "bitRate": 128_000, "sampleRate": 44_100, "bytes": 40_000 },
            }],
        }));
        let attachment = &message(&raw, Some(DM_GUID)).attachments[0];
        assert_eq!(attachment.duration_ms, Some(12_500.0));
        assert!(attachment.hidden);
    }

    #[test]
    fn defaults_hidden_to_false_and_leaves_duration_ms_unset() {
        let raw = raw_message(json!({
            "attachments": [{ "originalROWID": 7, "guid": "img-att-1", "uti": "public.jpeg", "mimeType": "image/jpeg", "totalBytes": 500, "transferName": "a.jpg" }],
        }));
        let attachment = &message(&raw, Some(DM_GUID)).attachments[0];
        assert!(!attachment.hidden);
        assert_eq!(attachment.duration_ms, None);
    }

    // malformed text handling

    #[test]
    fn replaces_a_lone_surrogate_so_the_text_round_trips_through_json() {
        let body = br#"{"guid":"m","text":"hello \uD800 world","handle":null}"#;
        assert!(serde_json::from_slice::<RawMessage>(body).is_err());
        let raw: RawMessage = parse_json(body).unwrap();
        let message = message(&raw, Some(DM_GUID));
        assert_eq!(message.text, "hello \u{FFFD} world");
        assert!(serde_json::to_string(&message).is_ok());
    }

    #[test]
    fn repair_keeps_pairs_escaped_backslashes_and_clean_bodies() {
        let clean = br#"{"a":"\ud83d\udd25 fine \\uD800 not an escape"}"#;
        assert!(matches!(repair_lone_surrogates(clean), Cow::Borrowed(_)));
        let lone = br#"["\udc00x","\uD83D","\uD83D\u0041"]"#;
        let repaired = repair_lone_surrogates(lone);
        assert_eq!(std::str::from_utf8(&repaired).unwrap(), r#"["\uFFFDx","\uFFFD","\uFFFD\u0041"]"#);
        assert_eq!(serde_json::from_slice::<Vec<String>>(&repaired).unwrap(), vec!["\u{FFFD}x", "\u{FFFD}", "\u{FFFD}A"]);
    }

    #[test]
    fn strips_the_attachment_placeholder_character_from_text() {
        let raw = raw_message(json!({
            "text": "\u{FFFC}",
            "attachments": [{ "originalROWID": 8, "guid": "img-att-2", "uti": "public.jpeg", "mimeType": "image/jpeg", "totalBytes": 500, "transferName": "b.jpg" }],
        }));
        assert_eq!(message(&raw, Some(DM_GUID)).text, "");
    }

    #[test]
    fn one_bad_field_does_not_fail_the_message() {
        let raw: RawMessage = serde_json::from_value(json!({
            "guid": "m", "text": null, "error": null, "dateCreated": 1.7e12, "isFromMe": 1, "attachments": [{ "guid": "a", "totalBytes": null, "width": 12.0, "metadata": "odd" }],
        }))
        .unwrap();
        let message = message(&raw, Some(DM_GUID));
        assert!(message.from_me);
        assert_eq!(message.date, 1_700_000_000_000);
        assert_eq!(message.attachments[0].width, Some(12));
    }

    // toParts

    const PART: &str = "__kIMMessagePartAttributeName";

    fn body(string: &str, runs: Value) -> Value {
        json!([{ "string": string, "runs": runs }])
    }

    fn run(text: &str) -> RichRun {
        RichRun { text: text.to_owned(), ..Default::default() }
    }

    fn text_part(runs: Vec<RichRun>) -> MessagePart {
        MessagePart::Text { runs }
    }

    #[test]
    fn returns_none_for_a_body_with_one_unstyled_run() {
        assert_eq!(to_parts(&body("hello there", json!([{ "range": [0, 11], "attributes": { PART: 0 } }])), &[]), None);
    }

    #[test]
    fn returns_none_when_the_body_is_missing_or_malformed() {
        assert_eq!(to_parts(&Value::Null, &[]), None);
        assert_eq!(to_parts(&json!([{ "string": "hello" }]), &[]), None);
        assert_eq!(to_parts(&json!([{ "string": "hello", "runs": [] }]), &[]), None);
        assert_eq!(to_parts(&json!([{ "runs": [{ "range": [0, 5] }] }]), &[]), None);
        assert_eq!(to_parts(&body("hello", json!([{ "range": [0, 5] }, { "attributes": { PART: 0 } }])), &[]), None);
    }

    #[test]
    fn keeps_bold_and_italic_runs_and_merges_the_unstyled_ones_around_them() {
        let parts = to_parts(
            &body(
                "a bold and italic end",
                json!([
                    { "range": [0, 2], "attributes": { PART: 0 } },
                    { "range": [2, 4], "attributes": { PART: 0, "__kIMTextBoldAttributeName": 1 } },
                    { "range": [6, 5], "attributes": { PART: 0 } },
                    { "range": [11, 6], "attributes": { PART: 0, "__kIMTextItalicAttributeName": 1 } },
                    { "range": [17, 4], "attributes": { PART: 0 } },
                ]),
            ),
            &[],
        );
        assert_eq!(
            parts,
            Some(vec![text_part(vec![
                run("a "),
                RichRun { bold: true, ..run("bold") },
                run(" and "),
                RichRun { italic: true, ..run("italic") },
                run(" end"),
            ])])
        );
    }

    #[test]
    fn marks_underline_strikethrough_and_a_text_effect() {
        let parts = to_parts(
            &body(
                "under struck big",
                json!([
                    { "range": [0, 6], "attributes": { PART: 0, "__kIMTextUnderlineAttributeName": 1 } },
                    { "range": [6, 7], "attributes": { PART: 0, "__kIMTextStrikethroughAttributeName": 1 } },
                    { "range": [13, 3], "attributes": { PART: 0, "__kIMTextEffectAttributeName": 5 } },
                ]),
            ),
            &[],
        )
        .unwrap();
        let MessagePart::Text { runs } = &parts[0] else { panic!("expected text") };
        assert!(runs[0].underline);
        assert!(runs[1].strike);
        assert_eq!(runs[2], RichRun { effect: Some(TextEffect::Big), ..run("big") });
    }

    #[test]
    fn carries_a_mention_address_and_the_display_name_it_covers() {
        let parts = to_parts(
            &body(
                "hey Sam, ready?",
                json!([
                    { "range": [0, 4], "attributes": { PART: 0 } },
                    { "range": [4, 3], "attributes": { PART: 0, "__kIMMentionConfirmedMention": "+14155550103" } },
                    { "range": [7, 8], "attributes": { PART: 0 } },
                ]),
            ),
            &[],
        );
        assert_eq!(
            parts,
            Some(vec![text_part(vec![run("hey "), RichRun { mention: Some("+14155550103".into()), ..run("Sam") }, run(", ready?")])])
        );
    }

    #[test]
    fn takes_the_href_from_the_link_attribute_rather_than_the_run_text() {
        let parts = to_parts(
            &body(
                "read the docs",
                json!([
                    { "range": [0, 9], "attributes": { PART: 0 } },
                    { "range": [9, 4], "attributes": { PART: 0, "__kIMLinkAttributeName": "https://docs.bluebubbles.app" } },
                ]),
            ),
            &[],
        );
        assert_eq!(parts, Some(vec![text_part(vec![run("read the "), RichRun { link: Some("https://docs.bluebubbles.app".into()), ..run("docs") }])]));
    }

    fn raw_attachment(guid: &str) -> RawAttachment {
        serde_json::from_value(json!({ "originalROWID": 1, "guid": guid, "uti": "public.jpeg", "mimeType": "image/jpeg", "totalBytes": 1000, "transferName": format!("{guid}.jpg") })).unwrap()
    }

    #[test]
    fn places_an_attachment_between_the_two_lines_of_text_around_it() {
        let parts = to_parts(
            &body(
                "before\n\u{FFFC}\nafter",
                json!([
                    { "range": [0, 7], "attributes": { PART: 0 } },
                    { "range": [7, 1], "attributes": { PART: 1, "__kIMFileTransferGUIDAttributeName": "att-1" } },
                    { "range": [8, 6], "attributes": { PART: 2 } },
                ]),
            ),
            &[raw_attachment("att-1")],
        );
        assert_eq!(
            parts,
            Some(vec![text_part(vec![run("before")]), MessagePart::Attachment { guid: "att-1".into() }, text_part(vec![run("after")])])
        );
    }

    #[test]
    fn drops_a_placeholder_for_an_attachment_the_query_did_not_return() {
        let parts = to_parts(
            &body(
                "look\n\u{FFFC}",
                json!([
                    { "range": [0, 5], "attributes": { PART: 0, "__kIMTextBoldAttributeName": 1 } },
                    { "range": [5, 1], "attributes": { PART: 1, "__kIMFileTransferGUIDAttributeName": "missing" } },
                ]),
            ),
            &[],
        );
        assert_eq!(parts, Some(vec![text_part(vec![RichRun { bold: true, ..run("look") }])]));
    }

    #[test]
    fn splits_a_two_part_message_on_the_part_index() {
        let parts = to_parts(
            &body("first\nsecond", json!([{ "range": [0, 6], "attributes": { PART: 0 } }, { "range": [6, 6], "attributes": { PART: 1 } }])),
            &[],
        );
        assert_eq!(parts, Some(vec![text_part(vec![run("first")]), text_part(vec![run("second")])]));
    }

    #[test]
    fn ranges_count_utf16_code_units() {
        let parts = to_parts(
            &body("🔥 hot", json!([{ "range": [0, 2], "attributes": { PART: 0, "__kIMTextBoldAttributeName": 1 } }, { "range": [2, 4], "attributes": { PART: 0 } }])),
            &[],
        );
        assert_eq!(parts, Some(vec![text_part(vec![RichRun { bold: true, ..run("🔥") }, run(" hot")])]));
    }

    #[test]
    fn to_message_sets_parts_from_attributed_body_and_leaves_plain_messages_alone() {
        let styled = message(
            &raw_message(json!({
                "text": "hello there",
                "attributedBody": body("hello there", json!([
                    { "range": [0, 5], "attributes": { PART: 0, "__kIMTextBoldAttributeName": 1 } },
                    { "range": [5, 6], "attributes": { PART: 0 } },
                ])),
            })),
            Some(DM_GUID),
        );
        assert_eq!(styled.parts, Some(vec![text_part(vec![RichRun { bold: true, ..run("hello") }, run(" there")])]));
        assert_eq!(styled.text, "hello there");
        assert_eq!(message(&raw_message(json!({})), Some(DM_GUID)).parts, None);
    }
}
