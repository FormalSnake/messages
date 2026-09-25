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
use crate::menus::{MenuRequest, context_menu};
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
    menu: Option<MenuRequest>,
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
        let sidebar = cx.new(|cx| sidebar::Sidebar::new(window, cx));
        let header = cx.new(|cx| header::ConversationHeader::new(window, cx));
        let thread = cx.new(|cx| thread::Thread::new(window, cx));
        let composer = cx.new(|cx| composer::Composer::new(window, cx));
        let details = cx.new(|cx| details::InfoPanel::new(window, cx));
        let facetime = cx.new(|cx| facetime::FaceTimeBanner::new(window, cx));
        let connect = cx.new(|cx| ConnectScreen::new(String::new(), String::new(), |_, _, _, _| {}, |_, _| {}, window, cx));
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
        this.reconnect(cx);
        this
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
        self.new_chat_view = Some(cx.new(|cx| new_chat::NewChat::new(window, cx)));
        cx.notify();
    }

    fn on_open_switcher(&mut self, _: &OpenSwitcher, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.switcher = Some(cx.new(|cx| switcher::Switcher::new(window, cx)));
        cx.notify();
    }

    fn on_focus_search(&mut self, _: &FocusSearch, _window: &mut Window, _cx: &mut Context<Self>) {
        // D1's sidebar owns the search field; it has nothing to focus yet.
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
        // Needs the selected chat guid, which lives in `AppState` once C2's
        // store lands; wired by D1 alongside the sidebar selection it reads.
    }

    fn on_next_conversation(&mut self, _: &NextConversation, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn on_prev_conversation(&mut self, _: &PrevConversation, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// `Shell::openMenu`/`closeMenu` in context.ts: any screen can call this
    /// through `bridge`-style plumbing once D1/D2/D3 wire their menus.
    pub fn open_menu(this: &Entity<Self>, request: MenuRequest, cx: &mut App) {
        let _ = this.update(cx, |this, cx| {
            this.menu = Some(request);
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

        let close_menu = {
            let weak = weak.clone();
            move |_window: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| {
                    this.menu = None;
                    cx.notify();
                });
            }
        };
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
            .child(self.sidebar.clone())
            .child(div().w(px(1.)).h_full().flex_shrink_0().bg(palette.sidebar_border))
            .child(main_pane)
            .when(self.info_open, |el| el.child(div().h_full().flex_shrink_0().child(self.details.clone())))
            .when_some(self.menu.as_ref(), |el, request| el.child(context_menu(request, cx, close_menu.clone())))
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
