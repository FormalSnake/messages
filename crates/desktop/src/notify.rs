//! Desktop notifications, the tray item and what the OS tells the app, all through
//! `nd::system`: a message that arrives while Messages is in the background posts a
//! notification, clicking it opens that conversation; the tray shows the unread count.

use std::cell::Cell;

use messages_core::notify::notification_body;
use messages_core::store::Incoming;
use messages_core::{chat_title, conversations::conversation_chats};
use nd::prelude::*;
use nd::system::{Notification, SystemEvent, Tray, TrayIcon, TrayMenuItem};

use crate::bridge::{self, Bridge};

const TRAY: &str = "messages";

/// The tray's last unread count, so it is only rebuilt when the count moves.
#[derive(Default)]
struct TrayState {
    shown: Cell<Option<usize>>,
}

pub fn install(cx: &mut App) {
    cx.set_global(TrayState::default());
    Bridge::on_incoming(cx, post);
    nd::system::on_event(&nd::system::system(), event);
}

fn event(event: &SystemEvent, cx: &mut App) {
    match event {
        SystemEvent::NotificationClicked { data, .. } => {
            let chat = data.as_ref().and_then(|d| d.get("chat")).and_then(|c| c.as_str()).map(str::to_owned);
            raise(cx);
            if let (Some(chat), Some(store)) = (chat, bridge::store(cx)) {
                crate::rt::detach(async move { store.open_chat(&chat).await });
            }
        }
        SystemEvent::Tray { item, .. } => match item.as_deref() {
            None | Some("open") => raise(cx),
            Some("new") => {
                raise(cx);
                crate::root::with_root(cx, |root, cx| root.start_new_chat(cx));
            }
            Some("quit") => std::process::exit(0),
            Some(other) => {
                if let (Some(chat), Some(store)) = (other.strip_prefix("chat:").map(str::to_owned), bridge::store(cx)) {
                    raise(cx);
                    crate::rt::detach(async move { store.open_chat(&chat).await });
                }
            }
        },
        // Coming back to the window with a conversation open reads it, as a click in it would.
        SystemEvent::Activated => {
            if let Some(store) = bridge::store(cx) {
                let selected = store.state().selected_chat.clone();
                if let Some(guid) = selected {
                    store.engage(&guid);
                }
            }
        }
        SystemEvent::Deactivated => {
            if let Some(store) = bridge::store(cx) {
                store.disengage();
            }
        }
        SystemEvent::FileDrop { paths, .. } => {
            let paths = paths.clone();
            crate::root::with_root(cx, |root, cx| root.attach_files(paths, cx));
        }
        _ => {}
    }
}

fn raise(cx: &mut App) {
    let window = cx.window_ids().next();
    if let Some(window) = window {
        cx.activate_window(window);
    }
}

/// A notification for `incoming` while the app is in the background and notifications are on.
fn post(incoming: &Incoming, cx: &mut App) {
    if !crate::settings::get(cx).notifications || nd::system::system().is_active() {
        return;
    }
    let Some(body) = notification_body(&incoming.chat, &incoming.message, incoming.target.as_deref()) else { return };
    let title = chat_title(&incoming.chat);
    let notification = Notification::new(title)
        .body(body)
        .data(serde_json::json!({ "chat": incoming.chat.guid }))
        .action("open", "Open");
    nd::system::spawn(nd::system::system().notifications().show(notification), |shown, _| {
        if let Err(error) = shown {
            tracing::warn!("notification: {error}");
        }
    });
}

/// Keeps the tray item in step with the unread count; called whenever the chat list changes.
pub fn sync_tray(cx: &mut App) {
    let enabled = crate::settings::get(cx).tray;
    let unread: Option<Vec<(String, String)>> = bridge::store(cx).filter(|_| enabled).map(|store| {
        let state = store.state();
        conversation_chats(&state).iter().filter(|c| c.unread).map(|c| (c.guid.clone(), chat_title(c))).collect()
    });
    let Some(state) = cx.try_global::<TrayState>() else { return };
    let count = unread.as_ref().map(Vec::len);
    if state.shown.get() == count {
        return;
    }
    state.shown.set(count);
    let system = nd::system::system();
    let Some(unread) = unread else {
        nd::system::spawn(system.tray().remove(nd::system::TrayId(TRAY.into())), |_, _| {});
        return;
    };
    let mut tray = Tray::new(TRAY)
        .icon(TrayIcon::Named(if unread.is_empty() { "message".into() } else { "message.badge".into() }))
        .tooltip(match unread.len() {
            0 => "Messages".to_owned(),
            1 => "Messages: 1 unread conversation".to_owned(),
            n => format!("Messages: {n} unread conversations"),
        })
        .item(TrayMenuItem::item("open", "Open Messages"))
        .item(TrayMenuItem::item("new", "New Message"));
    if !unread.is_empty() {
        tray = tray.item(TrayMenuItem::Separator);
        for (guid, title) in unread.into_iter().take(8) {
            tray = tray.item(TrayMenuItem::item(format!("chat:{guid}"), title));
        }
    }
    tray = tray.item(TrayMenuItem::Separator).item(TrayMenuItem::item("quit", "Quit Messages"));
    if !unread_count_label(count).is_empty() {
        tray = tray.title(unread_count_label(count));
    }
    nd::system::spawn(system.tray().set(tray), |set, _| {
        if let Err(error) = set {
            tracing::warn!("tray: {error}");
        }
    });
}

fn unread_count_label(count: Option<usize>) -> String {
    match count {
        Some(n) if n > 0 => n.to_string(),
        _ => String::new(),
    }
}
