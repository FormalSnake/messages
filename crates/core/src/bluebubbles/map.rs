//! Port of packages/core/src/bluebubbles/map.ts: raw server JSON to the model.
//! Stays synchronous; the transport precomputes paths, services and icons.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use crate::model::{Attachment, Chat, Contact, Handle, Message, MessagePart, ServerInfo, Service, UrlPreview};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawHandle {
    #[serde(default, rename = "originalROWID")]
    pub original_rowid: i64,
    pub address: String,
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub uncanonicalized_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RawAttachmentMetadata {
    #[serde(default)]
    pub duration: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawAttachment {
    #[serde(default, rename = "originalROWID")]
    pub original_rowid: i64,
    pub guid: String,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub uti: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub transfer_name: Option<String>,
    #[serde(default)]
    pub is_sticker: Option<bool>,
    #[serde(default)]
    pub hide_attachment: Option<bool>,
    #[serde(default)]
    pub has_live_photo: Option<bool>,
    #[serde(default)]
    pub metadata: Option<RawAttachmentMetadata>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawChat {
    #[serde(default, rename = "originalROWID")]
    pub original_rowid: i64,
    pub guid: String,
    #[serde(default)]
    pub participants: Option<Vec<RawHandle>>,
    #[serde(default)]
    pub last_message: Option<Box<RawMessage>>,
    /// 43 is a group.
    #[serde(default)]
    pub style: i64,
    #[serde(default)]
    pub chat_identifier: String,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub group_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawMessage {
    #[serde(default, rename = "originalROWID")]
    pub original_rowid: i64,
    #[serde(default)]
    pub temp_guid: Option<String>,
    pub guid: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub attributed_body: Option<Value>,
    #[serde(default)]
    pub handle: Option<RawHandle>,
    #[serde(default)]
    pub handle_id: i64,
    #[serde(default)]
    pub chats: Option<Vec<RawChat>>,
    #[serde(default)]
    pub attachments: Option<Vec<RawAttachment>>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub error: i64,
    #[serde(default)]
    pub date_created: i64,
    #[serde(default)]
    pub date_read: Option<i64>,
    #[serde(default)]
    pub date_delivered: Option<i64>,
    #[serde(default)]
    pub is_from_me: bool,
    #[serde(default)]
    pub is_audio_message: Option<bool>,
    #[serde(default)]
    pub item_type: i64,
    #[serde(default)]
    pub group_title: Option<String>,
    #[serde(default)]
    pub group_action_type: i64,
    #[serde(default)]
    pub balloon_bundle_id: Option<String>,
    #[serde(default)]
    pub associated_message_guid: Option<String>,
    /// A decoded name ("love", "-like", "sticker") or the raw chat.db integer as a string.
    #[serde(default, deserialize_with = "string_or_number")]
    pub associated_message_type: Option<String>,
    #[serde(default)]
    pub expressive_send_style_id: Option<String>,
    #[serde(default)]
    pub thread_originator_guid: Option<String>,
    #[serde(default)]
    pub thread_originator_part: Option<String>,
    #[serde(default)]
    pub date_retracted: Option<i64>,
    #[serde(default)]
    pub date_edited: Option<i64>,
    #[serde(default)]
    pub was_delivered_quietly: Option<bool>,
    #[serde(default)]
    pub did_notify_recipient: Option<bool>,
    #[serde(default)]
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
    pub os_version: String,
    pub server_version: String,
    pub private_api: bool,
    pub helper_connected: bool,
    #[serde(default)]
    pub detected_icloud: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RawContactAddress {
    pub address: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawContact {
    pub id: Value,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub phone_numbers: Vec<RawContactAddress>,
    #[serde(default)]
    pub emails: Vec<RawContactAddress>,
    /// Bare base64 JPEG, no data: prefix.
    #[serde(default)]
    pub avatar: Option<String>,
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

/// serde_json rejects a lone UTF-16 surrogate escape and the whole page with it,
/// and the server sends them on live data. Rewrites each lone `\uD8xx`-`\uDFxx`
/// escape in a response body to `�` before parsing. Borrowed when there is none.
pub fn repair_lone_surrogates(body: &[u8]) -> Cow<'_, [u8]> {
    let _ = body;
    unimplemented!()
}

/// Resolves an NSKeyedArchiver plist (already JSON) into plain values.
pub fn decode_keyed_archive(archive: &Value) -> Value {
    let _ = archive;
    unimplemented!()
}

pub fn to_url_preview(payload_data: &Value, attachments: &[RawAttachment]) -> Option<UrlPreview> {
    let _ = (payload_data, attachments);
    unimplemented!()
}

/// None for a body that says nothing `text` does not already say. Ranges are UTF-16 code units.
pub fn to_parts(attributed_body: &Value, attachments: &[RawAttachment]) -> Option<Vec<MessagePart>> {
    let _ = (attributed_body, attachments);
    unimplemented!()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadPlan {
    /// `original=true` on the download: everything but HEIC/HEIF/TIFF and CAF, and always for stickers.
    pub original: bool,
    /// Cache entry extension including the dot: `.jpg` for HEIC/TIFF, `.m4a` for CAF, `.png` for stickers.
    pub extension: String,
    pub sticker: bool,
}

pub fn download_plan(name: Option<&str>, mime: Option<&str>, sticker: bool) -> DownloadPlan {
    let _ = (name, mime, sticker);
    unimplemented!()
}

pub fn to_handle(raw: &RawHandle, contacts: Option<&ContactIndex>) -> Handle {
    let _ = (raw, contacts);
    unimplemented!()
}

pub fn to_attachment(raw: &RawAttachment, local_path: Option<PathBuf>) -> Attachment {
    let _ = (raw, local_path);
    unimplemented!()
}

pub fn to_message(raw: &RawMessage, chat_guid: Option<&str>, options: &MapOptions<'_>) -> Message {
    let _ = (raw, chat_guid, options);
    unimplemented!()
}

pub fn to_chat(raw: &RawChat, options: &MapOptions<'_>) -> Chat {
    let _ = (raw, options);
    unimplemented!()
}

pub fn to_server_info(raw: &RawServerInfo) -> ServerInfo {
    let _ = raw;
    unimplemented!()
}

/// Without `avatar_path` the base64 becomes a `data:image/jpeg;base64,` URL.
pub fn to_contact(raw: &RawContact, avatar_path: Option<String>) -> Contact {
    let _ = (raw, avatar_path);
    unimplemented!()
}

/// Name and avatar by address: exact digits, then the last nine digits, emails case-insensitive.
#[derive(Clone, Debug, Default)]
pub struct ContactIndex {
    _private: (),
}

impl ContactIndex {
    pub fn new(contacts: &[Contact]) -> Self {
        let _ = contacts;
        unimplemented!()
    }

    pub fn resolve(&self, address: &str) -> Option<&str> {
        let _ = address;
        unimplemented!()
    }

    pub fn avatar(&self, address: &str) -> Option<&str> {
        let _ = address;
        unimplemented!()
    }
}
