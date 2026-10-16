//! The window: conversation list in the sidebar, the open conversation (header, thread,
//! composer) as content, details in the inspector, the switcher and toasts over it all. It owns
//! the store's lifetime and every command the menus and key bindings send.

use std::time::Duration;

use messages_core::config::Config;
use messages_core::conversations::{conversation_chats, conversation_messages};
use messages_core::transport::{ConnectionStatus, TransportKind};
use messages_core::{MessagesStore, StoreOptions, chat_title};
use nd::prelude::*;

use crate::actions::*;
use crate::bridge::{Bridge, ConfigHandle, StoreHandle, Topic};
use crate::{composer, connect, details, header, new_chat, sidebar, switcher, thread};

/// How long an error toast stays up.
const TOAST_FOR: Duration = Duration::from_secs(6);
/// Sidebar width at and above 900 pt of window; narrower windows get the compact one.
const SIDEBAR_WIDTH: f32 = 320.;
const SIDEBAR_WIDTH_COMPACT: f32 = 248.;
pub const INFO_WIDTH: f32 = 280.;

enum StoreStatus {
    Loading,
    /// No server configured and not the demo: the connect screen is all there is.
    Unconfigured,
    Ready {
        store: MessagesStore,
        key: String,
    },
}

/// A new connection key means a new store; any other change edits the live one.
fn connection_key(config: &Config) -> Option<String> {
    if config.demo {
        return Some("demo".to_owned());
    }
    let server = config.server.as_ref().filter(|s| !s.url.is_empty())?;
    let agent = config.agent.as_ref().map(|agent| agent.url.as_str()).unwrap_or("");
    Some(format!("{}\u{0}{}\u{0}{agent}", server.url, server.password))
}

/// One plain sentence for a transport error. The raw text goes to the log.
pub fn toast_text(raw: &str) -> SharedString {
    let text = raw.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));
    if has(&[
        "timed out",
        "timeout",
        "connection refused",
        "error sending request",
        "dns error",
        "connection reset",
        "network unreachable",
        "no route to host",
        "connection closed",
    ]) {
        "Could not reach the Mac.".into()
    } else if has(&["status 401", "status 403", "password", "unauthori", "forbidden"]) {
        "The server password was rejected.".into()
    } else if has(&["status 404", "chat does not exist", "not found"]) {
        "The Mac could not find that chat.".into()
    } else {
        raw.to_owned().into()
    }
}

pub struct RootHandle(pub WeakEntity<Root>);

/// Runs `f` on the window's root view, when the window is open.
pub fn with_root(cx: &mut App, f: impl FnOnce(&mut Root, &mut Context<Root>)) {
    let Some(root) = cx.try_global::<RootHandle>().and_then(|h| h.0.upgrade()) else { return };
    root.update(cx, f);
}

/// The settings files changed: the store may need a new connection.
pub fn settings_changed(cx: &mut App) {
    let Some(root) = cx.try_global::<RootHandle>().and_then(|h| h.0.upgrade()) else { return };
    root.update(cx, |root, cx| root.reload_config(cx));
    crate::notify::sync_tray(cx);
}

pub struct Root {
    store: StoreStatus,
    config: Config,
    info_open: bool,
    new_chat: Option<Entity<new_chat::NewChat>>,
    switcher: switcher::Switcher,
    toast: Option<(String, SharedString)>,
    toast_epoch: u64,
    sidebar: Entity<sidebar::Sidebar>,
    header: Entity<header::Header>,
    thread: Entity<thread::Thread>,
    composer: Entity<composer::Composer>,
    details: Entity<details::Details>,
    connect: Entity<connect::Connect>,
}

impl Root {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_config(None, window, cx)
    }

    /// `config: None` reads `config.json` and the settings; tests hand one in.
    pub fn with_config(config: Option<Config>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.weak_entity();
        cx.set_global(RootHandle(weak.clone()));
        for topic in [Topic::Error, Topic::Selection, Topic::Connection, Topic::ChatList] {
            Bridge::watch(cx, topic, &weak);
        }
        let sidebar = cx.new(|cx| sidebar::Sidebar::new(window, cx));
        let header = cx.new(|cx| header::Header::new(cx));
        let thread = cx.new(|cx| thread::Thread::new(cx));
        let composer = cx.new(|cx| composer::Composer::new(cx));
        let details = cx.new(|cx| details::Details::new(cx));
        let connect = cx.new(|cx| connect::Connect::new(cx));
        let mut this = Self {
            store: StoreStatus::Loading,
            config: Config::default(),
            info_open: false,
            new_chat: None,
            switcher: switcher::Switcher::default(),
            toast: None,
            toast_epoch: 0,
            sidebar,
            header,
            thread,
            composer,
            details,
            connect,
        };
        match config {
            Some(config) => this.apply_config(config, cx),
            None => this.reload_config(cx),
        }
        this
    }

    /// A root on a store that already runs (the demo for the audit).
    pub fn with_store(config: Config, store: MessagesStore, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self::with_config(Some(Config { demo: false, server: None, ..config.clone() }), window, cx);
        cx.set_global(ConfigHandle(config));
        Bridge::drain(cx, store.clone());
        this.store = StoreStatus::Ready { store, key: "demo".into() };
        this
    }

    /// `config.json` with the environment, then the settings over it.
    pub fn reload_config(&mut self, cx: &mut Context<Self>) {
        let settings = crate::settings::get(cx);
        crate::rt::spawn_for(cx.weak_entity(), messages_core::config::load_config(), move |this, config, cx| {
            this.apply_config(settings.apply_to(config), cx);
        });
    }

    /// A new connection key builds a new store (and stops the old one); the same key only
    /// updates what views read from the config.
    pub fn apply_config(&mut self, config: Config, cx: &mut Context<Self>) {
        cx.set_global(ConfigHandle(config.clone()));
        // `MESSAGES_FONT` (or `font` in config.json) sets the text family; unset keeps the
        // platform's. Only a change touches the theme, which re-renders every view.
        if config.font != self.config.font {
            let font = config.font.clone().unwrap_or_default();
            nd::theme::set_app_theme(cx, nd::theme::ThemeRefinement::new().font(font));
        }
        let key = connection_key(&config);
        self.config = config;
        let current = match &self.store {
            StoreStatus::Ready { key, .. } => Some(key.clone()),
            _ => None,
        };
        if key != current || matches!(self.store, StoreStatus::Loading) {
            self.connect_store(key, cx);
        }
        for id in [self.header.entity_id(), self.composer.entity_id(), self.thread.entity_id()] {
            App::notify(cx, id);
        }
        cx.notify();
    }

    fn connect_store(&mut self, key: Option<String>, cx: &mut Context<Self>) {
        if let StoreStatus::Ready { store, .. } = &self.store {
            let old = store.clone();
            crate::rt::detach(async move { old.stop().await });
        }
        let Some(key) = key else {
            self.store = StoreStatus::Unconfigured;
            cx.set_global(StoreHandle(None));
            return;
        };
        let store = {
            let runtime = crate::rt::handle();
            let _guard = runtime.enter();
            build_store(&self.config, runtime.clone())
        };
        Bridge::drain(cx, store.clone());
        let starting = store.clone();
        store.spawn(async move { starting.start().await });
        self.store = StoreStatus::Ready { store, key };
        for id in [
            self.sidebar.entity_id(),
            self.header.entity_id(),
            self.thread.entity_id(),
            self.composer.entity_id(),
            self.details.entity_id(),
        ] {
            App::notify(cx, id);
        }
    }

    pub fn store(&self) -> Option<&MessagesStore> {
        match &self.store {
            StoreStatus::Ready { store, .. } => Some(store),
            _ => None,
        }
    }

    fn selected(&self) -> Option<String> {
        let store = self.store()?;
        let state = store.state();
        state.selected_chat.clone().filter(|guid| state.chat(guid).is_some())
    }

    pub fn start_new_chat(&mut self, cx: &mut Context<Self>) {
        self.info_open = false;
        let root = cx.weak_entity();
        self.new_chat = Some(cx.new(|cx| new_chat::NewChat::new(root, cx)));
        cx.notify();
    }

    pub fn close_new_chat(&mut self, cx: &mut Context<Self>) {
        self.new_chat = None;
        cx.notify();
    }

    pub fn set_info(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.info_open != open {
            self.info_open = open;
            self.details.update(cx, |details, cx| details.set_open(open, cx));
            let header = self.header.entity_id();
            App::notify(cx, header);
            cx.notify();
        }
    }

    pub fn info_open(&self) -> bool {
        self.info_open
    }

    /// Files dropped on the window or picked: staged in the composer of the open chat.
    pub fn attach_files(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        self.composer.update(cx, |composer, cx| composer.stage_files(paths, cx));
    }

    fn open_chat(&mut self, guid: String, cx: &mut Context<Self>) {
        self.new_chat = None;
        cx.notify();
        if let Some(store) = self.store().cloned() {
            crate::rt::detach(async move { store.open_chat(&guid).await });
        }
    }

    /// Cmd/Ctrl+]/[ and Shift+arrows: the next or previous conversation, wrapping.
    fn step_conversation(&mut self, delta: i32, cx: &mut Context<Self>) {
        let Some(store) = self.store() else { return };
        let guid = {
            let state = store.state();
            let chats = conversation_chats(&state);
            if chats.is_empty() {
                return;
            }
            let n = chats.len() as i32;
            let index = state.selected_chat.as_deref().and_then(|guid| chats.iter().position(|c| c.guid == guid));
            let next = match index {
                Some(index) => (index as i32 + delta).rem_euclid(n),
                None => (delta.rem_euclid(n) + n - 1) % n,
            };
            chats[next as usize].guid.clone()
        };
        self.open_chat(guid, cx);
    }

    /// Cmd/Ctrl+1..6: the n-th tapback on the newest incoming message that can take one.
    fn quick_tapback(&mut self, n: usize) {
        let Some(store) = self.store().cloned() else { return };
        let Some(kind) = messages_core::TapbackKind::NAMED.get(n.saturating_sub(1)).copied() else { return };
        let target = {
            let state = store.state();
            if !state.capabilities.reactions {
                return;
            }
            let Some(chat) = state.selected_chat.clone() else { return };
            conversation_messages(&state, &chat)
                .into_iter()
                .rev()
                .find(|m| !m.from_me && m.group_event.is_none() && m.date_retracted.is_none())
                .map(|m| (m.chat_guid.clone(), m.guid.clone()))
        };
        if let Some((chat, message)) = target {
            crate::rt::detach(async move { store.react(&chat, &message, kind, None).await });
        }
    }

    /// The store's error, as a toast for six seconds, then cleared.
    fn sync_toast(&mut self, cx: &mut Context<Self>) {
        let error = self.store().and_then(|store| store.state().error.clone());
        let Some(raw) = error else {
            self.toast = None;
            return;
        };
        if self.toast.as_ref().is_some_and(|(shown, _)| *shown == raw) {
            return;
        }
        tracing::warn!("transport: {raw}");
        self.toast = Some((raw.clone(), toast_text(&raw)));
        self.toast_epoch += 1;
        let epoch = self.toast_epoch;
        let weak = cx.weak_entity();
        crate::rt::after(TOAST_FOR, move |cx| {
            weak.update(cx, |this, cx| {
                if this.toast_epoch == epoch {
                    this.clear_toast(cx);
                }
            });
        });
    }

    fn clear_toast(&mut self, cx: &mut Context<Self>) {
        if let Some(store) = self.store() {
            store.clear_error();
        }
        self.toast = None;
        cx.notify();
    }

    fn handle_chat_action(&mut self, guid: &str, f: impl FnOnce(MessagesStore, String) + 'static) {
        if let Some(store) = self.store().cloned() {
            f(store, guid.to_owned());
        }
    }

    fn delete_chat(&mut self, guid: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = self.store().cloned() else { return };
        let title = store.state().chat(&guid).map(|chat| chat_title(chat)).unwrap_or_default();
        window.confirm(
            cx,
            format!("Delete \u{201c}{title}\u{201d}?"),
            Some("The conversation is removed on your Mac and on every device that syncs with it.".into()),
            "Delete",
            move |confirmed, _| {
                if confirmed {
                    crate::rt::detach(async move { store.delete_chat(&guid).await });
                }
            },
        );
    }

    pub fn switcher_mut(&mut self) -> &mut switcher::Switcher {
        &mut self.switcher
    }

    /// Message and attachment commands: menus and taps dispatch them from wherever focus is
    /// (often the composer), so they land here and go to the thread.
    fn message_actions(&self, root: Scaffold, cx: &mut Context<Self>) -> Scaffold {
        root.on_action(cx.listener(|this, Reply(at): &Reply, _, _| {
            if let Some(store) = this.store() {
                store.set_replying_to(&this.primary_of(&at.chat), Some(&at.message));
            }
        }))
        .on_action(cx.listener(|this, CopyText(at): &CopyText, _, _| {
            let text = this.store().and_then(|s| s.state().find_message(&at.chat, &at.message).map(|m| m.text.clone()));
            if let Some(text) = text {
                nd::system::spawn(nd::system::system().clipboard().write_text(text), |_, _| {});
            }
        }))
        .on_action(cx.listener(|this, Translate(at): &Translate, _, cx| {
            let guid = at.message.clone();
            this.thread.update(cx, |thread, cx| thread.toggle_translation(&guid, cx));
        }))
        .on_action(cx.listener(|this, Edit(at): &Edit, _, _| {
            if let Some(store) = this.store() {
                store.set_editing(&this.primary_of(&at.chat), Some(&at.message));
            }
        }))
        .on_action(cx.listener(|this, Unsend(at): &Unsend, _, _| {
            if let Some(store) = this.store().cloned() {
                let (chat, message) = (this.primary_of(&at.chat), at.message.clone());
                crate::rt::detach(async move { store.unsend(&chat, &message).await });
            }
        }))
        .on_action(cx.listener(|this, Retry(at): &Retry, _, _| {
            if let Some(store) = this.store() {
                store.retry(&this.primary_of(&at.chat), &at.message);
            }
        }))
        .on_action(cx.listener(|this, NotifyAnyway(at): &NotifyAnyway, _, _| {
            if let Some(store) = this.store().cloned() {
                let (chat, message) = (this.primary_of(&at.chat), at.message.clone());
                crate::rt::detach(async move { store.notify_silenced(&chat, &message).await });
            }
        }))
        .on_action(cx.listener(|this, OpenThread(at): &OpenThread, _, cx| {
            let guid = at.message.clone();
            this.thread.update(cx, |thread, cx| thread.open_reply_thread(&guid, cx));
        }))
        .on_action(cx.listener(|this, OpenTapbacks(at): &OpenTapbacks, _, cx| {
            let guid = at.message.clone();
            this.thread.update(cx, |thread, cx| thread.set_picker(Some(guid), cx));
        }))
        .on_action(cx.listener(|this, _: &CloseTapbacks, _, cx| {
            this.thread.update(cx, |thread, cx| thread.set_picker(None, cx));
        }))
        .on_action(cx.listener(|this, JumpTo(at): &JumpTo, _, cx| {
            let guid = at.message.clone();
            this.thread.update(cx, |thread, cx| thread.jump_to(&guid, cx));
        }))
        .on_action(cx.listener(|this, action: &React, _, _| {
            if let Some(store) = this.store().cloned() {
                let (chat, message, kind) = (action.at.chat.clone(), action.at.message.clone(), action.kind);
                crate::rt::detach(async move { store.react(&chat, &message, kind, None).await });
            }
        }))
        .on_action(cx.listener(|this, OpenAttachment(r): &OpenAttachment, _, cx| {
            let r = r.clone();
            this.thread.update(cx, |thread, cx| thread.open_attachment(&r, cx));
        }))
        .on_action(cx.listener(|this, CopyAttachment(r): &CopyAttachment, _, cx| {
            let r = r.clone();
            this.thread.update(cx, |thread, cx| thread.copy_attachment(&r, cx));
        }))
        .on_action(cx.listener(|this, CopyAttachmentName(r): &CopyAttachmentName, _, _| {
            let name = this.store().and_then(|s| {
                s.state()
                    .find_message(&r.at.chat, &r.at.message)
                    .and_then(|m| m.attachments.iter().find(|a| a.guid == r.attachment).map(|a| a.name.clone()))
            });
            if let Some(name) = name {
                nd::system::spawn(nd::system::system().clipboard().write_text(name), |_, _| {});
            }
        }))
        .on_action(cx.listener(|this, Download(r): &Download, _, cx| {
            let r = r.clone();
            this.thread.update(cx, |thread, cx| thread.download(&r, cx));
        }))
        .on_action(cx.listener(|this, Transcribe(r): &Transcribe, _, cx| {
            let r = r.clone();
            this.thread.update(cx, |thread, cx| thread.transcribe(&r, cx));
        }))
        .on_action(cx.listener(|this, _: &LightboxPrev, _, cx| {
            this.thread.update(cx, |thread, cx| thread.step_lightbox(-1, cx));
        }))
        .on_action(cx.listener(|this, _: &LightboxNext, _, cx| {
            this.thread.update(cx, |thread, cx| thread.step_lightbox(1, cx));
        }))
        .on_action(cx.listener(|this, _: &LightboxCopy, _, cx| {
            this.thread.update(cx, |thread, cx| thread.copy_lightbox(cx));
        }))
        .on_action(cx.listener(|this, _: &LightboxPlay, _, cx| {
            this.thread.update(cx, |thread, cx| {
                if let Some(lightbox) = thread.lightbox_mut() {
                    lightbox.toggle_play();
                }
                cx.notify();
            });
        }))
        .on_action(cx.listener(|this, _: &LightboxMute, _, cx| {
            this.thread.update(cx, |thread, cx| {
                if let Some(lightbox) = thread.lightbox_mut() {
                    lightbox.toggle_mute();
                }
                cx.notify();
            });
        }))
        .on_action(cx.listener(|this, ShowAttachment(r): &ShowAttachment, window, cx| {
            let (r, window) = (r.clone(), window.id());
            this.thread.update(cx, |thread, cx| thread.show_attachment(&r, window, cx));
        }))
    }

    /// The conversation a member chat folds into: replies, drafts and edits key on it.
    fn primary_of(&self, chat: &str) -> String {
        match self.store() {
            Some(store) => messages_core::conversations::conversation_guid(&store.state().grouping, chat).to_owned(),
            None => chat.to_owned(),
        }
    }

    fn actions(&self, root: Scaffold, cx: &mut Context<Self>) -> Scaffold {
        let root = self.message_actions(root, cx);
        root.on_action(cx.listener(|this, _: &Dismiss, _, cx| {
            if this.switcher.open {
                this.switcher.close();
            } else if this.new_chat.is_some() {
                this.new_chat = None;
            } else if this.info_open {
                this.set_info(false, cx);
            } else {
                cx.propagate();
                return;
            }
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &NewChat, _, cx| this.start_new_chat(cx)))
        .on_action(cx.listener(|this, _: &OpenSwitcher, _, cx| {
            this.switcher.open();
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
            this.sidebar.update(cx, |sidebar, cx| sidebar.focus_search(window, cx));
        }))
        .on_action(cx.listener(|this, _: &ToggleInfo, _, cx| this.set_info(!this.info_open, cx)))
        .on_action(cx.listener(|_, _: &OpenSettings, _, cx| connect::open_settings_window(cx)))
        .on_action(cx.listener(|this, _: &MarkUnread, _, _| {
            if let (Some(store), Some(guid)) = (this.store().cloned(), this.selected()) {
                crate::rt::detach(async move { store.mark_unread(&guid).await });
            }
        }))
        .on_action(cx.listener(|this, _: &NextConversation, _, cx| this.step_conversation(1, cx)))
        .on_action(cx.listener(|this, _: &PrevConversation, _, cx| this.step_conversation(-1, cx)))
        .on_action(cx.listener(|this, QuickTapback(n): &QuickTapback, _, _| this.quick_tapback(*n)))
        .on_action(cx.listener(|_, _: &ReportIssue, _, _| {
            if let Some(bundle) = nd::report_issue("user report") {
                let _ = nd::system::reveal_path(&bundle);
            }
        }))
        .on_action(cx.listener(|this, OpenChat(guid): &OpenChat, _, cx| this.open_chat(guid.clone(), cx)))
        .on_action(cx.listener(|this, TogglePin(guid): &TogglePin, _, _| {
            this.handle_chat_action(guid, |store, guid| store.toggle_pin(&guid));
        }))
        .on_action(cx.listener(|this, ToggleMute(guid): &ToggleMute, _, _| {
            this.handle_chat_action(guid, |store, guid| store.toggle_mute(&guid));
        }))
        .on_action(cx.listener(|this, ToggleReceipts(guid): &ToggleReceipts, _, _| {
            this.handle_chat_action(guid, |store, guid| store.toggle_read_receipts(&guid));
        }))
        .on_action(cx.listener(|this, MarkChatRead(guid): &MarkChatRead, _, _| {
            this.handle_chat_action(guid, |store, guid| crate::rt::detach(async move { store.mark_read(&guid).await }));
        }))
        .on_action(cx.listener(|this, MarkChatUnread(guid): &MarkChatUnread, _, _| {
            this.handle_chat_action(guid, |store, guid| {
                crate::rt::detach(async move { store.mark_unread(&guid).await })
            });
        }))
        .on_action(cx.listener(|this, ShowDetails(guid): &ShowDetails, _, cx| {
            this.open_chat(guid.clone(), cx);
            this.set_info(true, cx);
        }))
        .on_action(cx.listener(|this, ExportChat(guid): &ExportChat, _, _| {
            this.handle_chat_action(guid, |store, guid| {
                crate::rt::detach(async move { store.export_conversation(&guid).await })
            });
        }))
        .on_action(
            cx.listener(|this, DeleteChat(guid): &DeleteChat, window, cx| this.delete_chat(guid.clone(), window, cx)),
        )
        .on_action(cx.listener(|_, OpenLink(url): &OpenLink, _, _| {
            if let Err(error) = nd::system::open_url(url) {
                tracing::warn!("open {url}: {error}");
            }
        }))
        .on_action(cx.listener(|_, CopyAddress(address): &CopyAddress, _, _| {
            nd::system::spawn(nd::system::system().clipboard().write_text(address.clone()), |_, _| {});
        }))
        .on_action(cx.listener(|this, action: &RemoveParticipant, _, _| {
            if let Some(store) = this.store().cloned() {
                let (chat, address) = (action.chat.clone(), action.address.clone());
                crate::rt::detach(async move { store.remove_participant(&chat, &address).await });
            }
        }))
    }
}

pub fn build_store(config: &Config, runtime: tokio::runtime::Handle) -> MessagesStore {
    let http = reqwest::Client::new();
    let transport: std::sync::Arc<dyn messages_core::Transport> = if config.demo {
        std::sync::Arc::new(messages_core::demo::DemoTransport::new())
    } else {
        let (url, password) =
            config.server.as_ref().map(|server| (server.url.clone(), server.password.clone())).unwrap_or_default();
        std::sync::Arc::new(messages_core::bluebubbles::BlueBubblesTransport::new(
            messages_core::bluebubbles::BlueBubblesOptions {
                url,
                password,
                attachments_dir: messages_core::config::attachments_dir(),
            },
            http,
        ))
    };
    let cache = if config.demo {
        None
    } else {
        Some(std::sync::Arc::new(messages_core::cache::StateCache::new(&messages_core::config::cache_dir())))
    };
    let on_prefs_change: Option<messages_core::store::PrefsCallback> =
        Some(std::sync::Arc::new(|chats: &std::collections::HashMap<String, messages_core::agent::ChatPrefs>| {
            let chats = chats.clone();
            tokio::spawn(messages_core::config::save_config(move |config| config.chats = chats));
        }));
    let on_gif_favorites_change: Option<messages_core::store::GifFavoritesCallback> =
        Some(std::sync::Arc::new(|favorites: &std::collections::HashMap<String, messages_core::gifs::GifFavorite>| {
            let favorites = favorites.clone();
            tokio::spawn(messages_core::config::save_config(move |config| config.gif_favorites = Some(favorites)));
        }));
    let options = StoreOptions {
        prefs: config.chats.clone(),
        on_prefs_change,
        gif_favorites: config.gif_favorites.clone().unwrap_or_default(),
        on_gif_favorites_change,
        agent: config.agent.clone(),
        cache,
        ..Default::default()
    };
    MessagesStore::new(transport, options, runtime)
}

fn empty_state(status: ConnectionStatus, reason: Option<String>) -> AnyElement {
    let (title, body) = match status {
        ConnectionStatus::Online => {
            ("No Conversation Selected", "Pick one on the left, or start a new one.".to_owned())
        }
        ConnectionStatus::Connecting => {
            ("Connecting to Your Mac\u{2026}", "Conversations appear once the server answers.".to_owned())
        }
        ConnectionStatus::Offline => (
            "Your Mac Is Not Answering",
            match reason {
                Some(reason) => format!("Retrying. Last attempt: {reason}."),
                None => "Retrying. Check that BlueBubbles is running and reachable.".to_owned(),
            },
        ),
    };
    v_flex()
        .id("empty-state")
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .child(status_page(title).icon(Icon::Mail).description(body))
        .when(status == ConnectionStatus::Online, |col| {
            col.child(button("empty-new", "New Message").on_click(|_, window, cx| {
                crate::ui::dispatch(window, cx, NewChat);
            }))
        })
        .into_any_element()
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_toast(cx);
        crate::notify::sync_tray(cx);
        let (status, reason, has_chats, demo) = match self.store() {
            Some(store) => {
                let state = store.state();
                (
                    state.status,
                    state.connection_error.clone(),
                    !state.chats.is_empty(),
                    store.transport().kind() == TransportKind::Demo,
                )
            }
            None => (ConnectionStatus::Connecting, None, false, false),
        };
        let unconfigured = matches!(self.store, StoreStatus::Unconfigured);
        let offline_empty = status == ConnectionStatus::Offline && !has_chats && !demo && self.store().is_some();
        let selected = self.selected();
        // The pane opens at its width once; a window opened under 900 pt starts it compact.
        let width = window.viewport_size().width;
        let wide = width <= 0. || width >= 900.;

        let shown = selected.is_some() && self.new_chat.is_none();
        // The transcript runs up under the toolbar while nothing sits above it in the content;
        // the conversation then stands over it, avatar over name, as in Messages.
        let under = shown && self.header.read(cx).is_empty(cx);
        let stacked = if under { header::identity(true, true, cx) } else { None };
        let content = if unconfigured || offline_empty {
            self.connect.clone().into_any_element()
        } else if let Some(new_chat) = self.new_chat.clone() {
            new_chat.into_any_element()
        } else if selected.is_some() {
            v_flex()
                .size_full()
                .relative()
                .child(self.header.clone())
                .child(div().flex_1().min_h_0().child(self.thread.clone()))
                .child(self.composer.clone())
                .children(stacked.map(|identity| {
                    h_flex().absolute().top(px(header::IDENTITY_TOP)).left_0().right_0().justify_center().child(identity)
                }))
                .into_any_element()
        } else {
            empty_state(status, reason)
        };

        let toasts = self.toast.as_ref().map(|(_, text)| toast("error", text.clone()).timeout(TOAST_FOR));
        let info = self.info_open && selected.is_some() && self.new_chat.is_none();
        let switcher = self.switcher.element(self.store(), cx);
        // Messages' unified toolbar: compose at the start of the conversation's half, the
        // conversation in the middle, its actions at the end.
        let mut bar = toolbar()
            .hide_title()
            .navigation(
                // A toolbar item: on macOS 26 the toolbar gives it its glass.
                icon_button("new-chat", Icon::SquarePen)
                    .tooltip("New Message")
                    .on_click(|_, window, cx| crate::ui::dispatch(window, cx, NewChat)),
            )
            .flexible_space();
        if !under && let Some(identity) = header::identity(shown, false, cx) {
            bar = bar.centered_item(identity).flexible_space();
        }
        for item in header::toolbar_items(shown, cx) {
            bar = bar.item(item);
        }
        let mut root = scaffold();
        if under {
            root = root.content_under_toolbar();
        }
        let root = root
            .key_context(CONTEXT)
            .toolbar(bar)
            .sidebar(
                sidebar()
                    .width(px(if wide { SIDEBAR_WIDTH } else { SIDEBAR_WIDTH_COMPACT }))
                    .min_w(px(SIDEBAR_WIDTH_COMPACT))
                    .child(self.sidebar.clone()),
            )
            .content(content)
            .inspector(
                inspector()
                    .width(px(INFO_WIDTH))
                    .collapsed(!info)
                    .on_collapse_change(cx.listener(|this, e: &PaneCollapsed, _, cx| this.set_info(!e.collapsed, cx)))
                    .child(self.details.clone()),
            )
            .overlay(
                div().size_full().child(switcher).child(
                    toast_overlay("toasts")
                        .toasts(toasts)
                        .on_dismiss(cx.listener(|this, _: &ToastDismissed, _, cx| this.clear_toast(cx))),
                ),
            );
        let _ = demo;
        self.actions(root, cx)
    }
}
