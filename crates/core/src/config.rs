//! Port of packages/core/src/config.ts. Reads and writes the same
//! `$XDG_CONFIG_HOME/messages/config.json` as the TS client, keeping keys it does not know.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::agent::{AgentConfig, ChatPrefs};
use crate::gifs::GifFavorite;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerConfig {
    pub url: String,
    pub password: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KlipyConfig {
    pub api_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanaryLlmConfig {
    pub api_key: String,
    /// Gateway model id, `provider/model`. Defaults to DEFAULT_CANARYLLM_MODEL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Target language for translation. Defaults to the system locale's language, then English.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub server: Option<ServerConfig>,
    /// Font family override. Defaults per platform in the theme.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    #[serde(default = "yes")]
    pub notifications: bool,
    /// Run against the built-in fixtures instead of a server.
    #[serde(default)]
    pub demo: bool,
    #[serde(default)]
    pub chats: HashMap<String, ChatPrefs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentConfig>,
    /// The composer's GIF button only shows when this is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub klipy: Option<KlipyConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gif_favorites: Option<HashMap<String, GifFavorite>>,
    /// Unset hides the summarize, translate and transcribe buttons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canaryllm: Option<CanaryLlmConfig>,
    /// Keys written by other versions, carried through a save untouched.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn yes() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: None,
            font: None,
            notifications: true,
            demo: false,
            chats: HashMap::new(),
            agent: None,
            klipy: None,
            gif_favorites: None,
            canaryllm: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// `$XDG_CONFIG_HOME/messages`, else `~/.config/messages` on every platform.
pub fn config_dir() -> PathBuf {
    unimplemented!()
}

/// `$XDG_CACHE_HOME/messages`, else `~/Library/Caches/messages` on macOS and `~/.cache/messages` elsewhere.
pub fn cache_dir() -> PathBuf {
    unimplemented!()
}

pub fn attachments_dir() -> PathBuf {
    cache_dir().join("attachments")
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.json")
}

/// `<config_dir>/theme.json`, the matugen palette the desktop app polls.
pub fn theme_file() -> PathBuf {
    config_dir().join("theme.json")
}

/// The file merged with the environment: MESSAGES_SERVER_URL + MESSAGES_SERVER_PASSWORD,
/// MESSAGES_FONT, MESSAGES_DEMO=1, MESSAGES_AGENT_URL + MESSAGES_AGENT_TOKEN,
/// MESSAGES_KLIPY_KEY, MESSAGES_CANARYLLM_KEY. An unparsable file logs and loads defaults.
pub async fn load_config() -> Config {
    unimplemented!()
}

/// Applies `update` to what is on disk (never to the env-merged config, so
/// overrides are never written back) and writes it with mode 0600 in a 0700 dir.
pub async fn save_config(update: impl FnOnce(&mut Config) + Send) -> anyhow::Result<Config> {
    let _ = update;
    unimplemented!()
}

pub async fn ensure_cache_dirs() -> anyhow::Result<()> {
    unimplemented!()
}
