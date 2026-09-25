//! Port of packages/core/src/cache.ts: the last known chats and threads on
//! disk, so the window paints before the server answers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::task::AbortHandle;

use crate::model::{Chat, Contact, Message, Millis};
use crate::store::AppState;

const MESSAGES_PER_CHAT: usize = 100;
const SAVE_DELAY: Duration = Duration::from_millis(2000);

/// Same shape and file (`<cache_dir>/state.json`) as the TS client. Rows are
/// the store's own `Arc`s, so taking a snapshot copies pointers, not messages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedState {
    /// Always 1; anything else is ignored.
    pub version: u32,
    #[serde(default)]
    pub saved_at: Millis,
    #[serde(default)]
    pub selected_chat: Option<String>,
    pub chats: Vec<Arc<Chat>>,
    #[serde(default)]
    pub messages: HashMap<String, Vec<Arc<Message>>>,
    #[serde(default)]
    pub contacts: Arc<Vec<Contact>>,
}

type Snapshot = Box<dyn FnOnce() -> CachedState + Send>;

struct Shared {
    file: PathBuf,
    temp: PathBuf,
    pending: Mutex<Option<Snapshot>>,
    timer: Mutex<Option<AbortHandle>>,
    /// Held for the length of a write, so `flush` can wait out one in flight.
    writing: tokio::sync::Mutex<()>,
}

/// Writes are debounced (2 s, snapshot taken when the timer fires) and atomic
/// (a per-instance temp file, then rename). Serialization and I/O run on the
/// runtime's blocking pool, never on the caller.
pub struct StateCache {
    shared: Arc<Shared>,
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl StateCache {
    pub fn new(cache_dir: &Path) -> Self {
        let file = cache_dir.join("state.json");
        // A reconnect builds a new store, and its cache, beside the old one; two
        // of them writing the same temp file lose a snapshot to a failed rename.
        let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
        let temp = cache_dir.join(format!("state.json.{}-{n}.tmp", std::process::id()));
        Self {
            shared: Arc::new(Shared {
                file,
                temp,
                pending: Mutex::new(None),
                timer: Mutex::new(None),
                writing: tokio::sync::Mutex::new(()),
            }),
        }
    }

    pub fn file(&self) -> &Path {
        &self.shared.file
    }

    /// None when missing, unparsable or the wrong version. A null attachment mime reads back as UNKNOWN_MIME.
    pub async fn load(&self) -> Option<CachedState> {
        let file = self.shared.file.clone();
        let bytes = match tokio::fs::read(&file).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => {
                tracing::warn!("cache: ignoring {}: {error}", file.display());
                return None;
            }
        };
        let parsed = tokio::task::spawn_blocking(move || serde_json::from_slice::<CachedState>(&bytes)).await.ok()?;
        match parsed {
            Ok(state) if state.version == 1 => Some(state),
            Ok(_) => None,
            Err(error) => {
                tracing::warn!("cache: ignoring {}: {error}", file.display());
                None
            }
        }
    }

    /// Schedules a write. `snapshot` runs when the timer fires, not now.
    /// Must be called inside the runtime's context (`Handle::enter`).
    pub fn schedule(&self, snapshot: Box<dyn FnOnce() -> CachedState + Send>) {
        *self.shared.pending.lock() = Some(snapshot);
        let mut timer = self.shared.timer.lock();
        if timer.is_some() {
            return;
        }
        let shared = self.shared.clone();
        let task = tokio::spawn(async move {
            tokio::time::sleep(SAVE_DELAY).await;
            shared.timer.lock().take();
            flush(&shared).await;
        });
        *timer = Some(task.abort_handle());
    }

    /// Writes anything pending and waits for an in-flight write, so `stop` leaves the file complete.
    pub async fn flush(&self) {
        if let Some(timer) = self.shared.timer.lock().take() {
            timer.abort();
        }
        flush(&self.shared).await;
    }
}

async fn flush(shared: &Arc<Shared>) {
    let snapshot = shared.pending.lock().take();
    let _writing = shared.writing.lock().await;
    let Some(snapshot) = snapshot else { return };
    let state = snapshot();
    let file = shared.file.clone();
    let temp = shared.temp.clone();
    let result = tokio::task::spawn_blocking(move || write(&file, &temp, &state)).await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!("cache: {error}"),
        Err(error) => tracing::warn!("cache: {error}"),
    }
}

fn write(file: &Path, temp: &Path, state: &CachedState) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = file.parent() {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(dir)?;
    }
    let bytes = serde_json::to_vec(state).map_err(std::io::Error::other)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut out = options.open(temp)?;
    out.write_all(&bytes)?;
    drop(out);
    std::fs::rename(temp, file)
}

/// Drops in-flight sends (they would come back as ghosts) and keeps the last 100 messages per chat.
pub fn snapshot_for_cache(state: &AppState) -> CachedState {
    let mut messages = HashMap::with_capacity(state.messages.len());
    for (guid, list) in &state.messages {
        let settled: Vec<Arc<Message>> = list
            .iter()
            .filter(|message| message.temp_guid.as_deref() != Some(message.guid.as_str()))
            .cloned()
            .collect();
        if settled.is_empty() {
            continue;
        }
        let start = settled.len().saturating_sub(MESSAGES_PER_CHAT);
        messages.insert(guid.clone(), settled[start..].to_vec());
    }
    CachedState {
        version: 1,
        saved_at: chrono::Utc::now().timestamp_millis(),
        selected_chat: state.selected_chat.clone(),
        chats: state.chats.clone(),
        messages,
        contacts: state.contacts.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Attachment, Service, UNKNOWN_MIME};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("messages-cache-{name}-{}-{}", std::process::id(), fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn message(chat: &str, guid: &str, date: Millis) -> Message {
        Message { guid: guid.into(), chat_guid: chat.into(), text: format!("text {guid}"), date, ..Message::default() }
    }

    #[tokio::test]
    async fn repairs_an_attachment_an_older_build_cached_with_no_mime() {
        let dir = temp_dir("mime");
        let json = serde_json::json!({
            "version": 1,
            "savedAt": 1,
            "selectedChat": "a",
            "chats": [{ "guid": "a", "identifier": "a", "service": "iMessage", "isGroup": false, "participants": [], "pinned": false, "muted": false, "archived": false, "unread": false, "lastActivity": 1 }],
            "contacts": [],
            "messages": { "a": [{ "guid": "m1", "chatGuid": "a", "text": "Your bill is ready", "fromMe": false, "date": 1, "service": "iMessage", "tapbacks": [], "isAudio": false,
                "attachments": [{ "guid": "brand-logo", "name": "BrandLogoImage", "mime": null, "bytes": 40150, "isSticker": false, "hidden": false }] }] }
        });
        std::fs::write(dir.join("state.json"), serde_json::to_vec(&json).unwrap()).unwrap();
        let loaded = StateCache::new(&dir).load().await.unwrap();
        assert_eq!(loaded.messages["a"][0].attachments[0].mime, UNKNOWN_MIME);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn ignores_a_missing_corrupt_or_foreign_file() {
        let dir = temp_dir("corrupt");
        let cache = StateCache::new(&dir);
        assert!(cache.load().await.is_none());
        std::fs::write(cache.file(), b"{ not json").unwrap();
        assert!(cache.load().await.is_none());
        std::fs::write(cache.file(), br#"{"version":2,"chats":[]}"#).unwrap();
        assert!(cache.load().await.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test(start_paused = true)]
    async fn debounces_writes_and_flushes_what_is_pending() {
        let dir = temp_dir("debounce");
        let cache = StateCache::new(&dir);
        let snapshot = |n: i64| -> Snapshot {
            Box::new(move || CachedState { version: 1, saved_at: n, selected_chat: None, chats: vec![], messages: HashMap::new(), contacts: Arc::new(vec![]) })
        };
        cache.schedule(snapshot(1));
        cache.schedule(snapshot(2));
        assert!(!cache.file().exists());
        tokio::time::sleep(Duration::from_millis(2100)).await;
        // The write itself runs on the blocking pool; flush waits it out.
        cache.flush().await;
        assert_eq!(cache.load().await.unwrap().saved_at, 2);
        cache.schedule(snapshot(3));
        cache.flush().await;
        assert_eq!(cache.load().await.unwrap().saved_at, 3);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(cache.file()).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn snapshot_drops_in_flight_sends_and_keeps_the_last_hundred() {
        let mut state = AppState::new(HashMap::new());
        let mut rows: Vec<Arc<Message>> = (0..150).map(|n| Arc::new(message("a", &format!("m{n}"), n))).collect();
        rows.push(Arc::new(Message { temp_guid: Some("temp-1".into()), ..message("a", "temp-1", 200) }));
        state.messages.insert("a".into(), rows);
        state.messages.insert("b".into(), vec![Arc::new(Message { temp_guid: Some("temp-2".into()), ..message("b", "temp-2", 1) })]);
        let snapshot = snapshot_for_cache(&state);
        assert_eq!(snapshot.messages["a"].len(), 100);
        assert_eq!(snapshot.messages["a"][99].guid, "m149");
        assert!(!snapshot.messages.contains_key("b"));
    }

    /// `cargo test -p messages-core --release -- --ignored --nocapture cache_parse_timing`
    #[test]
    #[ignore]
    fn cache_parse_timing() {
        let chats: Vec<Arc<Chat>> = (0..40)
            .map(|n| Arc::new(Chat { guid: format!("iMessage;-;+1555000{n:04}"), identifier: format!("+1555000{n:04}"), last_activity: n, ..Chat::default() }))
            .collect();
        let mut messages = HashMap::new();
        for chat in &chats {
            let rows = (0..100)
                .map(|n| {
                    let mut row = message(&chat.guid, &format!("{}-{n}", chat.guid), n);
                    row.text = "A reasonably ordinary message, about the length people actually type 👍".into();
                    row.service = Service::IMessage;
                    if n % 10 == 0 {
                        row.attachments.push(Attachment {
                            guid: format!("att-{n}"),
                            name: "IMG_0001.HEIC".into(),
                            mime: "image/jpeg".into(),
                            bytes: 1_000_000,
                            width: Some(1200),
                            height: Some(800),
                            measured: true,
                            is_sticker: false,
                            local_path: Some(PathBuf::from("/home/me/.cache/messages/attachments/0123456789abcdef.jpg")),
                            hidden: false,
                            duration_ms: None,
                        });
                    }
                    Arc::new(row)
                })
                .collect();
            messages.insert(chat.guid.clone(), rows);
        }
        let state = CachedState { version: 1, saved_at: 0, selected_chat: None, chats, messages, contacts: Arc::new(vec![]) };
        let bytes = serde_json::to_vec(&state).unwrap();
        let started = std::time::Instant::now();
        let runs = 20;
        for _ in 0..runs {
            let parsed: CachedState = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(parsed.chats.len(), 40);
        }
        println!("state.json {} KiB, parse {:?} per load", bytes.len() / 1024, started.elapsed() / runs);
    }
}
