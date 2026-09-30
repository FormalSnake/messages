//! Everything that is not pixels. No UI dependency, ever: a second frontend
//! imports this crate and gets the whole backend.

pub mod agent;
pub mod assistant;
pub mod audio;
pub mod bluebubbles;
pub mod cache;
pub mod clipboard;
pub mod config;
pub mod conversations;
pub mod dedupe;
pub mod demo;
pub mod export;
pub mod findmy;
pub mod format;
pub mod fuzzy;
pub mod gifs;
pub mod image;
pub mod media;
pub mod model;
pub mod notify;
pub mod open;
pub mod process;
pub mod search;
pub mod store;
pub mod transport;
pub mod video;
#[cfg(test)]
mod testing;
#[cfg(windows)]
pub mod windows;

pub use model::*;
pub use store::{AppState, MessagesStore, StoreEvent, StoreOptions};
pub use transport::{ConnectionStatus, Transport, TransportError, TransportEvent};
