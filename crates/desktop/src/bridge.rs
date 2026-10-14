//! Store events to view notifies (docs/rust-architecture.md, "How a change reaches a view"): a
//! `StoreEvent` broadcast turns into `cx.notify` on only the entities watching the topic it
//! touched. Views register once with `Bridge::watch(cx, topic, &cx.weak_entity())`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use messages_core::config::Config;
use messages_core::conversations::conversation_guid;
use messages_core::store::Incoming;
use messages_core::{MessagesStore, StoreEvent};
use nd::prelude::*;

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

struct Watcher {
    id: EntityId,
    alive: Box<dyn Fn() -> bool>,
}

#[derive(Default)]
struct State {
    watchers: HashMap<Topic, Vec<Watcher>>,
    on_incoming: Option<Rc<dyn Fn(&Incoming, &mut App)>>,
    /// Bumped by every `drain`, so the pump of a store that was replaced stops.
    epoch: u64,
}

/// An app global. nd's `App` hands out globals by shared reference only, so the state sits in
/// a `RefCell`.
#[derive(Default)]
pub struct Bridge(RefCell<State>);

impl Bridge {
    pub fn install(cx: &mut App) {
        cx.set_global(Bridge::default());
        cx.set_global(StoreHandle(None));
    }

    fn get(cx: &mut App) -> std::cell::RefMut<'_, State> {
        if !cx.has_global::<Bridge>() {
            cx.set_global(Bridge::default());
        }
        cx.global::<Bridge>().0.borrow_mut()
    }

    /// Registers `entity`'s interest in `topic`; a second call for the same pair is a no-op.
    pub fn watch<V: 'static>(cx: &mut App, topic: Topic, entity: &WeakEntity<V>) {
        let id = entity.entity_id();
        let weak = entity.clone();
        let mut state = Self::get(cx);
        let watchers = state.watchers.entry(topic).or_default();
        if !watchers.iter().any(|w| w.id == id) {
            watchers.push(Watcher { id, alive: Box::new(move || weak.upgrade().is_some()) });
        }
    }

    /// Drops `entity`'s interest in `topic`, and the topic once nobody watches it.
    pub fn unwatch(cx: &mut App, topic: &Topic, id: EntityId) {
        let mut state = Self::get(cx);
        let watchers = &mut state.watchers;
        let Some(entities) = watchers.get_mut(topic) else { return };
        entities.retain(|w| w.id != id && (w.alive)());
        if entities.is_empty() {
            watchers.remove(topic);
        }
    }

    /// The one handler for `StoreEvent::Incoming` (the desktop notification).
    pub fn on_incoming(cx: &mut App, handler: impl Fn(&Incoming, &mut App) + 'static) {
        Self::get(cx).on_incoming = Some(Rc::new(handler));
    }

    /// Every member chat guid an event names folds to its conversation's primary guid, so a raw
    /// socket event and the merged thread it repaints agree on one topic.
    fn topics_for(cx: &App, event: &StoreEvent) -> Vec<Topic> {
        let fold = |guid: &str| match store(cx) {
            Some(store) => conversation_guid(&store.state().grouping, guid).to_owned(),
            None => guid.to_owned(),
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
            StoreEvent::Incoming(_) => vec![],
        }
    }

    fn dispatch(cx: &mut App, topic: &Topic) {
        let ids: Vec<EntityId> = {
            let mut state = Self::get(cx);
            let watchers = &mut state.watchers;
            let Some(entities) = watchers.get_mut(topic) else { return };
            entities.retain(|w| (w.alive)());
            let ids = entities.iter().map(|w| w.id).collect::<Vec<_>>();
            if ids.is_empty() {
                watchers.remove(topic);
            }
            ids
        };
        for id in ids {
            cx.notify(id);
        }
    }

    fn dispatch_all(cx: &mut App) {
        let topics: Vec<Topic> = Self::get(cx).watchers.keys().cloned().collect();
        for topic in topics {
            Self::dispatch(cx, &topic);
        }
    }

    /// Delivers one store event; the pump and tests call it.
    pub fn deliver(cx: &mut App, event: Result<StoreEvent, tokio::sync::broadcast::error::RecvError>) {
        match event {
            Ok(StoreEvent::Incoming(incoming)) => {
                let handler = Self::get(cx).on_incoming.clone();
                if let Some(handler) = handler {
                    handler(&incoming, cx);
                }
            }
            Ok(event) => {
                for topic in Self::topics_for(cx, &event) {
                    Self::dispatch(cx, &topic);
                }
            }
            // A lagged receiver missed events: every registered view repaints once.
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => Self::dispatch_all(cx),
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {}
        }
    }

    /// Makes `store` current and pumps its broadcast channel into views for as long as it is.
    pub fn drain(cx: &mut App, store: MessagesStore) {
        cx.set_global(StoreHandle(Some(store.clone())));
        let epoch = {
            let mut state = Self::get(cx);
            state.epoch += 1;
            state.epoch
        };
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut events = store.events();
        crate::rt::handle().spawn(async move {
            loop {
                let event = events.recv().await;
                let closed = matches!(event, Err(tokio::sync::broadcast::error::RecvError::Closed));
                if tx.send(event).is_err() || closed {
                    break;
                }
            }
        });
        crate::rt::pump(rx, move |event, cx| {
            let current = Self::get(cx).epoch == epoch;
            if current {
                Self::deliver(cx, event);
            }
        });
    }
}

/// The current store, once a connection (or the demo) is configured.
#[derive(Clone, Default)]
pub struct StoreHandle(pub Option<MessagesStore>);

pub fn store(cx: &App) -> Option<MessagesStore> {
    cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone())
}

/// The effective core config: `config.json` with the settings and environment over it.
#[derive(Clone, Default)]
pub struct ConfigHandle(pub Config);

pub fn config(cx: &App) -> Option<&Config> {
    cx.try_global::<ConfigHandle>().map(|handle| &handle.0)
}
