//! Client for `@messages/mac-agent`, which serves decrypted Find My
//! locations and the prefs shared between clients.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::findmy::{DeviceLocation, FriendLocation, MapSnapshotRequest};
use crate::gifs::GifFavorite;
use crate::model::Millis;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub url: String,
    pub token: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentKeys {
    pub friends: bool,
    pub fmf: bool,
    pub fmip: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHealth {
    pub ok: bool,
    pub keys: AgentKeys,
    #[serde(default)]
    pub prefs: Option<bool>,
    /// Whether FindMy.app is running on the Mac. Nothing refreshes its caches while it is not.
    #[serde(default)]
    pub find_my_open: Option<bool>,
}

/// One push from `/findmy/stream`. A half the agent could not read is None, so a client keeps what it had.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindMySnapshot {
    pub friends: Option<Vec<FriendLocation>>,
    pub devices: Option<Vec<DeviceLocation>>,
    #[serde(default, deserialize_with = "crate::findmy::millis")]
    pub updated_at: Millis,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendsResponse {
    pub friends: Vec<FriendLocation>,
    #[serde(default, deserialize_with = "crate::findmy::millis")]
    pub updated_at: Millis,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicesResponse {
    pub devices: Vec<DeviceLocation>,
    #[serde(default, deserialize_with = "crate::findmy::millis")]
    pub updated_at: Millis,
}

/// Per-chat client state, kept in config.json `chats` and on the Mac agent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPrefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_receipts: Option<bool>,
    /// The composer text for this chat, synced between clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<String>,
    /// When a client last changed this entry; the newer entry wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<Millis>,
    /// Moved only by a pin or unpin, so a draft or a mute cannot outrank the Mac's list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<Millis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac_pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac_pinned_at: Option<Millis>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedPrefs {
    #[serde(default)]
    pub chats: HashMap<String, ChatPrefs>,
    /// Absent from agents that predate GIF favorites.
    #[serde(default)]
    pub gifs: Option<HashMap<String, GifFavorite>>,
    /// Pinned in Messages.app: an address for a one-to-one chat, a group id or chat guid for a group.
    #[serde(default)]
    pub mac_pinned: Vec<String>,
    #[serde(default, deserialize_with = "crate::findmy::opt_millis")]
    pub mac_pinned_at: Option<Millis>,
}

/// A Mac pin is overridden by a pin this client changed after the Mac's list was last edited, and by nothing else.
pub fn is_pinned(prefs: Option<&ChatPrefs>) -> bool {
    let Some(prefs) = prefs else {
        return false;
    };
    if prefs.mac_pinned == Some(true) && prefs.mac_pinned_at.unwrap_or(0) > prefs.pinned_at.unwrap_or(0) {
        return true;
    }
    prefs.pinned.unwrap_or(false)
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// The agent predates `/findmy/stream` (404); fall back to polling instead of retrying.
    #[error("agent: this Mac agent does not serve /findmy/stream")]
    StreamUnsupported,
    /// The agent predates `/findmy/snapshot` (404).
    #[error("agent: this Mac agent does not serve /findmy/snapshot")]
    SnapshotUnsupported,
    #[error("agent: {0}")]
    Http(String),
}

const TIMEOUT: Duration = Duration::from_secs(10);
/// A first render compiles the MapKit helper on the Mac, which takes a few seconds on its own.
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(30);
const SNAPSHOT_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// All requests but the stream and snapshots time out after 10 s. Auth is `Authorization: Bearer <token>`.
#[derive(Clone)]
pub struct MacAgentClient {
    config: AgentConfig,
    http: reqwest::Client,
}

impl MacAgentClient {
    pub fn new(config: AgentConfig, http: reqwest::Client) -> Self {
        Self { config, http }
    }

    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    fn endpoint(&self, pathname: &str) -> String {
        url::Url::parse(&self.config.url).and_then(|base| base.join(pathname)).map(|u| u.to_string()).unwrap_or_else(|_| format!("{}{pathname}", self.config.url))
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, pathname: &str, authed: bool) -> Result<T, AgentError> {
        let url = self.endpoint(pathname);
        let mut request = self.http.get(&url).timeout(TIMEOUT);
        if authed {
            request = request.bearer_auth(&self.config.token);
        }
        let response = request.send().await.map_err(|error| AgentError::Http(error.to_string()))?;
        if !response.status().is_success() {
            return Err(AgentError::Http(format!("agent: {url} returned {}", response.status().as_u16())));
        }
        response.json::<T>().await.map_err(|error| AgentError::Http(error.to_string()))
    }

    /// `GET /health`, unauthenticated.
    pub async fn health(&self) -> Result<AgentHealth, AgentError> {
        self.get_json("/health", false).await
    }

    /// `GET /findmy/friends`.
    pub async fn friends(&self) -> Result<FriendsResponse, AgentError> {
        self.get_json("/findmy/friends", true).await
    }

    /// `GET /findmy/devices`.
    pub async fn devices(&self) -> Result<DevicesResponse, AgentError> {
        self.get_json("/findmy/devices", true).await
    }

    /// `GET /findmy/stream`, server-sent events. Sends each snapshot to `tx` until
    /// the agent closes the stream or `tx` is dropped. No timeout: the point of
    /// the connection is to stay open.
    pub async fn stream_findmy(&self, tx: mpsc::Sender<FindMySnapshot>) -> Result<(), AgentError> {
        use futures_util::StreamExt;

        let url = self.endpoint("/findmy/stream");
        let response = self
            .http
            .get(&url)
            .bearer_auth(&self.config.token)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .send()
            .await
            .map_err(|error| AgentError::Http(error.to_string()))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(AgentError::StreamUnsupported);
        }
        if !response.status().is_success() {
            return Err(AgentError::Http(format!("agent: /findmy/stream returned {}", response.status().as_u16())));
        }

        let mut stream = response.bytes_stream();
        let mut buffer: Vec<u8> = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| AgentError::Http(error.to_string()))?;
            buffer.extend_from_slice(&chunk);
            // An SSE message ends at a blank line; a `:` line is the agent's keepalive and carries no data.
            while let Some(end) = buffer.windows(2).position(|w| w == b"\n\n") {
                let message: Vec<u8> = buffer.drain(..end + 2).collect();
                let message = String::from_utf8_lossy(&message[..message.len() - 2]).into_owned();
                let data: String = message.lines().filter(|line| line.starts_with("data:")).map(|line| line["data:".len()..].trim()).collect();
                if data.is_empty() {
                    continue;
                }
                match serde_json::from_str::<FindMySnapshot>(&data) {
                    Ok(snapshot) => {
                        if tx.send(snapshot).await.is_err() {
                            return Ok(());
                        }
                    }
                    Err(error) => tracing::error!("agent: unreadable Find My snapshot: {error}"),
                }
            }
        }
        Ok(())
    }

    /// `GET /findmy/snapshot`, saved as `dir/<rounded request>.png` and reused for a day.
    pub async fn find_my_snapshot(&self, request: &MapSnapshotRequest, dir: &Path) -> Result<PathBuf, AgentError> {
        let target = dir.join(request.file_name());
        let fresh = tokio::fs::metadata(&target).await.ok().and_then(|meta| meta.modified().ok()).is_some_and(|modified| SystemTime::now().duration_since(modified).is_ok_and(|age| age < SNAPSHOT_MAX_AGE));
        if fresh {
            return Ok(target);
        }
        let url = format!("{}?{}", self.endpoint("/findmy/snapshot"), request.query());
        let response = self.http.get(&url).timeout(SNAPSHOT_TIMEOUT).bearer_auth(&self.config.token).send().await.map_err(|error| AgentError::Http(error.to_string()))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(AgentError::SnapshotUnsupported);
        }
        if !response.status().is_success() {
            return Err(AgentError::Http(format!("agent: /findmy/snapshot returned {}", response.status().as_u16())));
        }
        let bytes = response.bytes().await.map_err(|error| AgentError::Http(error.to_string()))?;
        if !bytes.starts_with(b"\x89PNG") {
            return Err(AgentError::Http("agent: /findmy/snapshot did not answer a PNG".to_owned()));
        }
        let io = |error: std::io::Error| AgentError::Http(format!("agent: saving the snapshot: {error}"));
        tokio::fs::create_dir_all(dir).await.map_err(io)?;
        let partial = target.with_extension(format!("png.{}.part", std::process::id()));
        tokio::fs::write(&partial, &bytes).await.map_err(io)?;
        tokio::fs::rename(&partial, &target).await.map_err(io)?;
        Ok(target)
    }

    /// `PUT /prefs` with `{chats, gifs}`; answers the merged set plus the Mac's own pins.
    pub async fn sync_prefs(&self, chats: &HashMap<String, ChatPrefs>, gifs: &HashMap<String, GifFavorite>) -> Result<SharedPrefs, AgentError> {
        #[derive(Serialize)]
        struct Body<'a> {
            chats: &'a HashMap<String, ChatPrefs>,
            gifs: &'a HashMap<String, GifFavorite>,
        }
        let url = self.endpoint("/prefs");
        let response = self.http.put(&url).timeout(TIMEOUT).bearer_auth(&self.config.token).json(&Body { chats, gifs }).send().await.map_err(|error| AgentError::Http(error.to_string()))?;
        if !response.status().is_success() {
            return Err(AgentError::Http(format!("agent: {url} returned {}", response.status().as_u16())));
        }
        response.json::<SharedPrefs>().await.map_err(|error| AgentError::Http(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::Mutex as AsyncMutex;

    fn snapshot_json(latitude: f64) -> String {
        serde_json::json!({
            "friends": [{ "id": "f1", "addresses": ["+34600111222"], "latitude": latitude, "longitude": -15.4, "timestamp": 1, "isSharing": true }],
            "devices": null,
            "updatedAt": 2,
        })
        .to_string()
    }

    /// Serves one connection on localhost, replying with `status` and streaming
    /// `chunks` as separate chunked-encoding writes (mirroring the browser
    /// test's split reads), and hands the raw request bytes back through `seen`.
    async fn sse_server(status: u16, chunks: Vec<String>, seen: Arc<AsyncMutex<Vec<u8>>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 8192];
            if let Ok(n) = socket.read(&mut buf).await {
                seen.lock().await.extend_from_slice(&buf[..n]);
            }
            let status_text = match status {
                404 => "Not Found",
                503 => "Service Unavailable",
                _ => "OK",
            };
            let header = format!("HTTP/1.1 {status} {status_text}\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n");
            let _ = socket.write_all(header.as_bytes()).await;
            for chunk in chunks {
                let framed = format!("{:x}\r\n{chunk}\r\n", chunk.len());
                let _ = socket.write_all(framed.as_bytes()).await;
            }
            let _ = socket.write_all(b"0\r\n\r\n").await;
        });
        format!("http://{addr}")
    }

    fn client(base: String) -> MacAgentClient {
        MacAgentClient::new(AgentConfig { url: base, token: "tok".to_owned() }, reqwest::Client::new())
    }

    async fn collect(chunks: Vec<String>) -> Vec<FindMySnapshot> {
        let seen = Arc::new(AsyncMutex::new(Vec::new()));
        let base = sse_server(200, chunks, seen).await;
        let (tx, mut rx) = mpsc::channel(8);
        client(base).stream_findmy(tx).await.unwrap();
        let mut out = Vec::new();
        while let Ok(snapshot) = rx.try_recv() {
            out.push(snapshot);
        }
        out
    }

    #[tokio::test]
    async fn sends_the_token_and_asks_for_an_event_stream() {
        let seen = Arc::new(AsyncMutex::new(Vec::new()));
        let base = sse_server(200, vec![], seen.clone()).await;
        let (tx, _rx) = mpsc::channel(8);
        client(base).stream_findmy(tx).await.unwrap();
        let request = String::from_utf8_lossy(&seen.lock().await).to_lowercase();
        assert!(request.starts_with("get /findmy/stream"));
        assert!(request.contains("authorization: bearer tok"));
        assert!(request.contains("accept: text/event-stream"));
    }

    #[tokio::test]
    async fn reassembles_a_message_split_across_reads() {
        let message = format!("data: {}\n\n", snapshot_json(28.1));
        let seen = vec![message[..30].to_owned(), message[30..60].to_owned(), message[60..].to_owned()];
        let snapshots = collect(seen).await;
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].friends.as_ref().unwrap()[0].latitude, 28.1);
    }

    #[tokio::test]
    async fn reads_two_messages_out_of_one_read_and_skips_the_keepalive_between_them() {
        let combined = format!("data: {}\n\n: keepalive\n\ndata: {}\n\n", snapshot_json(28.1), snapshot_json(28.2));
        let snapshots = collect(vec![combined]).await;
        let lats: Vec<f64> = snapshots.iter().map(|s| s.friends.as_ref().unwrap()[0].latitude).collect();
        assert_eq!(lats, vec![28.1, 28.2]);
    }

    #[tokio::test]
    async fn drops_a_message_it_cannot_parse_and_keeps_reading() {
        let combined = format!("data: {{\"friends\":\n\ndata: {}\n\n", snapshot_json(28.3));
        let snapshots = collect(vec![combined]).await;
        let lats: Vec<f64> = snapshots.iter().map(|s| s.friends.as_ref().unwrap()[0].latitude).collect();
        assert_eq!(lats, vec![28.3]);
    }

    #[tokio::test]
    async fn tells_an_agent_without_the_endpoint_apart_from_one_that_is_down() {
        let seen = Arc::new(AsyncMutex::new(Vec::new()));
        let base = sse_server(404, vec![], seen).await;
        let (tx, _rx) = mpsc::channel(8);
        let error = client(base).stream_findmy(tx).await.unwrap_err();
        assert!(matches!(error, AgentError::StreamUnsupported));

        let seen = Arc::new(AsyncMutex::new(Vec::new()));
        let base = sse_server(503, vec![], seen).await;
        let (tx, _rx) = mpsc::channel(8);
        let error = client(base).stream_findmy(tx).await.unwrap_err();
        assert!(error.to_string().contains("503"));
    }

    /// Answers one connection with `status` and `body`, handing the request line back through `seen`.
    async fn png_server(status: u16, body: &'static [u8], seen: Arc<AsyncMutex<Vec<u8>>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 8192];
            if let Ok(n) = socket.read(&mut buf).await {
                seen.lock().await.extend_from_slice(&buf[..n]);
            }
            let header = format!("HTTP/1.1 {status} X\r\ncontent-type: image/png\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len());
            let _ = socket.write_all(header.as_bytes()).await;
            let _ = socket.write_all(body).await;
        });
        format!("http://{addr}")
    }

    fn snapshot_request() -> MapSnapshotRequest {
        MapSnapshotRequest { latitude: 37.795512, longitude: -122.39371, width: 248, height: 160, scale: 2, dark: true, span: 1500 }
    }

    #[tokio::test]
    async fn saves_a_snapshot_under_its_rounded_request_and_reuses_it() {
        let dir = crate::testing::temp_dir("snapshot");
        let seen = Arc::new(AsyncMutex::new(Vec::new()));
        let base = png_server(200, b"\x89PNG fake", seen.clone()).await;
        let agent = client(base);
        let path = agent.find_my_snapshot(&snapshot_request(), dir.as_path()).await.unwrap();
        assert_eq!(path, dir.join(snapshot_request().file_name()));
        assert_eq!(std::fs::read(&path).unwrap(), b"\x89PNG fake");
        let request = String::from_utf8_lossy(&seen.lock().await).to_lowercase();
        assert!(request.starts_with("get /findmy/snapshot?lat=37.79551&lon=-122.39371&w=248&h=160&scale=2&dark=1&span=1500 "));
        assert!(request.contains("authorization: bearer tok"));
        // The server answers one connection only, so a second fetch has to come from disk.
        assert_eq!(agent.find_my_snapshot(&snapshot_request(), dir.as_path()).await.unwrap(), path);
    }

    #[tokio::test]
    async fn tells_an_agent_without_snapshots_apart_from_a_failed_render() {
        let dir = crate::testing::temp_dir("snapshot");
        let base = png_server(404, b"{}", Arc::new(AsyncMutex::new(Vec::new()))).await;
        assert!(matches!(client(base).find_my_snapshot(&snapshot_request(), dir.as_path()).await, Err(AgentError::SnapshotUnsupported)));
        let base = png_server(503, b"{}", Arc::new(AsyncMutex::new(Vec::new()))).await;
        let error = client(base).find_my_snapshot(&snapshot_request(), dir.as_path()).await.unwrap_err();
        assert!(error.to_string().contains("503"));
        assert!(std::fs::read_dir(dir.as_path()).unwrap().next().is_none());
    }

    #[test]
    fn reads_an_agent_answer_that_predates_mac_pins_and_gifs() {
        let shared: SharedPrefs = serde_json::from_str(r#"{"chats":{"c":{"pinned":true,"updatedAt":5}}}"#).unwrap();
        assert_eq!(shared.chats["c"].updated_at, Some(5));
        assert!(shared.mac_pinned.is_empty());
        assert_eq!(shared.mac_pinned_at, None);
        assert_eq!(shared.gifs, None);
    }

    #[test]
    fn no_prefs_means_not_pinned() {
        assert!(!is_pinned(None));
    }

    #[test]
    fn a_plain_pin_is_pinned() {
        assert!(is_pinned(Some(&ChatPrefs { pinned: Some(true), ..Default::default() })));
    }

    #[test]
    fn a_mac_pin_newer_than_the_local_unpin_wins() {
        let prefs = ChatPrefs { pinned: Some(false), pinned_at: Some(100), mac_pinned: Some(true), mac_pinned_at: Some(200), ..Default::default() };
        assert!(is_pinned(Some(&prefs)));
    }

    #[test]
    fn a_local_unpin_after_the_mac_pin_wins() {
        let prefs = ChatPrefs { pinned: Some(false), pinned_at: Some(200), mac_pinned: Some(true), mac_pinned_at: Some(100), ..Default::default() };
        assert!(!is_pinned(Some(&prefs)));
    }
}
