//! Reads and writes `$XDG_CONFIG_HOME/messages/config.json`, keeping keys it does not know.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// `$XDG_CONFIG_HOME/messages`, else `~/.config/messages` on every platform.
pub fn config_dir() -> PathBuf {
    env_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home().join(".config")).join("messages")
}

/// `$XDG_CACHE_HOME/messages`, else `~/Library/Caches/messages` on macOS and `~/.cache/messages` elsewhere.
pub fn cache_dir() -> PathBuf {
    if let Some(dir) = env_dir("XDG_CACHE_HOME") {
        return dir.join("messages");
    }
    if cfg!(target_os = "macos") { home().join("Library").join("Caches").join("messages") } else { home().join(".cache").join("messages") }
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

/// What is on disk, and nothing else.
async fn read_stored_config(file: &Path) -> Config {
    let bytes = match tokio::fs::read(file).await {
        Ok(bytes) => bytes,
        Err(_) => return Config::default(),
    };
    match serde_json::from_slice(&bytes) {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!("config: cannot parse {}: {error}", file.display());
            Config::default()
        }
    }
}

fn apply_env(config: &mut Config, env: impl Fn(&str) -> Option<String>) {
    if let (Some(url), Some(password)) = (env("MESSAGES_SERVER_URL"), env("MESSAGES_SERVER_PASSWORD")) {
        config.server = Some(ServerConfig { url, password });
    }
    if let Some(font) = env("MESSAGES_FONT") {
        config.font = Some(font);
    }
    if env("MESSAGES_DEMO").as_deref() == Some("1") {
        config.demo = true;
    }
    if let (Some(url), Some(token)) = (env("MESSAGES_AGENT_URL"), env("MESSAGES_AGENT_TOKEN")) {
        config.agent = Some(AgentConfig { url, token });
    }
    if let Some(api_key) = env("MESSAGES_KLIPY_KEY") {
        config.klipy = Some(KlipyConfig { api_key });
    }
    if let Some(api_key) = env("MESSAGES_CANARYLLM_KEY") {
        match &mut config.canaryllm {
            Some(canaryllm) => canaryllm.api_key = api_key,
            None => config.canaryllm = Some(CanaryLlmConfig { api_key, model: None, language: None }),
        }
    }
}

/// The file merged with the environment: MESSAGES_SERVER_URL + MESSAGES_SERVER_PASSWORD,
/// MESSAGES_FONT, MESSAGES_DEMO=1, MESSAGES_AGENT_URL + MESSAGES_AGENT_TOKEN,
/// MESSAGES_KLIPY_KEY, MESSAGES_CANARYLLM_KEY. An unparsable file logs and loads defaults.
pub async fn load_config() -> Config {
    let mut config = read_stored_config(&config_file()).await;
    apply_env(&mut config, env_value);
    config
}

/// Applies `update` to what is on disk (never to the env-merged config, so
/// overrides are never written back) and writes it with mode 0600 in a 0700 dir.
pub async fn save_config(update: impl FnOnce(&mut Config) + Send) -> anyhow::Result<Config> {
    save_config_at(&config_file(), update).await
}

async fn save_config_at(file: &Path, update: impl FnOnce(&mut Config) + Send) -> anyhow::Result<Config> {
    let mut config = read_stored_config(file).await;
    update(&mut config);
    let mut bytes = serde_json::to_vec_pretty(&config)?;
    bytes.push(b'\n');
    let file = file.to_owned();
    tokio::task::spawn_blocking(move || write_private(&file, &bytes)).await??;
    Ok(config)
}

fn write_private(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = file.parent() {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(dir)?;
    }
    let temp = file.with_extension(format!("json.{}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut out = options.open(&temp)?;
    out.write_all(bytes)?;
    drop(out);
    std::fs::rename(&temp, file)
}

pub async fn ensure_cache_dirs() -> anyhow::Result<()> {
    tokio::fs::create_dir_all(attachments_dir()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_overrides_the_file() {
        let mut config = Config::default();
        let env = |name: &str| -> Option<String> {
            match name {
                "MESSAGES_SERVER_URL" => Some("http://mac:1234".into()),
                "MESSAGES_SERVER_PASSWORD" => Some("pw".into()),
                "MESSAGES_DEMO" => Some("1".into()),
                "MESSAGES_CANARYLLM_KEY" => Some("key".into()),
                _ => None,
            }
        };
        config.canaryllm = Some(CanaryLlmConfig { api_key: "old".into(), model: Some("m".into()), language: None });
        apply_env(&mut config, env);
        assert_eq!(config.server, Some(ServerConfig { url: "http://mac:1234".into(), password: "pw".into() }));
        assert!(config.demo);
        assert_eq!(config.canaryllm.as_ref().map(|c| (c.api_key.as_str(), c.model.as_deref())), Some(("key", Some("m"))));
        assert!(config.agent.is_none());
    }

    #[tokio::test]
    async fn saves_into_the_file_as_it_is_and_keeps_unknown_keys() {
        let dir = std::env::temp_dir().join(format!("messages-config-{}-{}", std::process::id(), fastrand::u64(..)));
        let file = dir.join("config.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&file, br#"{"server":{"url":"u","password":"p"},"notifications":false,"future":{"x":1}}"#).unwrap();
        let saved = save_config_at(&file, |config| config.font = Some("Inter".into())).await.unwrap();
        assert_eq!(saved.font.as_deref(), Some("Inter"));
        let text = std::fs::read_to_string(&file).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["future"]["x"], 1);
        assert_eq!(value["notifications"], false);
        assert_eq!(value["server"]["url"], "u");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::write(&file, b"{ broken").unwrap();
        assert_eq!(read_stored_config(&file).await, Config::default());
        std::fs::remove_dir_all(&dir).ok();
    }
}
