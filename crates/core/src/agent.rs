//! Port of packages/core/src/agent.ts: client for `@messages/mac-agent`, which
//! serves decrypted Find My locations and the prefs shared between clients.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::findmy::{DeviceLocation, FriendLocation};
use crate::gifs::GifFavorite;
use crate::model::Millis;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub url: String,
    pub token: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentKeys {
    pub friends: bool,
    pub fmf: bool,
    pub fmip: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHealth {
    pub ok: bool,
    pub keys: AgentKeys,
    #[serde(default)]
    pub prefs: Option<bool>,
    /// Whether FindMy.app is running on the Mac. Nothing refreshes its caches while it is not.
    #[serde(default)]
    pub find_my_open: Option<bool>,
}

/// One push from `/findmy/stream`. A half the agent could not read is None, so a client keeps what it had.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindMySnapshot {
    pub friends: Option<Vec<FriendLocation>>,
    pub devices: Option<Vec<DeviceLocation>>,
    pub updated_at: Millis,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendsResponse {
    pub friends: Vec<FriendLocation>,
    pub updated_at: Millis,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicesResponse {
    pub devices: Vec<DeviceLocation>,
    pub updated_at: Millis,
}

/// Per-chat client state, kept in config.json `chats` and on the Mac agent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPrefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_receipts: Option<bool>,
    /// The composer text for this chat, synced between clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<String>,
    /// When a client last changed this entry; the newer entry wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<Millis>,
    /// Moved only by a pin or unpin, so a draft or a mute cannot outrank the Mac's list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<Millis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac_pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac_pinned_at: Option<Millis>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedPrefs {
    pub chats: HashMap<String, ChatPrefs>,
    /// Absent from agents that predate GIF favorites.
    #[serde(default)]
    pub gifs: Option<HashMap<String, GifFavorite>>,
    /// Pinned in Messages.app: an address for a one-to-one chat, a group id or chat guid for a group.
    pub mac_pinned: Vec<String>,
    pub mac_pinned_at: Option<Millis>,
}

/// A Mac pin is overridden by a pin this client changed after the Mac's list was last edited, and by nothing else.
pub fn is_pinned(prefs: Option<&ChatPrefs>) -> bool {
    let _ = prefs;
    unimplemented!()
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// The agent predates `/findmy/stream` (404); fall back to polling instead of retrying.
    #[error("agent: this Mac agent does not serve /findmy/stream")]
    StreamUnsupported,
    #[error("agent: {0}")]
    Http(String),
}

/// All requests but the stream time out after 10 s. Auth is `Authorization: Bearer <token>`.
pub struct MacAgentClient {
    config: AgentConfig,
    http: reqwest::Client,
}

impl MacAgentClient {
    pub fn new(config: AgentConfig, http: reqwest::Client) -> Self {
        Self { config, http }
    }

    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// `GET /health`, unauthenticated.
    pub async fn health(&self) -> Result<AgentHealth, AgentError> {
        unimplemented!()
    }

    /// `GET /findmy/friends`.
    pub async fn friends(&self) -> Result<FriendsResponse, AgentError> {
        unimplemented!()
    }

    /// `GET /findmy/devices`.
    pub async fn devices(&self) -> Result<DevicesResponse, AgentError> {
        unimplemented!()
    }

    /// `GET /findmy/stream`, server-sent events. Sends each snapshot to `tx` until
    /// the agent closes the stream. No timeout. Cancel by dropping the future.
    pub async fn stream_findmy(&self, tx: mpsc::Sender<FindMySnapshot>) -> Result<(), AgentError> {
        let _ = tx;
        unimplemented!()
    }

    /// `PUT /prefs` with `{chats, gifs}`; answers the merged set plus the Mac's own pins.
    pub async fn sync_prefs(
        &self,
        chats: &HashMap<String, ChatPrefs>,
        gifs: &HashMap<String, GifFavorite>,
    ) -> Result<SharedPrefs, AgentError> {
        let _ = (chats, gifs);
        unimplemented!()
    }
}
