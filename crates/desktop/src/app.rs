//! The root view. Builds the tokio runtime's store, wires the bridge, lays
//! out sidebar | main pane | info panel, and owns the overlay stack (menu,
//! confirm, switcher, new chat, lightbox, connect/settings) and its Escape order.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::config::Config;
use messages_core::notify::{NotifyAction, NotifyOptions, notify_incoming};
use messages_core::store::Incoming;
use messages_core::transport::{ConnectionStatus, TransportKind};
use messages_core::{MessagesStore, StoreOptions};

use crate::bridge::{Bridge, ConfigHandle, Topic};
use crate::confirm::{ConfirmRequest, confirm_dialog};
use crate::connect::ConnectScreen;
use crate::icons::{Icon, IconName};
use crate::menus::{ContextMenu, MenuRequest, shortcut};
use crate::motion::{DURATION_BASE, DURATION_FAST, DURATION_PANEL, Presence};
use crate::primitives::{Button, ButtonKind};
use crate::theme::{INFO_WIDTH, SIDEBAR_WIDTH, SIDEBAR_WIDTH_COMPACT, TITLEBAR_HEIGHT, Theme, spacing, type_scale};
use crate::{composer, details, facetime, header, motion, new_chat, sidebar, switcher, thread, toast};

const CONTEXT: &str = "App";

actions!(app, [NewChat, OpenSwitcher, FocusSearch, ToggleInfo, OpenSettings, MarkUnread, NextConversation, PrevConversation, Dismiss]);
actions!(app, [Tapback1, Tapback2, Tapback3, Tapback4, Tapback5, Tapback6, ToggleFrameOverlay]);

/// Registers key bindings. Call once at startup, before any window opens.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-n", NewChat, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-n", NewChat, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-k", OpenSwitcher, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-k", OpenSwitcher, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-f", FocusSearch, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-f", FocusSearch, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-i", ToggleInfo, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-i", ToggleInfo, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-,", OpenSettings, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-,", OpenSettings, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-u", MarkUnread, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-u", MarkUnread, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-]", NextConversation, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-]", NextConversation, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-down", NextConversation, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-down", NextConversation, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-[", PrevConversation, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-[", PrevConversation, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-up", PrevConversation, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-up", PrevConversation, Some(CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(CONTEXT)),
        #[cfg(feature = "frame-overlay")]
        KeyBinding::new("f12", ToggleFrameOverlay, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-1", Tapback1, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-1", Tapback1, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-2", Tapback2, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-2", Tapback2, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-3", Tapback3, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-3", Tapback3, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-4", Tapback4, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-4", Tapback4, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-5", Tapback5, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-5", Tapback5, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-6", Tapback6, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-6", Tapback6, Some(CONTEXT)),
    ]);
}

/// One plain sentence for the toast. `TransportError`'s Display drops the
/// HTTP status, so the store's text is all there is to go on; the raw text
/// goes to stderr for anyone debugging.
fn toast_text(raw: &str) -> SharedString {
    let text = raw.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));
    if has(&["timed out", "timeout", "connection refused", "error sending request", "dns error", "connection reset", "network unreachable", "no route to host", "connection closed"]) {
        "Could not reach the Mac.".into()
    } else if has(&["status 401", "status 403", "password", "unauthori", "forbidden"]) {
        "The server password was rejected.".into()
    } else if has(&["status 404", "chat does not exist", "not found"]) {
        "The Mac could not find that chat.".into()
    } else {
        // Anything else is a hint from the server itself ("Turn off Encrypt
        // communications"), which reads better than a generic line.
        raw.to_owned().into()
    }
}

/// The error toast stays up this long, then `clear_error`.
const TOAST_FOR: std::time::Duration = std::time::Duration::from_secs(6);

enum StoreStatus {
    /// `config.json` has not been read yet.
    Loading,
    /// No server configured and not in demo mode: only the connect screen shows.
    Unconfigured,
    Ready { store: MessagesStore, key: String },
}

/// A new connection key means a new store; anything else
/// (pins, mutes, notifications) edits the live one.
fn connection_key(config: &Config) -> Option<String> {
    if config.demo {
        return Some("demo".to_owned());
    }
    let server = config.server.as_ref()?;
    Some(format!("{}\u{0}{}\u{0}{}", server.url, server.password, config.agent.as_ref().map(|agent| agent.url.as_str()).unwrap_or("")))
}

/// Weak handle to the window's root, for screens that open a menu or a
/// confirm without being handed the root at construction.
pub struct RootHandle(pub WeakEntity<AppRoot>);

impl Global for RootHandle {}

pub fn root(cx: &App) -> Option<Entity<AppRoot>> {
    cx.try_global::<RootHandle>().and_then(|handle| handle.0.upgrade())
}

/// The root view. One entity owns both the store and the overlay stack,
/// since GPUI has no context-provider equivalent to hand the store down
/// without threading it.
pub struct AppRoot {
    runtime: tokio::runtime::Handle,
    window: AnyWindowHandle,
    store: StoreStatus,
    config: Config,
    settings_open: bool,
    info_open: bool,
    new_chat: bool,
    menu: Option<Entity<ContextMenu>>,
    confirm: Option<ConfirmRequest>,
    confirm_shown: Presence<ConfirmRequest>,
    focus_confirm: bool,
    confirm_focus: FocusHandle,
    root_focus: FocusHandle,
    /// The error being shown, and which timer owns its dismissal.
    toast: Option<SharedString>,
    /// The raw transport text the toast stands for, so the same failure
    /// reported twice does not restart the timer.
    toast_for: Option<String>,
    toast_epoch: u64,
    toast_shown: Presence<SharedString>,
    info_shown: Presence<()>,
    details_open: bool,
    connect_shown: Presence<()>,

    sidebar: Entity<sidebar::Sidebar>,
    header: Entity<header::ConversationHeader>,
    thread: Entity<thread::Thread>,
    composer: Entity<composer::Composer>,
    details: Entity<details::InfoPanel>,
    facetime: Entity<facetime::FaceTimeBanner>,
    connect: Option<Entity<ConnectScreen>>,
    new_chat_view: Option<Entity<new_chat::NewChat>>,
    switcher: Option<Entity<switcher::Switcher>>,
}

impl AppRoot {
    pub fn new(runtime: tokio::runtime::Handle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_config(runtime, None, window, cx)
    }

    /// `config: None` reads `config.json`; the tests hand one in.
    pub fn with_config(runtime: tokio::runtime::Handle, config: Option<Config>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade();
        cx.set_global(RootHandle(weak.clone()));
        motion::install(cx);
        #[cfg(feature = "frame-overlay")]
        match std::env::var("MESSAGES_FRAME_OVERLAY").ok().as_deref() {
            Some("full") => window.set_debug_frame_overlay_mode(DebugFrameOverlayMode::Full),
            Some("minimal") => window.set_debug_frame_overlay_mode(DebugFrameOverlayMode::Minimal),
            _ => {}
        }
        Bridge::watch(cx, Topic::Error, weak.clone().into());
        Bridge::watch(cx, Topic::Selection, weak.clone().into());
        Bridge::watch(cx, Topic::Connection, weak.clone().into());
        Bridge::watch(cx, Topic::ChatList, weak.clone().into());
        {
            let weak = weak.clone();
            Bridge::on_incoming(cx, move |incoming, cx| {
                let _ = weak.update(cx, |this, cx| this.notify_incoming(incoming, cx));
            });
        }

        let sidebar = cx.new(|cx| sidebar::Sidebar::new(window, weak.clone(), cx));
        let header = cx.new(|cx| header::ConversationHeader::new(window, cx));
        let thread = cx.new(|cx| thread::Thread::new(window, cx));
        let composer = cx.new(|cx| composer::Composer::new(window, cx));
        let details = cx.new(|cx| details::InfoPanel::new(window, cx));
        let facetime = cx.new(|cx| facetime::FaceTimeBanner::new(window, cx));
        let confirm_focus = cx.focus_handle();
        let root_focus = cx.focus_handle();
        root_focus.focus(window, cx);
        cx.observe_window_activation(window, |this, window, _| {
            if !window.is_window_active() {
                if let Some(store) = this.store() {
                    store.disengage();
                }
            }
        })
        .detach();

        let mut this = Self {
            runtime,
            window: window.window_handle(),
            store: StoreStatus::Loading,
            config: Config::default(),
            settings_open: false,
            info_open: false,
            new_chat: false,
            menu: None,
            confirm: None,
            confirm_shown: Presence::new(DURATION_FAST),
            focus_confirm: false,
            confirm_focus,
            root_focus,
            toast: None,
            toast_for: None,
            toast_epoch: 0,
            toast_shown: Presence::new(DURATION_FAST),
            info_shown: Presence::new(DURATION_PANEL),
            details_open: false,
            connect_shown: Presence::new(DURATION_FAST),
            sidebar,
            header,
            thread,
            composer,
            details,
            facetime,
            connect: None,
            new_chat_view: None,
            switcher: None,
        };
        match config {
            Some(config) => this.apply_config(config, cx),
            None => this.load_config_and_connect(cx),
        }
        #[cfg(feature = "screenshot")]
        if let Ok(out) = std::env::var("MESSAGES_SCREENSHOT") {
            screenshot(out.into(), window, cx);
        }
        this
    }

    /// `load_config` merges `config.json` with `MESSAGES_DEMO=1`,
    /// `MESSAGES_SERVER_URL`/`PASSWORD` and friends (config.rs); this is the
    /// one place that env-driven config actually reaches the store.
    fn load_config_and_connect(&mut self, cx: &mut Context<Self>) {
        let task = self.runtime.spawn(messages_core::config::load_config());
        cx.spawn(async move |this, cx| {
            let config = task.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| this.apply_config(config, cx));
        })
        .detach();
    }

    /// Takes a new config. A new connection key builds a new store (and stops
    /// the old one); the same key only updates what views read from it.
    pub fn apply_config(&mut self, config: Config, cx: &mut Context<Self>) {
        cx.set_global(ConfigHandle(config.clone()));
        // Both read the optional integrations (CanaryLLM, Klipy) from it.
        self.header.update(cx, |_, cx| cx.notify());
        self.composer.update(cx, |_, cx| cx.notify());
        let key = connection_key(&config);
        self.config = config;
        let current = match &self.store {
            StoreStatus::Ready { key, .. } => Some(key.clone()),
            _ => None,
        };
        if key != current || matches!(self.store, StoreStatus::Loading) {
            self.connect_store(key, cx);
        }
        cx.notify();
    }

    fn connect_store(&mut self, key: Option<String>, cx: &mut Context<Self>) {
        if let StoreStatus::Ready { store, .. } = &self.store {
            let old = store.clone();
            self.runtime.spawn(async move { old.stop().await });
        }
        let Some(key) = key else {
            self.store = StoreStatus::Unconfigured;
            cx.set_global(crate::bridge::StoreHandle(None));
            return;
        };
        let store = {
            let _guard = self.runtime.enter();
            build_store(&self.config, self.runtime.clone())
        };
        Bridge::drain(cx, store.clone());
        let starting = store.clone();
        store.spawn(async move { starting.start().await });
        self.store = StoreStatus::Ready { store, key };
        cx.notify();
    }

    fn store(&self) -> Option<&MessagesStore> {
        match &self.store {
            StoreStatus::Ready { store, .. } => Some(store),
            _ => None,
        }
    }

    /// A desktop notification while the window is not focused; clicking it
    /// selects the conversation (the store folds a member chat into its
    /// primary) and raises the window.
    fn notify_incoming(&mut self, incoming: &Incoming, cx: &mut Context<Self>) {
        if !self.config.notifications {
            return;
        }
        let Some(store) = self.store().cloned() else { return };
        let window = self.window;
        if window.update(cx, |_, window, _| window.is_window_active()).unwrap_or(false) {
            return;
        }
        let chat = incoming.chat.clone();
        let message = incoming.message.clone();
        let options = NotifyOptions { target: incoming.target.as_deref().cloned(), icon: Some(crate::assets::icon_svg_path()) };
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let _ = tx.send(notify_incoming(&chat, &message, options).await);
        });
        let chat_guid = incoming.chat.guid.clone();
        cx.spawn(async move |_, cx| {
            if let Ok(Some(NotifyAction::Open)) = rx.await {
                let selecting = store.clone();
                store.spawn(async move { selecting.select_chat(Some(&chat_guid)).await });
                let _ = window.update(cx, |_, window, _| window.activate_window());
            }
        })
        .detach();
    }

    /// Hands focus back to the composer when a chat is open, else the root,
    /// once an overlay that held it goes away.
    fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_chat_shown(cx) && !self.new_chat {
            self.composer.update(cx, |composer, cx| composer.focus(window, cx));
        } else {
            self.root_focus.focus(window, cx);
        }
    }

    fn selected_chat_shown(&self, _cx: &App) -> bool {
        self.store().is_some_and(|store| {
            let state = store.state();
            state.selected_chat.as_deref().is_some_and(|guid| state.chat(guid).is_some())
        })
    }

    /// Escape closes the topmost overlay, in this order.
    /// With nothing to close the key goes on to whoever has focus (the
    /// composer's reply banner, the search field, the lightbox).
    fn on_dismiss(&mut self, _: &Dismiss, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
        } else if self.confirm.take().is_some() {
        } else if self.switcher.take().is_some() {
        } else if self.new_chat {
            self.new_chat = false;
            self.new_chat_view = None;
        } else if self.info_open {
            self.set_info(false, cx);
        } else if self.settings_open {
            self.settings_open = false;
        } else {
            cx.propagate();
            return;
        }
        self.restore_focus(window, cx);
        cx.notify();
    }

    #[cfg(feature = "frame-overlay")]
    fn on_toggle_frame_overlay(&mut self, _: &ToggleFrameOverlay, window: &mut Window, _cx: &mut Context<Self>) {
        window.cycle_debug_frame_overlay_mode();
        window.refresh();
    }

    fn on_new_chat(&mut self, _: &NewChat, window: &mut Window, cx: &mut Context<Self>) {
        self.start_new_chat(window, cx);
    }

    pub fn start_new_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.new_chat = true;
        self.set_info(false, cx);
        let weak = cx.entity().downgrade();
        let close = move |window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                this.new_chat = false;
                this.new_chat_view = None;
                this.restore_focus(window, cx);
                cx.notify();
            });
        };
        self.new_chat_view = Some(cx.new(|cx| new_chat::NewChat::new(window, close, cx)));
        cx.notify();
    }

    fn on_open_switcher(&mut self, _: &OpenSwitcher, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let weak = cx.entity().downgrade();
        let close = move |window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                this.switcher = None;
                this.restore_focus(window, cx);
                cx.notify();
            });
        };
        self.switcher = Some(cx.new(|cx| switcher::Switcher::new(window, close, cx)));
        cx.notify();
    }

    fn on_focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar.update(cx, |sidebar, cx| sidebar.focus_search(window, cx));
    }

    fn on_toggle_info(&mut self, _: &ToggleInfo, _window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.set_info(!self.info_open, cx);
        cx.notify();
    }

    fn on_open_settings(&mut self, _: &OpenSettings, _window: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = true;
        cx.notify();
    }

    /// A click, scroll or keystroke in the open conversation's pane: the only
    /// thing that reads it. Selecting it from the sidebar does not.
    fn engage(&mut self, window: &Window) {
        if !window.is_window_active() {
            return;
        }
        let Some(store) = self.store() else { return };
        let Some(guid) = store.state().selected_chat.clone() else { return };
        store.engage(&guid);
    }

    fn on_mark_unread(&mut self, _: &MarkUnread, _window: &mut Window, _cx: &mut Context<Self>) {
        let Some(store) = self.store() else { return };
        let Some(guid) = store.state().selected_chat.clone() else { return };
        let store = store.clone();
        store.clone().spawn(async move { store.mark_unread(&guid).await });
    }

    fn on_next_conversation(&mut self, _: &NextConversation, _window: &mut Window, cx: &mut Context<Self>) {
        self.step_conversation(1, cx);
    }
    fn on_prev_conversation(&mut self, _: &PrevConversation, _window: &mut Window, cx: &mut Context<Self>) {
        self.step_conversation(-1, cx);
    }

    fn on_tapback_1(&mut self, _: &Tapback1, _window: &mut Window, _cx: &mut Context<Self>) {
        self.tapback(messages_core::TapbackKind::Love);
    }
    fn on_tapback_2(&mut self, _: &Tapback2, _window: &mut Window, _cx: &mut Context<Self>) {
        self.tapback(messages_core::TapbackKind::Like);
    }
    fn on_tapback_3(&mut self, _: &Tapback3, _window: &mut Window, _cx: &mut Context<Self>) {
        self.tapback(messages_core::TapbackKind::Dislike);
    }
    fn on_tapback_4(&mut self, _: &Tapback4, _window: &mut Window, _cx: &mut Context<Self>) {
        self.tapback(messages_core::TapbackKind::Laugh);
    }
    fn on_tapback_5(&mut self, _: &Tapback5, _window: &mut Window, _cx: &mut Context<Self>) {
        self.tapback(messages_core::TapbackKind::Emphasize);
    }
    fn on_tapback_6(&mut self, _: &Tapback6, _window: &mut Window, _cx: &mut Context<Self>) {
        self.tapback(messages_core::TapbackKind::Question);
    }

    /// Cmd/Ctrl+1..6: reacts to the newest incoming reactable message in the
    /// open conversation, when the server supports reactions. Skips group
    /// events and unsends, which never render as reactable bubbles.
    fn tapback(&self, kind: messages_core::TapbackKind) {
        let Some(store) = self.store() else { return };
        let target = {
            let state = store.state();
            if !state.capabilities.reactions {
                return;
            }
            let Some(chat_guid) = state.selected_chat.clone() else { return };
            messages_core::conversations::conversation_messages(&state, &chat_guid)
                .into_iter()
                .rev()
                .find(|message| !message.from_me && message.group_event.is_none() && message.date_retracted.is_none())
                .map(|message| (chat_guid, message.guid.clone()))
        };
        let Some((chat_guid, message_guid)) = target else { return };
        let store = store.clone();
        store.clone().spawn(async move { store.react(&chat_guid, &message_guid, kind, None).await });
    }

    /// Cmd/Ctrl+]/[ and their Shift+arrow twins: selects the next or previous
    /// row the sidebar shows, wrapping at either end.
    fn step_conversation(&mut self, delta: i32, cx: &mut Context<Self>) {
        let Some(store) = self.store() else { return };
        let guid = {
            let state = store.state();
            let chats = messages_core::conversations::conversation_chats(&state);
            if chats.is_empty() {
                return;
            }
            let current = state.selected_chat.clone();
            let index = current.as_deref().and_then(|guid| chats.iter().position(|chat| chat.guid == guid));
            let next = match index {
                Some(index) => ((index as i32 + delta).rem_euclid(chats.len() as i32)) as usize,
                None => ((delta.rem_euclid(chats.len() as i32) + chats.len() as i32 - 1) % chats.len() as i32) as usize,
            };
            chats[next].guid.clone()
        };
        let store = store.clone();
        self.new_chat = false;
        self.new_chat_view = None;
        cx.notify();
        store.clone().spawn(async move { store.select_chat(Some(&guid)).await });
    }

    pub fn open_menu(this: &Entity<Self>, request: MenuRequest, window: &mut Window, cx: &mut App) {
        let weak = this.downgrade();
        let close = move |window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                this.menu = None;
                this.restore_focus(window, cx);
                cx.notify();
            });
        };
        let menu = ContextMenu::open(request, close, window, cx);
        let _ = this.update(cx, |this, cx| {
            this.menu = Some(menu);
            cx.notify();
        });
    }

    /// Which menu is up, so a container can tell whether a child already
    /// answered the click it is looking at.
    pub fn menu_id(&self) -> Option<EntityId> {
        self.menu.as_ref().map(|menu| menu.entity_id())
    }

    /// Opens the yes/no dialog `AppRoot` itself renders in its overlay
    /// stack (see `confirm.rs`).
    pub fn open_confirm(this: &Entity<Self>, request: ConfirmRequest, cx: &mut App) {
        let _ = this.update(cx, |this, cx| {
            this.menu = None;
            this.confirm = Some(request);
            this.focus_confirm = true;
            cx.notify();
        });
    }

    /// Used by "Show details" in a chat row's menu, which wants the panel
    /// open regardless of its prior state (unlike Cmd+I, which toggles it).
    pub fn open_info(this: &Entity<Self>, cx: &mut App) {
        let _ = this.update(cx, |this, cx| {
            this.menu = None;
            this.set_info(true, cx);
            cx.notify();
        });
    }

    pub fn info_open(&self) -> bool {
        self.info_open
    }

    /// The header is a cached view that reads `info_open` for its button,
    /// so every change has to reach it too.
    fn set_info(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.info_open != open {
            self.info_open = open;
            self.header.update(cx, |_, cx| cx.notify());
        }
    }

    /// The toast timer: `state.error` shows for six seconds, then clears.
    fn sync_toast(&mut self, show_connect: bool, cx: &mut Context<Self>) {
        let error = self.store().and_then(|store| store.state().error.clone());
        if !self.settings_open {
            if let Some(raw) = error {
                if self.toast_for.as_deref() != Some(raw.as_str()) {
                    eprintln!("transport: {raw}");
                    self.toast_for = Some(raw.clone());
                    self.toast = Some(toast_text(&raw));
                    self.toast_epoch += 1;
                    let epoch = self.toast_epoch;
                    let store = self.store().cloned();
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(TOAST_FOR).await;
                        let _ = this.update(cx, |this, cx| {
                            if this.toast_epoch != epoch {
                                return;
                            }
                            if let Some(store) = store {
                                store.clear_error();
                            }
                            this.toast = None;
                            this.toast_for = None;
                            cx.notify();
                        });
                    })
                    .detach();
                }
            }
        }
        let notice = self.toast.clone().filter(|_| !show_connect);
        self.toast_shown.set(notice, |this: &mut Self| &mut this.toast_shown, cx);
    }

    fn connect_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<ConnectScreen> {
        if let Some(screen) = &self.connect {
            return screen.clone();
        }
        let weak = cx.entity().downgrade();
        let runtime = self.runtime.clone();
        let on_connect = {
            let weak = weak.clone();
            let runtime = runtime.clone();
            move |url: String, password: String, _window: &mut Window, cx: &mut App| {
                let server = messages_core::config::ServerConfig { url, password };
                let to_save = server.clone();
                runtime.spawn(messages_core::config::save_config(move |config| {
                    config.server = Some(to_save);
                    config.demo = false;
                }));
                let _ = weak.update(cx, |this, cx| {
                    this.settings_open = false;
                    let mut config = this.config.clone();
                    config.server = Some(server);
                    config.demo = false;
                    this.apply_config(config, cx);
                });
            }
        };
        let on_demo = move |_window: &mut Window, cx: &mut App| {
            runtime.spawn(messages_core::config::save_config(|config| config.demo = true));
            let _ = weak.update(cx, |this, cx| {
                this.settings_open = false;
                let mut config = this.config.clone();
                config.demo = true;
                this.apply_config(config, cx);
            });
        };
        let (url, password) = self.config.server.as_ref().map(|server| (server.url.clone(), server.password.clone())).unwrap_or_default();
        let screen = cx.new(|cx| ConnectScreen::new(url, password, on_connect, on_demo, window, cx));
        self.connect = Some(screen.clone());
        screen
    }
}

/// `bun run screenshot`: once the demo has a conversation open, give the
/// images a moment to decode, render the frame offscreen (animations jumped to
/// their end) and write it as a PNG, then quit.
#[cfg(feature = "screenshot")]
fn screenshot(out: std::path::PathBuf, window: &mut Window, cx: &mut Context<AppRoot>) {
    cx.set_reduce_motion(true);
    cx.spawn_in(window, async move |_, cx| {
        let started = std::time::Instant::now();
        loop {
            cx.background_executor().timer(std::time::Duration::from_millis(100)).await;
            let ready = cx.update(|_, cx| crate::bridge::store(cx).is_some_and(|store| store.state().selected_chat.is_some())).unwrap_or(false);
            if ready || started.elapsed() > std::time::Duration::from_secs(30) {
                break;
            }
        }
        cx.background_executor().timer(std::time::Duration::from_millis(2500)).await;
        let _ = cx.update(|window, cx| {
            if let Ok(scene) = std::env::var("MESSAGES_SCREENSHOT_SCENE") {
                screenshot_scene(&scene, window, cx);
            }
            window.refresh();
        });
        cx.background_executor().timer(std::time::Duration::from_millis(400)).await;
        let _ = cx.update(|window, _| window.refresh());
        cx.background_executor().timer(std::time::Duration::from_millis(200)).await;
        let _ = cx.update(|window, cx| {
            match window.render_to_image() {
                Ok(image) => {
                    if let Some(parent) = out.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    match image.save(&out) {
                        Ok(()) => println!("[screenshot] wrote {}", out.display()),
                        Err(error) => eprintln!("[screenshot] {error}"),
                    }
                }
                Err(error) => eprintln!("[screenshot] {error}"),
            }
            cx.quit();
        });
    })
    .detach();
}

/// `MESSAGES_SCREENSHOT_SCENE` opens one overlay before the frame is taken,
/// so each surface can be checked without a pointer.
#[cfg(feature = "screenshot")]
fn screenshot_scene(scene: &str, window: &mut Window, cx: &mut App) {
    let Some(root) = root(cx) else { return };
    match scene {
        "switcher" => window.dispatch_action(Box::new(OpenSwitcher), cx),
        "new-chat" => window.dispatch_action(Box::new(NewChat), cx),
        "info" => window.dispatch_action(Box::new(ToggleInfo), cx),
        "settings" => window.dispatch_action(Box::new(OpenSettings), cx),
        "menu" | "confirm" => {
            let Some(store) = crate::bridge::store(cx) else { return };
            let chat = {
                let state = store.state();
                state.selected_chat.as_deref().and_then(|guid| state.chat(guid).cloned())
            };
            let Some(chat) = chat else { return };
            if scene == "menu" {
                let items = crate::sidebar_row::chat_menu(&chat, &store, root.downgrade(), true);
                AppRoot::open_menu(&root, crate::menus::MenuRequest::at(point(px(200.), px(150.)), items), window, cx);
            } else {
                crate::sidebar_row::confirm_delete(&chat, &store, root.downgrade(), cx);
            }
        }
        _ => {}
    }
}

fn build_store(config: &Config, runtime: tokio::runtime::Handle) -> MessagesStore {
    let http = reqwest::Client::new();
    let transport: std::sync::Arc<dyn messages_core::Transport> = if config.demo {
        std::sync::Arc::new(messages_core::demo::DemoTransport::new())
    } else {
        let (url, password) = config.server.as_ref().map(|server| (server.url.clone(), server.password.clone())).unwrap_or_default();
        std::sync::Arc::new(messages_core::bluebubbles::BlueBubblesTransport::new(
            messages_core::bluebubbles::BlueBubblesOptions { url, password, attachments_dir: messages_core::config::attachments_dir() },
            http,
        ))
    };
    let cache = if config.demo { None } else { Some(std::sync::Arc::new(messages_core::cache::StateCache::new(&messages_core::config::cache_dir()))) };
    let on_prefs_change: Option<messages_core::store::PrefsCallback> = Some(std::sync::Arc::new(|chats: &std::collections::HashMap<String, messages_core::agent::ChatPrefs>| {
        let chats = chats.clone();
        tokio::spawn(messages_core::config::save_config(move |config| config.chats = chats));
    }));
    let on_gif_favorites_change: Option<messages_core::store::GifFavoritesCallback> = Some(std::sync::Arc::new(|favorites: &std::collections::HashMap<String, messages_core::gifs::GifFavorite>| {
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

fn empty_state(status: ConnectionStatus, reason: Option<String>, cx: &mut Context<AppRoot>) -> impl IntoElement {
    let palette = Theme::get(cx);
    let online = status == ConnectionStatus::Online;
    let title = match status {
        ConnectionStatus::Online => "No conversation selected",
        ConnectionStatus::Connecting => "Connecting to your Mac\u{2026}",
        ConnectionStatus::Offline => "Your Mac is not answering",
    };
    let body = match status {
        ConnectionStatus::Online => "Pick one on the left, or start a new one.".to_owned(),
        ConnectionStatus::Connecting => "Conversations appear once the server answers.".to_owned(),
        ConnectionStatus::Offline => match reason {
            Some(reason) => format!("Retrying. Last attempt: {reason}."),
            None => "Retrying. Check that BlueBubbles is running and reachable.".to_owned(),
        },
    };
    div()
        .id("empty-state")
        .flex_grow(1.)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(spacing::X2)
        .px(spacing::X6)
        .child(Icon::new(IconName::Conversation).size(px(30.)).color(palette.tertiary))
        .child(div().text_size(type_scale::TITLE.font_size).line_height(type_scale::TITLE.line_height).font_weight(FontWeight::SEMIBOLD).text_color(palette.text).text_align(TextAlign::Center).child(title))
        .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).text_align(TextAlign::Center).child(body))
        .when(online, |el| {
            el.child(div().pt(spacing::X2).child(Button::new("empty-new-message", format!("New message  {}", shortcut("N", false, false))).kind(ButtonKind::Primary).on_click(
                cx.listener(|this, _, window, cx| this.start_new_chat(window, cx)),
            )))
        })
}

impl Render for AppRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::trace::render("AppRoot");
        let palette = Theme::get(cx);
        let bounds = window.viewport_size();
        let weak = cx.entity().downgrade();

        let (status, connection_error, server, has_chats, selected, demo) = match self.store() {
            Some(store) => {
                let state = store.state();
                let selected = state.selected_chat.as_deref().is_some_and(|guid| state.chat(guid).is_some());
                (state.status, state.connection_error.clone(), state.server.clone(), !state.chats.is_empty(), selected, store.transport().kind() == TransportKind::Demo)
            }
            None => (ConnectionStatus::Connecting, None, None, false, false, false),
        };
        let unconfigured = matches!(self.store, StoreStatus::Unconfigured);
        let show_connect = self.settings_open || unconfigured || (status == ConnectionStatus::Offline && !has_chats && !demo && self.store().is_some());

        self.sync_toast(show_connect, cx);

        let was_shown = self.connect_shown.is_open();
        self.connect_shown.set(show_connect.then_some(()), |this: &mut Self| &mut this.connect_shown, cx);
        if show_connect {
            let screen = self.connect_screen(window, cx);
            let error = if status == ConnectionStatus::Offline { connection_error.clone().map(SharedString::from) } else { None };
            let can_close = self.settings_open && (has_chats || demo);
            let close = can_close.then(|| {
                let weak = weak.clone();
                move |window: &mut Window, cx: &mut App| {
                    let _ = weak.update(cx, |this, cx| {
                        this.settings_open = false;
                        this.restore_focus(window, cx);
                        cx.notify();
                    });
                }
            });
            screen.update(cx, |screen, _| {
                screen.error(error).connecting(status == ConnectionStatus::Connecting && self.store().is_some()).server(server).on_close(close);
            });
        } else if !self.connect_shown.is_mounted() {
            self.connect = None;
        }
        if was_shown && !show_connect {
            self.restore_focus(window, cx);
        }

        let info_wanted = self.info_open && selected && !self.new_chat;
        self.info_shown.set(info_wanted.then_some(()), |this: &mut Self| &mut this.info_shown, cx);
        let details_open = self.info_shown.is_mounted();
        if details_open != self.details_open {
            self.details_open = details_open;
            self.details.update(cx, |details, cx| details.set_open(details_open, cx));
        }

        self.confirm_shown.set(self.confirm.clone(), |this: &mut Self| &mut this.confirm_shown, cx);
        if self.focus_confirm && self.confirm.is_some() {
            self.focus_confirm = false;
            window.focus(&self.confirm_focus, cx);
        }

        let info_open = self.info_shown.is_open();
        self.facetime.update(cx, |banner, cx| banner.set_offset(if info_open { INFO_WIDTH + spacing::X3 } else { spacing::X3 }, cx));

        let close_confirm = {
            let weak = weak.clone();
            move |window: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| {
                    this.confirm = None;
                    this.restore_focus(window, cx);
                    cx.notify();
                });
            }
        };

        let main_pane = div()
            .id("main-pane")
            .flex()
            .flex_col()
            .flex_grow(1.)
            .min_w(px(0.))
            .h_full()
            .bg(palette.canvas)
            .capture_any_mouse_down(cx.listener(|this, _, window, _| this.engage(window)))
            .capture_key_down(cx.listener(|this, _, window, _| this.engage(window)))
            .on_scroll_wheel(cx.listener(|this, _, window, _| this.engage(window)));
        let main_pane = if self.new_chat {
            main_pane.when_some(self.new_chat_view.clone(), |el, view| el.child(view))
        } else if selected {
            main_pane
                .child(AnyView::from(self.header.clone()).cached(StyleRefinement::default().w_full().h(TITLEBAR_HEIGHT).flex_shrink_0()))
                // Not cached: re-rendering a cached view forces every cached
                // view inside it to re-render too (gpui's `refreshing`), so a
                // GIF frame in one row would redraw every visible row. Uncached,
                // the thread's own render is a pointer comparison and the rows
                // stay cached.
                .child(self.thread.clone())
                .child(self.composer.clone())
        } else {
            main_pane.child(empty_state(status, connection_error, cx))
        };

        // A column of its own, never a sheet over the thread, so a trackpad
        // scroll over the panel stops at the panel. The clip box animates and
        // the panel inside keeps its width, so nothing in it reflows mid-slide.
        let info = self.info_shown.is_mounted().then(|| {
            let clip = div().h_full().flex_shrink_0().overflow_hidden().flex().flex_row().justify_end().child(div().w(INFO_WIDTH).h_full().flex_shrink_0().child(self.details.clone()));
            motion::slide_width(clip, self.info_shown.id("info-slide"), info_open, INFO_WIDTH)
        });

        let dialog = self.confirm_shown.current().cloned().map(|request| {
            let open = self.confirm_shown.is_open();
            confirm_dialog(&request, bounds, &self.confirm_focus, close_confirm.clone(), open, self.confirm_shown.id("confirm-fade"), cx)
        });

        let notice = self.toast_shown.current().cloned().map(|message| {
            let open = self.toast_shown.is_open();
            let pill = toast::toast(&message, cx);
            motion::toward(pill, self.toast_shown.id("toast"), open, DURATION_BASE, DURATION_FAST, |el, t| el.pb(toast::BOTTOM - toast::RISE * (1. - t)).opacity(t))
        });

        let connect = self.connect.clone().filter(|_| self.connect_shown.is_mounted()).map(|screen| {
            let open = self.connect_shown.is_open();
            let layer = div().absolute().inset_0().flex().bg(palette.canvas).child(div().flex_grow(1.).flex().child(screen));
            motion::fade(layer, self.connect_shown.id("connect-fade"), open, DURATION_BASE, DURATION_FAST)
        });

        div()
            .key_context(CONTEXT)
            .id("app-root")
            .track_focus(&self.root_focus)
            .size_full()
            .relative()
            .flex()
            .flex_row()
            .bg(palette.canvas)
            .text_color(palette.text)
            .on_action(cx.listener(Self::on_dismiss))
            .when(cfg!(feature = "frame-overlay"), |el| {
                #[cfg(feature = "frame-overlay")]
                let el = el.on_action(cx.listener(Self::on_toggle_frame_overlay));
                el
            })
            .on_action(cx.listener(Self::on_new_chat))
            .on_action(cx.listener(Self::on_open_switcher))
            .on_action(cx.listener(Self::on_focus_search))
            .on_action(cx.listener(Self::on_toggle_info))
            .on_action(cx.listener(Self::on_open_settings))
            .on_action(cx.listener(Self::on_mark_unread))
            .on_action(cx.listener(Self::on_next_conversation))
            .on_action(cx.listener(Self::on_prev_conversation))
            .on_action(cx.listener(Self::on_tapback_1))
            .on_action(cx.listener(Self::on_tapback_2))
            .on_action(cx.listener(Self::on_tapback_3))
            .on_action(cx.listener(Self::on_tapback_4))
            .on_action(cx.listener(Self::on_tapback_5))
            .on_action(cx.listener(Self::on_tapback_6))
            .child(div().absolute().top_0().left_0().w_full().h(TITLEBAR_HEIGHT).child(crate::chrome::drag_layer("title-drag", window, cx)))
            .child(AnyView::from(self.sidebar.clone()).cached(StyleRefinement::default().w(if bounds.width < px(900.) { SIDEBAR_WIDTH_COMPACT } else { SIDEBAR_WIDTH }).h_full().flex_shrink_0()))
            .child(div().w(px(1.)).h_full().flex_shrink_0().bg(palette.sidebar_border))
            .child(main_pane)
            .children(info)
            .when_some(self.menu.clone(), |el, menu| el.child(menu))
            .children(dialog)
            .when_some(self.switcher.clone(), |el, view| el.child(view))
            .when(crate::chrome::has_caption_buttons(window), |el| el.child(div().absolute().top_0().right_0().occlude().child(crate::chrome::caption_buttons())))
            .child(self.facetime.clone())
            .children(notice)
            .children(connect)
            .children(crate::trace::probe())
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
