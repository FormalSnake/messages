//! What the user sets: `config.toml` in the app's config directory, `MESSAGES_*` environment
//! variables over it, and `programs.messages.settings` from Nix (the module is generated from
//! this struct, see `nix/settings-options.nix`). Secrets stay out of it: the server password,
//! the agent token and the API keys live in `config.json` or come from the environment, so a
//! Nix store path never holds them.

use messages_core::agent::AgentConfig;
use messages_core::config::{Config, ServerConfig};
use nd::prelude::*;
use nd_settings::{Options, Settings, SettingsHandle};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Settings, Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct MessagesSettings {
    /// The BlueBubbles server on your Mac, for example `http://192.168.1.20:1234`. Its password
    /// comes from `config.json` or `MESSAGES_SERVER_PASSWORD`.
    pub server_url: String,
    /// Show a notification for messages that arrive while Messages is in the background.
    pub notifications: bool,
    /// Keep an item in the menu bar or tray with the unread count.
    pub tray: bool,
    /// Run on the built-in fixtures instead of a server.
    #[settings(reload = "restart")]
    pub demo: bool,
    /// The Mac agent for Find My and shared pins, for example `http://192.168.1.20:1236`. Its
    /// token comes from `config.json` or `MESSAGES_AGENT_TOKEN`.
    pub agent_url: String,
    /// CanaryLLM model for summaries, translations and transcripts, as `provider/model`. Empty
    /// uses the default.
    pub assistant_model: String,
    /// Language translations go into. Empty follows the system language.
    pub translate_to: String,
}

impl Default for MessagesSettings {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            notifications: true,
            tray: true,
            demo: false,
            agent_url: String::new(),
            assistant_model: String::new(),
            translate_to: String::new(),
        }
    }
}

impl MessagesSettings {
    /// The core's config with these settings over what `config.json` and the environment hold.
    /// A server needs both halves: the address from here, the password from the file or env.
    pub fn apply_to(&self, mut config: Config) -> Config {
        config.demo |= self.demo;
        config.notifications = self.notifications;
        let url = self.server_url.trim().trim_end_matches('/');
        if !url.is_empty() {
            let password = config.server.as_ref().map(|s| s.password.clone()).unwrap_or_default();
            config.server = Some(ServerConfig { url: url.to_owned(), password });
        }
        let agent = self.agent_url.trim().trim_end_matches('/');
        if !agent.is_empty() {
            let token = config.agent.as_ref().map(|a| a.token.clone()).unwrap_or_default();
            config.agent = Some(AgentConfig { url: agent.to_owned(), token });
        }
        if let Some(llm) = config.canaryllm.as_mut() {
            if !self.assistant_model.is_empty() {
                llm.model = Some(self.assistant_model.clone());
            }
            if !self.translate_to.is_empty() {
                llm.language = Some(self.translate_to.clone());
            }
        }
        config
    }
}

/// The settings every view reads, and the handle the settings form writes through.
pub struct SettingsSource(pub SettingsHandle<MessagesSettings>);

/// `MESSAGES_DEMO=1`, read before anything else so a demo run never takes the instance lock.
pub fn demo_requested() -> bool {
    std::env::var("MESSAGES_DEMO").is_ok_and(|v| v == "1" || v == "true")
}

pub fn options() -> Options {
    Options::new("messages")
}

/// Loads the settings (defaults when the files do not parse), offers them to the settings
/// form, and re-reads the files when they change on disk.
pub fn install(cx: &mut App) {
    let handle = match SettingsHandle::<MessagesSettings>::load(options()) {
        Ok(handle) => handle,
        Err(error) => {
            tracing::warn!("settings: {error}");
            let empty = tempdir_options();
            SettingsHandle::load(empty).expect("default settings load")
        }
    };
    nd::components::forms::settings::provide(cx, handle.clone());
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    handle.on_event(move |_| {
        let _ = tx.send(());
    });
    let watch = handle.watch();
    cx.set_global(SettingsSource(handle));
    cx.set_global(SettingsWatch(watch.ok()));
    crate::rt::pump(rx, |(), cx| crate::root::settings_changed(cx));
}

struct SettingsWatch(#[allow(dead_code)] Option<nd_settings::WatchGuard>);

/// Settings in a directory nothing else uses, for when the real files are broken.
fn tempdir_options() -> Options {
    let dir = std::env::temp_dir().join(format!("messages-settings-{}", std::process::id()));
    Options::new("messages").paths(nd_settings::Paths::in_dir(&dir))
}

pub fn get(cx: &App) -> std::sync::Arc<MessagesSettings> {
    match cx.try_global::<SettingsSource>() {
        Some(source) => source.0.get(),
        None => std::sync::Arc::new(MessagesSettings::default()),
    }
}

pub fn handle(cx: &App) -> Option<SettingsHandle<MessagesSettings>> {
    cx.try_global::<SettingsSource>().map(|s| s.0.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_fill_in_the_config_without_dropping_secrets() {
        let mut config = Config {
            server: Some(ServerConfig { url: "http://old:1".into(), password: "pw".into() }),
            ..Config::default()
        };
        config.canaryllm =
            Some(messages_core::config::CanaryLlmConfig { api_key: "k".into(), model: None, language: None });
        let settings = MessagesSettings {
            server_url: "http://mac:1234/".into(),
            notifications: false,
            assistant_model: "openai/gpt-5".into(),
            ..MessagesSettings::default()
        };
        let config = settings.apply_to(config);
        assert_eq!(config.server, Some(ServerConfig { url: "http://mac:1234".into(), password: "pw".into() }));
        assert!(!config.notifications);
        assert_eq!(config.canaryllm.and_then(|c| c.model).as_deref(), Some("openai/gpt-5"));
    }

    /// The committed schema and the Nix options generated from it match this struct. Run with
    /// `MESSAGES_WRITE_SCHEMA=1` to rewrite both after changing a field.
    #[test]
    fn nix_options_are_generated_from_the_settings() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../nix");
        let schema = nd_settings::schema_json::<MessagesSettings>();
        let schema_text = format!("{}\n", serde_json::to_string_pretty(&schema).expect("schema serializes"));
        let module = nd_settings::nix::options_module(&schema);
        if std::env::var_os("MESSAGES_WRITE_SCHEMA").is_some() {
            std::fs::write(root.join("settings.schema.json"), &schema_text).expect("write schema");
            std::fs::write(root.join("settings-options.nix"), &module).expect("write options");
        }
        let on_disk = std::fs::read_to_string(root.join("settings.schema.json")).unwrap_or_default();
        assert_eq!(on_disk, schema_text, "nix/settings.schema.json is stale: MESSAGES_WRITE_SCHEMA=1 cargo test");
        let on_disk = std::fs::read_to_string(root.join("settings-options.nix")).unwrap_or_default();
        assert_eq!(on_disk, module, "nix/settings-options.nix is stale: MESSAGES_WRITE_SCHEMA=1 cargo test");
    }
}
