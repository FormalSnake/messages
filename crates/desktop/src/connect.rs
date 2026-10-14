//! "Connect to your Mac": the content when there is no server yet (or the Mac stopped answering
//! before any chat loaded), and the Connection section of the settings window. The address
//! goes to `config.toml`, the password to `config.json`, so a Nix-managed settings file never
//! holds a secret.

use messages_core::transport::ConnectionStatus;
use messages_core::{ServerInfo, transport::TransportKind};
use nd::prelude::*;

use crate::bridge;
use crate::settings::MessagesSettings;

pub struct Connect {
    url: SharedString,
    password: SharedString,
    loaded: bool,
}

/// `^https?://\S+$` and a password.
pub fn valid(url: &str, password: &str) -> bool {
    let url = url.trim();
    let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"));
    rest.is_some_and(|rest| !rest.is_empty() && !rest.chars().any(char::is_whitespace)) && !password.is_empty()
}

impl Connect {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let weak = cx.weak_entity();
        crate::bridge::Bridge::watch(cx, crate::bridge::Topic::Connection, &weak);
        Self { url: SharedString::default(), password: SharedString::default(), loaded: false }
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        if self.loaded {
            return;
        }
        let Some(config) = bridge::config(cx) else { return };
        self.loaded = true;
        if let Some(server) = &config.server {
            self.url = server.url.clone().into();
            self.password = server.password.clone().into();
        }
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if !valid(&self.url, &self.password) {
            return;
        }
        let url = self.url.trim().trim_end_matches('/').to_owned();
        let password = self.password.to_string();
        connect(url, password, false, cx);
    }
}

/// Saves a server (or the demo) and reconnects: the address into the settings, the password
/// into `config.json`.
pub fn connect(url: String, password: String, demo: bool, cx: &mut App) {
    if let Some(handle) = crate::settings::handle(cx) {
        let written = if demo { handle.set("demo", &true) } else { handle.set("server_url", &url) };
        if let Err(error) = written {
            tracing::warn!("settings: {error:?}");
        }
        if !demo && handle.get().demo {
            let _ = handle.set("demo", &false);
        }
    }
    crate::rt::spawn(
        messages_core::config::save_config(move |config| {
            if !demo {
                config.server = Some(messages_core::config::ServerConfig { url, password });
                config.demo = false;
            }
        }),
        |_, cx| crate::root::settings_changed(cx),
    );
}

fn server_info(server: &ServerInfo, theme: &Theme) -> AnyElement {
    let private_api = server.private_api && server.helper_connected;
    let line = |title: &str, value: String| {
        h_flex()
            .gap_2()
            .child(text(title.to_owned()).text_color(theme.text.secondary).w(px(140.)))
            .child(text(value).flex_1())
    };
    v_flex()
        .id("server-info")
        .gap_1()
        .child(line("BlueBubbles", server.version.clone()))
        .child(line("macOS", server.macos_version.clone().unwrap_or_else(|| "unknown".into())))
        .child(line("iCloud account", server.icloud_account.clone().unwrap_or_else(|| "not detected".into())))
        .child(
            h_flex()
                .gap_2()
                .child(text("Private API").text_color(theme.text.secondary).w(px(140.)))
                .child(crate::ui::dot(if private_api { theme.success } else { theme.warning }, 8.))
                .child(text(if private_api { "On" } else { "Off" })),
        )
        .when(!private_api, |col| {
            col.child(
                label("Tapbacks, typing, read receipts, edits and unsend need the Private API on the Mac.")
                    .secondary()
                    .wraps(),
            )
        })
        .into_any_element()
}

impl Render for Connect {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.load(cx);
        let theme = cx.theme().clone();
        let (status, error, server, first_run) = match bridge::store(cx) {
            Some(store) => {
                let state = store.state();
                let demo = store.transport().kind() == TransportKind::Demo;
                (state.status, state.connection_error.clone(), state.server.clone(), !demo && state.chats.is_empty())
            }
            None => (ConnectionStatus::Offline, None, None, true),
        };
        let connecting = status == ConnectionStatus::Connecting && bridge::store(cx).is_some();
        let ready = valid(&self.url, &self.password) && !connecting;
        v_flex().id("connect").size_full().items_center().justify_center().child(
            v_flex()
                .w(px(420.))
                .gap_4()
                .p_6()
                .child(label("Connect to Your Mac").text_style(TextStyle::Title2))
                .child(
                    label(
                        "Messages talks to BlueBubbles, a free server that runs on your Mac and reads iMessage there.",
                    )
                    .secondary()
                    .wraps(),
                )
                .child(
                    text_field("server-url")
                        .text(self.url.clone())
                        .placeholder("http://192.168.1.20:1234")
                        .label("Server address")
                        .on_change(cx.listener(|this, e: &TextChanged, _, cx| {
                            this.url = e.text.clone();
                            cx.notify();
                        }))
                        .on_submit(cx.listener(|this, _: &TextSubmitted, _, cx| this.submit(cx))),
                )
                .child(
                    secure_field("server-password")
                        .text(self.password.clone())
                        .placeholder("Password")
                        .label("Password")
                        .on_change(cx.listener(|this, e: &TextChanged, _, cx| {
                            this.password = e.text.clone();
                            cx.notify();
                        }))
                        .on_submit(cx.listener(|this, _: &TextSubmitted, _, cx| this.submit(cx))),
                )
                .when_some(error.filter(|_| status == ConnectionStatus::Offline), |col, error| {
                    col.child(label(error).tone(LabelTone::Danger).wraps().id("connect-error"))
                })
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            button("connect-button", if connecting { "Connecting\u{2026}" } else { "Connect" })
                                .primary()
                                .disabled(!ready)
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.submit(cx))),
                        )
                        .when(first_run, |row| {
                            row.child(
                                button("connect-demo", "Use Demo Data")
                                    .on_click(|_, _, cx| connect(String::new(), String::new(), true, cx)),
                            )
                        }),
                )
                .children(server.map(|server| server_info(&server, &theme))),
        )
    }
}

/// The settings window: the connection, then everything `MessagesSettings` declares as native
/// settings rows (a key Nix manages shows read-only with its option path).
pub struct SettingsWindow {
    connect: Entity<Connect>,
}

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("settings")
            .size_full()
            .overflow_y_scroll()
            .gap_4()
            .p_4()
            .child(self.connect.clone())
            .child(settings_form::<MessagesSettings>())
    }
}

pub fn open_settings_window(cx: &mut App) {
    open_settings(cx, |_, cx| {
        let connect = cx.new(|cx| Connect::new(cx));
        cx.new(|_| SettingsWindow { connect })
    });
}

#[cfg(test)]
mod tests {
    use super::valid;

    #[test]
    fn a_server_needs_an_http_address_and_a_password() {
        assert!(valid("http://192.168.1.20:1234", "pw"));
        assert!(valid(" https://mac.local ", "pw"));
        assert!(!valid("192.168.1.20:1234", "pw"));
        assert!(!valid("http://", "pw"));
        assert!(!valid("http://a b", "pw"));
        assert!(!valid("http://mac", ""));
    }
}
