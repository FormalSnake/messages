//! Port of `apps/desktop/src/ui/app.tsx`: the root view. Builds the tokio
//! runtime's store, wires the bridge, lays out sidebar | main pane | info
//! panel, and owns the overlay stack (menu, confirm, switcher, new chat,
//! lightbox, connect/settings) and its Escape order.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::config::Config;
use messages_core::store::Incoming;
use messages_core::{MessagesStore, StoreOptions};

use crate::bridge::Bridge;
use crate::confirm::{ConfirmRequest, confirm_dialog};
use crate::connect::ConnectScreen;
use crate::menus::{ContextMenu, MenuRequest};
use crate::theme::Theme;
use crate::toast;
use crate::{composer, details, facetime, header, lightbox, new_chat, sidebar, switcher, thread};

const CONTEXT: &str = "App";

actions!(app, [NewChat, OpenSwitcher, FocusSearch, ToggleInfo, OpenSettings, MarkUnread, NextConversation, PrevConversation, Dismiss]);
actions!(app, [Tapback1, Tapback2, Tapback3, Tapback4, Tapback5, Tapback6]);

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

enum StoreStatus {
    Loading,
    /// No server configured, a bad config, or a core stub that still panics
    /// (`unimplemented!()`) under C1/C2/C3; the message goes in the connect
    /// screen's error banner either way.
    Failed(SharedString),
    Ready(MessagesStore),
}

/// The root view. `apps/desktop/src/ui/app.tsx` splits this into `MessagesApp`
/// (owns the store) and `Workspace` (owns the overlay stack); one entity does
/// both here since GPUI has no context-provider equivalent to hand the store
/// down without threading it.
pub struct AppRoot {
    runtime: tokio::runtime::Handle,
    store: StoreStatus,
    config: Config,
    settings_open: bool,
    info_open: bool,
    new_chat: bool,
    menu: Option<Entity<ContextMenu>>,
    confirm: Option<ConfirmRequest>,
    confirm_focus: FocusHandle,
    root_focus: FocusHandle,
    toast: Option<SharedString>,

    sidebar: Entity<sidebar::Sidebar>,
    header: Entity<header::ConversationHeader>,
    thread: Entity<thread::Thread>,
    composer: Entity<composer::Composer>,
    details: Entity<details::InfoPanel>,
    facetime: Entity<facetime::FaceTimeBanner>,
    connect: Entity<ConnectScreen>,
    new_chat_view: Option<Entity<new_chat::NewChat>>,
    switcher: Option<Entity<switcher::Switcher>>,
    lightbox: Option<Entity<lightbox::Lightbox>>,
}

impl AppRoot {
    pub fn new(runtime: tokio::runtime::Handle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade();
        let sidebar = cx.new(|cx| sidebar::Sidebar::new(window, weak.clone(), cx));
        let header = cx.new(|cx| header::ConversationHeader::new(window, cx));
        let thread = cx.new(|cx| thread::Thread::new(window, cx));
        let composer = cx.new(|cx| composer::Composer::new(window, cx));
        let details = cx.new(|cx| details::InfoPanel::new(window, cx));
        let facetime = cx.new(|cx| facetime::FaceTimeBanner::new(window, cx));

        // `connect.tsx`'s `onConnect`/`onDemo`: persist the choice (so a
        // relaunch keeps it) and reconnect. Preference edits on a live store
        // never go through here.
        let runtime_for_save = runtime.clone();
        let on_connect = {
            let weak = weak.clone();
            let runtime = runtime_for_save.clone();
            move |url: String, password: String, _window: &mut Window, cx: &mut App| {
                let server = messages_core::config::ServerConfig { url, password };
                let to_save = server.clone();
                runtime.spawn(messages_core::config::save_config(move |config| {
                    config.server = Some(to_save);
                    config.demo = false;
                }));
                let _ = weak.update(cx, |this, cx| {
                    this.settings_open = false;
                    this.config.server = Some(server);
                    this.config.demo = false;
                    this.reconnect(cx);
                });
            }
        };
        let on_demo = {
            let weak = weak.clone();
            let runtime = runtime_for_save;
            move |_window: &mut Window, cx: &mut App| {
                runtime.spawn(messages_core::config::save_config(|config| config.demo = true));
                let _ = weak.update(cx, |this, cx| {
                    this.settings_open = false;
                    this.config.demo = true;
                    this.reconnect(cx);
                });
            }
        };
        let connect = cx.new(|cx| ConnectScreen::new(String::new(), String::new(), on_connect, on_demo, window, cx));
        let confirm_focus = cx.focus_handle();
        let root_focus = cx.focus_handle();
        root_focus.focus(window, cx);

        let mut this = Self {
            runtime: runtime.clone(),
            store: StoreStatus::Loading,
            config: Config::default(),
            settings_open: false,
            info_open: false,
            new_chat: false,
            menu: None,
            confirm: None,
            confirm_focus,
            root_focus,
            toast: None,
            sidebar,
            header,
            thread,
            composer,
            details,
            facetime,
            connect,
            new_chat_view: None,
            switcher: None,
            lightbox: None,
        };
        this.load_config_and_connect(cx);
        this
    }

    /// `load_config` merges `config.json` with `MESSAGES_DEMO=1`,
    /// `MESSAGES_SERVER_URL`/`PASSWORD` and friends (config.rs); this is the
    /// one place that env-driven config actually reaches the store.
    fn load_config_and_connect(&mut self, cx: &mut Context<Self>) {
        let task = self.runtime.spawn(messages_core::config::load_config());
        cx.spawn(async move |this, cx| {
            let config = task.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                this.config = config;
                this.reconnect(cx);
            });
        })
        .detach();
    }

    /// Builds the transport (demo, real, or none) from `self.config` and
    /// starts the store on the tokio runtime. A fresh connection (demo
    /// toggled, or a new server saved) calls this again; editing a
    /// preference on a live store never does.
    fn reconnect(&mut self, cx: &mut Context<Self>) {
        self.store = StoreStatus::Loading;
        let runtime = self.runtime.clone();
        let config = self.config.clone();
        let task = self.runtime.spawn(async move { bootstrap(config, runtime).await });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(store)) => {
                        Bridge::drain(cx, store.clone());
                        // So the toast wiring in `render` notices a new
                        // `state.error` without polling for one.
                        let weak = cx.entity().downgrade();
                        Bridge::watch(cx, crate::bridge::Topic::Error, weak.into());
                        this.store = StoreStatus::Ready(store);
                    }
                    Ok(Err(message)) => this.store = StoreStatus::Failed(message.into()),
                    Err(join_error) => this.store = StoreStatus::Failed(format!("core not ready yet: {join_error}").into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn store(&self) -> Option<&MessagesStore> {
        match &self.store {
            StoreStatus::Ready(store) => Some(store),
            _ => None,
        }
    }

    /// Escape closes the topmost overlay, in this order (app.tsx:316-323).
    fn on_dismiss(&mut self, _: &Dismiss, _window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
        } else if self.confirm.take().is_some() {
        } else if self.switcher.take().is_some() {
        } else if self.new_chat {
            self.new_chat = false;
            self.new_chat_view = None;
        } else if self.info_open {
            self.info_open = false;
        } else if self.settings_open {
            self.settings_open = false;
        } else {
            return;
        }
        cx.notify();
    }

    fn on_new_chat(&mut self, _: &NewChat, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.new_chat = true;
        self.info_open = false;
        let weak = cx.entity().downgrade();
        let close = move |_window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                this.new_chat = false;
                this.new_chat_view = None;
                cx.notify();
            });
        };
        self.new_chat_view = Some(cx.new(|cx| new_chat::NewChat::new(window, close, cx)));
        cx.notify();
    }

    fn on_open_switcher(&mut self, _: &OpenSwitcher, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let weak = cx.entity().downgrade();
        let close = move |_window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                this.switcher = None;
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
        self.info_open = !self.info_open;
        cx.notify();
    }

    fn on_open_settings(&mut self, _: &OpenSettings, _window: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = true;
        cx.notify();
    }

    fn on_mark_unread(&mut self, _: &MarkUnread, _window: &mut Window, _cx: &mut Context<Self>) {
        let Some(store) = self.store() else { return };
        let Some(guid) = store.state().selected_chat.clone() else { return };
        let store = store.clone();
        store.clone().spawn(async move { store.mark_unread(&guid).await });
    }

    fn on_next_conversation(&mut self, _: &NextConversation, _window: &mut Window, _cx: &mut Context<Self>) {
        self.step_conversation(1);
    }
    fn on_prev_conversation(&mut self, _: &PrevConversation, _window: &mut Window, _cx: &mut Context<Self>) {
        self.step_conversation(-1);
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
    /// open conversation, when the server supports reactions (app.tsx:293-301,
    /// 340-344). Skips group events and unsends, which never render as
    /// reactable bubbles.
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
    /// row the sidebar shows, wrapping at either end (app.tsx:282-291).
    fn step_conversation(&self, delta: i32) {
        let Some(store) = self.store() else { return };
        let (guid, chats_empty) = {
            let state = store.state();
            let chats = messages_core::conversations::conversation_chats(&state);
            if chats.is_empty() {
                (None, true)
            } else {
                let current = state.selected_chat.clone();
                let index = current.as_deref().and_then(|guid| chats.iter().position(|chat| chat.guid == guid));
                let next = match index {
                    Some(index) => ((index as i32 + delta).rem_euclid(chats.len() as i32)) as usize,
                    None => 0,
                };
                (Some(chats[next].guid.clone()), false)
            }
        };
        if chats_empty {
            return;
        }
        let Some(guid) = guid else { return };
        let store = store.clone();
        store.clone().spawn(async move { store.select_chat(Some(&guid)).await });
    }

    /// `Shell::openMenu`/`closeMenu` in context.ts: any screen can call this
    /// through `bridge`-style plumbing once D1/D2/D3 wire their menus.
    pub fn open_menu(this: &Entity<Self>, request: MenuRequest, window: &mut Window, cx: &mut App) {
        let weak = this.downgrade();
        let close = move |window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                this.menu = None;
                cx.notify();
            });
            let _ = window;
        };
        let menu = ContextMenu::open(request, close, window, cx);
        let _ = this.update(cx, |this, cx| {
            this.menu = Some(menu);
            cx.notify();
        });
    }

    /// `Shell::confirm` in context.ts: opens the yes/no dialog `AppRoot`
    /// itself renders in its overlay stack (see `confirm.rs`).
    pub fn open_confirm(this: &Entity<Self>, request: ConfirmRequest, cx: &mut App) {
        let _ = this.update(cx, |this, cx| {
            this.confirm = Some(request);
            cx.notify();
        });
    }

    /// `Shell::setInfo(true)` in context.ts: used by "Show details" in a chat
    /// row's menu, which wants the panel open regardless of its prior state
    /// (unlike Cmd+I, which toggles it).
    pub fn open_info(this: &Entity<Self>, cx: &mut App) {
        let _ = this.update(cx, |this, cx| {
            this.menu = None;
            this.info_open = true;
            cx.notify();
        });
    }
}

impl Render for AppRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        let bounds = window.viewport_size();
        let online = self.store().is_some();
        let show_connect = self.settings_open || !online;
        let weak = cx.entity().downgrade();

        // Keeps the connect screen's read-only fields in sync with the
        // store's own state each render (connect.tsx reads `state` and
        // `store.transport.kind` as props; there is no props channel here,
        // so the parent pushes them into the child instead).
        let connecting = matches!(self.store, StoreStatus::Loading);
        let error = match &self.store {
            StoreStatus::Failed(message) => Some(message.clone()),
            _ => None,
        };
        let (server, has_chats) = self.store().map(|store| {
            let state = store.state();
            (state.server.clone(), !state.chats.is_empty())
        }).unwrap_or((None, false));
        let can_close = self.settings_open && (has_chats || self.config.demo);
        let close_connect = can_close.then(|| {
            let weak = weak.clone();
            move |_window: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| {
                    this.settings_open = false;
                    cx.notify();
                });
            }
        });
        // Error toast for 6s after `state.error` appears, then `clear_error`;
        // suppressed while settings are open (app.tsx:217-225, 308). Guarded
        // by a string compare so a render that changes nothing about the
        // error does not restart the timer.
        let store_error = self.store().and_then(|store| store.state().error.clone());
        if !self.settings_open {
            if let Some(message) = store_error {
                if self.toast.as_deref() != Some(message.as_str()) {
                    self.toast = Some(message.into());
                    if let Some(store) = self.store().cloned() {
                        let weak = weak.clone();
                        cx.spawn(async move |_, cx| {
                            cx.background_executor().timer(std::time::Duration::from_secs(6)).await;
                            store.clear_error();
                            let _ = cx.update(|cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.toast = None;
                                    cx.notify();
                                });
                            });
                        })
                        .detach();
                    }
                }
            }
        }

        self.connect.update(cx, |screen, cx| {
            screen.error(error).connecting(connecting).server(server).on_close(close_connect);
            cx.notify();
        });

        let close_confirm = {
            let weak = weak.clone();
            move |_window: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| {
                    this.confirm = None;
                    cx.notify();
                });
            }
        };

        let main_pane = if self.new_chat {
            div().flex().flex_col().flex_grow(1.).min_w(px(0.)).h_full().bg(palette.canvas).when_some(self.new_chat_view.clone(), |el, view| el.child(view))
        } else {
            div().flex().flex_col().flex_grow(1.).min_w(px(0.)).h_full().bg(palette.canvas).child(self.header.clone()).child(self.thread.clone()).child(self.composer.clone())
        };

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
            .child(self.sidebar.clone())
            .child(div().w(px(1.)).h_full().flex_shrink_0().bg(palette.sidebar_border))
            .child(main_pane)
            .when(self.info_open, |el| el.child(div().h_full().flex_shrink_0().child(self.details.clone())))
            .when_some(self.menu.clone(), |el, menu| el.child(menu))
            .when_some(self.confirm.as_ref(), |el, request| el.child(confirm_dialog(request, bounds, &self.confirm_focus, close_confirm.clone(), cx)))
            .when_some(self.switcher.clone(), |el, view| el.child(view))
            .when_some(self.lightbox.clone(), |el, view| el.child(view))
            .child(self.facetime.clone())
            .when_some(self.toast.clone(), |el, message| el.child(toast::toast(&message, true, cx)))
            .when(show_connect, |el| el.child(self.connect.clone()))
    }
}

/// Builds the transport for `config` and starts the store. Runs entirely on
/// the tokio runtime; a panic inside (any of `DemoTransport::new`,
/// `MessagesStore::new`, `store.start()` while C1/C2/C3 still stub their
/// bodies with `unimplemented!()`) is isolated by `tokio::spawn` to that
/// task rather than this function, so `reconnect`'s `Err(join_error)` arm is
/// what actually reports it.
async fn bootstrap(config: Config, runtime: tokio::runtime::Handle) -> Result<MessagesStore, String> {
    let transport: std::sync::Arc<dyn messages_core::Transport> = if config.demo {
        std::sync::Arc::new(messages_core::demo::DemoTransport::new())
    } else if let Some(server) = &config.server {
        std::sync::Arc::new(messages_core::bluebubbles::BlueBubblesTransport::new(
            messages_core::bluebubbles::BlueBubblesOptions {
                url: server.url.clone(),
                password: server.password.clone(),
                attachments_dir: messages_core::config::attachments_dir(),
            },
            reqwest::Client::new(),
        ))
    } else {
        return Err("no server configured".into());
    };

    let cache = if config.demo { None } else { Some(std::sync::Arc::new(messages_core::cache::StateCache::new(&messages_core::config::cache_dir()))) };
    let options = StoreOptions { prefs: config.chats.clone(), gif_favorites: config.gif_favorites.clone().unwrap_or_default(), agent: config.agent.clone(), cache, ..Default::default() };
    let store = MessagesStore::new(transport, options, runtime);
    store.start().await;
    Ok(store)
}

/// Registered by `bridge::Bridge::on_incoming`: posts the desktop
/// notification when the window is not focused (app.tsx:81-88).
pub fn handle_incoming(_incoming: &Incoming, _cx: &mut App) {
    // `messages_core::notify::notify_incoming` is still an unimplemented
    // core stub (C3); wiring the window-focus check and the click-to-select
    // callback belongs with that landing.
}
