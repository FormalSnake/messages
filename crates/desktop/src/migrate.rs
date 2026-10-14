//! The move from `es.canarycoders.messages` (the GPUI build) to `dev.formalsnake.messages`. The
//! caches, `config.json` and `state.json` stay where the core keeps them; what moves is what
//! the user set, which now lives in `config.toml`. Runs once: a `config.toml` that exists means
//! the move happened (or the user wrote one).

use std::path::Path;

use messages_core::config::Config;

const OLD_ID: &str = "es.canarycoders.messages";

pub fn run() {
    let dir = nd_settings::config_dir("messages");
    let toml = dir.join("config.toml");
    if toml.exists() {
        return;
    }
    let Some(config) = read_json(&messages_core::config::config_file()) else { return };
    let text = import(&config);
    if text.is_empty() {
        return;
    }
    if let Err(error) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&toml, text)) {
        tracing::warn!("migrate: writing {}: {error}", toml.display());
        return;
    }
    tracing::info!("migrate: settings of {OLD_ID} moved to {}", toml.display());
}

fn read_json(path: &Path) -> Option<Config> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The `config.toml` that says what the old `config.json` said, leaving every key at its
/// default unwritten. The password and tokens stay in `config.json`.
pub fn import(config: &Config) -> String {
    let mut table = toml_edit::DocumentMut::new();
    if let Some(server) = &config.server {
        table["server_url"] = toml_edit::value(server.url.clone());
    }
    if !config.notifications {
        table["notifications"] = toml_edit::value(false);
    }
    if let Some(agent) = &config.agent {
        table["agent_url"] = toml_edit::value(agent.url.clone());
    }
    if let Some(llm) = &config.canaryllm {
        if let Some(model) = &llm.model {
            table["assistant_model"] = toml_edit::value(model.clone());
        }
        if let Some(language) = &llm.language {
            table["translate_to"] = toml_edit::value(language.clone());
        }
    }
    if table.is_empty() {
        return String::new();
    }
    format!("# Moved from {OLD_ID}'s config.json. Secrets stay there.\n{table}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use messages_core::config::ServerConfig;

    #[test]
    fn old_config_becomes_settings_that_load() {
        let config = Config {
            server: Some(ServerConfig { url: "http://mac:1234".into(), password: "secret".into() }),
            notifications: false,
            ..Config::default()
        };
        let text = import(&config);
        assert!(!text.contains("secret"));
        let dir = std::env::temp_dir().join(format!("messages-migrate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), &text).unwrap();
        let options = nd_settings::Options::new("messages").paths(nd_settings::Paths::in_dir(&dir)).env(Vec::new());
        let handle = nd_settings::SettingsHandle::<crate::settings::MessagesSettings>::load(options).unwrap();
        assert_eq!(handle.get().server_url, "http://mac:1234");
        assert!(!handle.get().notifications);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_default_config_writes_nothing() {
        assert_eq!(import(&Config::default()), "");
    }
}
