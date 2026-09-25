//! Port of the change-notification path in `docs/rust-architecture.md`
//! ("How a change reaches a view"): a `StoreEvent` broadcast turns into a
//! `cx.notify()` on only the entities watching the topic it touched.
//!
//! One foreground task (`Bridge::drain`) owns the store's broadcast receiver.
//! Views never read the channel themselves; they call `bridge.watch(topic,
//! cx.entity().downgrade().into())` once, in their constructor.

use std::collections::HashMap;

use gpui_kit::{AnyWeakEntity, App, Global};
use messages_core::conversations::conversation_guid;
use messages_core::store::Incoming;
use messages_core::{MessagesStore, StoreEvent};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Topic {
    ChatList,
    Chat(String),
    Thread(String),
    Message(String),
    Typing(String),
    Selection,
    Draft(String),
    ComposerMode(String),
    Connection,
    Contacts,
    Scheduled,
    FaceTime,
    Locations,
    Focus,
    Error,
    Exporting,
    GifFavorites,
}

/// Registry of who watches what. A GPUI [`Global`]: every access is from the
/// foreground thread (a view's constructor, or `Bridge::drain`'s own task),
/// so no lock is needed.
#[derive(Default)]
pub struct Bridge {
    watchers: HashMap<Topic, Vec<AnyWeakEntity>>,
    /// `StoreEvent::Incoming` has no per-view watcher; app.rs registers one
    /// handler here to post the desktop notification.
    on_incoming: Option<Box<dyn Fn(&Incoming, &mut App)>>,
}

impl Global for Bridge {}

impl Bridge {
    pub fn install(cx: &mut App) {
        cx.set_global(Bridge::default());
    }

    /// Registers interest in `topic`. Call once per topic from the watching
    /// view's constructor with `cx.entity().downgrade().into()`.
    pub fn watch(cx: &mut App, topic: Topic, entity: AnyWeakEntity) {
        cx.global_mut::<Bridge>().watchers.entry(topic).or_default().push(entity);
    }

    /// app.rs's single handler for `StoreEvent::Incoming`: post a desktop
    /// notification when the window is not focused, per
    /// docs/rust-parity.md "Notification click activates the window...".
    pub fn on_incoming(cx: &mut App, handler: impl Fn(&Incoming, &mut App) + 'static) {
        cx.global_mut::<Bridge>().on_incoming = Some(Box::new(handler));
    }

    /// Every member chat guid a `StoreEvent` names is folded to the primary
    /// conversation guid it belongs to, so a raw socket event and the merged
    /// thread it repaints agree on one topic key.
    fn topics_for(cx: &App, event: &StoreEvent) -> Vec<Topic> {
        let fold = |guid: &str| {
            let store = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone());
            match store {
                Some(store) => conversation_guid(&store.state().grouping, guid).to_owned(),
                None => guid.to_owned(),
            }
        };
        match event {
            StoreEvent::Connection => vec![Topic::Connection],
            StoreEvent::ChatList => vec![Topic::ChatList],
            StoreEvent::Chat(guid) => vec![Topic::Chat(fold(guid))],
            StoreEvent::Thread(guid) => vec![Topic::Thread(fold(guid))],
            StoreEvent::Message { message_guid, .. } => vec![Topic::Message(message_guid.clone())],
            StoreEvent::Typing(guid) => vec![Topic::Typing(fold(guid))],
            StoreEvent::Selection => vec![Topic::Selection],
            StoreEvent::Draft(guid) => vec![Topic::Draft(fold(guid))],
            StoreEvent::ComposerMode(guid) => vec![Topic::ComposerMode(fold(guid))],
            StoreEvent::Contacts => vec![Topic::Contacts],
            StoreEvent::Scheduled => vec![Topic::Scheduled],
            StoreEvent::FaceTime => vec![Topic::FaceTime],
            StoreEvent::Locations => vec![Topic::Locations],
            StoreEvent::Focus => vec![Topic::Focus],
            StoreEvent::Error => vec![Topic::Error],
            StoreEvent::Exporting => vec![Topic::Exporting],
            StoreEvent::GifFavorites => vec![Topic::GifFavorites],
            // Handled through `on_incoming`, not the topic map.
            StoreEvent::Incoming(_) => vec![],
        }
    }

    fn dispatch(cx: &mut App, topic: &Topic) {
        let Some(entities) = cx.global_mut::<Bridge>().watchers.get_mut(topic) else { return };
        entities.retain(|weak| weak.is_upgradable());
        for weak in entities.clone() {
            cx.notify(weak.entity_id());
        }
    }

    fn dispatch_all(cx: &mut App) {
        let topics: Vec<Topic> = cx.global::<Bridge>().watchers.keys().cloned().collect();
        for topic in topics {
            Bridge::dispatch(cx, &topic);
        }
    }

    fn dispatch_incoming(cx: &mut App, incoming: &Incoming) {
        // Take the handler out for the call so it can take `&mut App` itself
        // (Rust cannot borrow `cx` mutably while `Bridge` still borrows it).
        let Some(handler) = cx.global_mut::<Bridge>().on_incoming.take() else { return };
        handler(incoming, cx);
        cx.global_mut::<Bridge>().on_incoming = Some(handler);
    }

    /// Runs for the life of the app: one task draining the store's broadcast
    /// receiver and turning each event into `cx.notify()` calls.
    pub fn drain(cx: &mut App, store: MessagesStore) {
        cx.set_global(StoreHandle(Some(store.clone())));
        cx.spawn(async move |cx| {
            let mut events = store.events();
            loop {
                match events.recv().await {
                    Ok(StoreEvent::Incoming(incoming)) => {
                        let _ = cx.update(|cx| Bridge::dispatch_incoming(cx, &incoming));
                    }
                    Ok(event) => {
                        let _ = cx.update(|cx: &mut App| {
                            for topic in Bridge::topics_for(cx, &event) {
                                Bridge::dispatch(cx, &topic);
                            }
                        });
                    }
                    // A lagged receiver missed events; the safe answer is
                    // "everything changed", so every registered view repaints once.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let _ = cx.update(Bridge::dispatch_all);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        })
        .detach();
    }
}

/// The current store, if construction succeeded. A [`Global`] so
/// `topics_for`'s conversation-guid folding can reach `state().grouping`
/// without threading the store through every call site.
#[derive(Clone, Default)]
pub struct StoreHandle(pub Option<MessagesStore>);

impl Global for StoreHandle {}
