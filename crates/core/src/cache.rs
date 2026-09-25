//! Port of packages/core/src/cache.ts: the last known chats and threads on
//! disk, so the window paints before the server answers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{Chat, Contact, Message, Millis};
use crate::store::AppState;

/// Same shape and file (`<cache_dir>/state.json`) as the TS client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedState {
    /// Always 1; anything else is ignored.
    pub version: u32,
    pub saved_at: Millis,
    pub selected_chat: Option<String>,
    pub chats: Vec<Chat>,
    pub messages: HashMap<String, Vec<Message>>,
    #[serde(default)]
    pub contacts: Vec<Contact>,
}

/// Writes are debounced (2 s, snapshot taken when the timer fires) and atomic
/// (a per-instance temp file, then rename). Serialization and I/O run on the
/// runtime's blocking pool, never on the caller.
pub struct StateCache {
    file: PathBuf,
}

impl StateCache {
    pub fn new(cache_dir: &Path) -> Self {
        Self { file: cache_dir.join("state.json") }
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    /// None when missing, unparsable or the wrong version. A null attachment mime reads back as UNKNOWN_MIME.
    pub async fn load(&self) -> Option<CachedState> {
        unimplemented!()
    }

    /// Schedules a write. `snapshot` runs when the timer fires, not now.
    pub fn schedule(&self, snapshot: Box<dyn FnOnce() -> CachedState + Send>) {
        let _ = snapshot;
        unimplemented!()
    }

    /// Writes anything pending and waits for an in-flight write, so `stop` leaves the file complete.
    pub async fn flush(&self) {
        unimplemented!()
    }
}

/// Drops in-flight sends (they would come back as ghosts) and keeps the last 100 messages per chat.
pub fn snapshot_for_cache(state: &AppState) -> CachedState {
    let _ = state;
    unimplemented!()
}
