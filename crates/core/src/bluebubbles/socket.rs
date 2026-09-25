//! A socket.io v4 client (Engine.IO protocol 4, websocket transport only), just
//! enough for BlueBubbles: connect to the default namespace, receive `42` events,
//! answer pings, emit with an ack. Built on tokio-tungstenite with rustls because
//! rust_socketio hard-depends on native-tls (OpenSSL on Linux). Binary packets
//! are not used by BlueBubbles and are ignored.
//!
//! Reconnects on its own like socket.io-client does (1 s doubling to 5 s, with
//! jitter, forever) until `close`; the transport maps each transition to a
//! `TransportEvent::Connection`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message as Frame;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::map::parse_json;

#[derive(Clone, Debug, PartialEq)]
pub enum SocketEvent {
    /// The namespace CONNECT was acknowledged after a reconnect. The first connect is `SocketIo::connect` returning.
    Connected,
    /// A reconnect attempt failed.
    ConnectError(String),
    Disconnected(String),
    /// A `42["name", ...args]` packet.
    Event { name: String, args: Vec<Value> },
}

#[derive(Debug, thiserror::Error)]
pub enum SocketError {
    #[error("socket: not connected")]
    NotConnected,
    #[error("socket: ack timed out")]
    AckTimeout,
    #[error("socket: {0}")]
    Io(String),
}

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// socket.io-client's defaults.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const RECONNECT_MIN_MS: f64 = 1000.0;
const RECONNECT_MAX_MS: f64 = 5000.0;
const RECONNECT_JITTER: f64 = 0.5;

struct Shared {
    connected: AtomicBool,
    closed: watch::Sender<bool>,
    outgoing: Mutex<Option<mpsc::UnboundedSender<String>>>,
    acks: Mutex<HashMap<u64, oneshot::Sender<Vec<Value>>>>,
    next_ack: AtomicU64,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Shared {
    fn send(&self, text: String) -> Result<(), SocketError> {
        if !self.connected.load(Ordering::Acquire) {
            return Err(SocketError::NotConnected);
        }
        let sender = self.outgoing.lock().clone().ok_or(SocketError::NotConnected)?;
        sender.send(text).map_err(|_| SocketError::NotConnected)
    }
}

/// Dropping the last handle stops the reconnect loop, which only holds `Shared`.
struct Owner(Arc<Shared>);

impl Drop for Owner {
    fn drop(&mut self) {
        self.0.closed.send_replace(true);
    }
}

/// Cheap to clone; clones share the connection.
#[derive(Clone)]
pub struct SocketIo {
    owner: Arc<Owner>,
}

/// `ws(s)://<host>/socket.io/?EIO=4&transport=websocket&password=<password>` from an http(s) server URL.
pub(crate) fn socket_url(server_url: &str, password: &str) -> Result<url::Url, SocketError> {
    let mut url = super::client::server_base(server_url).map_err(SocketError::Io)?;
    let scheme = if url.scheme() == "https" || url.scheme() == "wss" { "wss" } else { "ws" };
    url.set_scheme(scheme).map_err(|_| SocketError::Io("invalid server URL".into()))?;
    url.set_path("/socket.io/");
    url.query_pairs_mut().clear().append_pair("EIO", "4").append_pair("transport", "websocket").append_pair("password", password);
    Ok(url)
}

impl SocketIo {
    /// `ws(s)://<host>/socket.io/?EIO=4&transport=websocket&password=<password>` derived from the server URL.
    /// Returns once the first connect succeeded or failed; the receiver sees every later transition.
    /// A failed first connect does not keep retrying: the caller retries by connecting again.
    pub async fn connect(server_url: &str, password: &str) -> Result<(Self, mpsc::UnboundedReceiver<SocketEvent>), SocketError> {
        let url = socket_url(server_url, password)?;
        let shared = Arc::new(Shared {
            connected: AtomicBool::new(false),
            closed: watch::channel(false).0,
            outgoing: Mutex::new(None),
            acks: Mutex::new(HashMap::new()),
            next_ack: AtomicU64::new(0),
            task: Mutex::new(None),
        });
        let (events, receiver) = mpsc::unbounded_channel();
        let (first_tx, first_rx) = oneshot::channel();
        let task = tokio::spawn(run(shared.clone(), url.to_string(), events, first_tx));
        *shared.task.lock() = Some(task);
        let socket = SocketIo { owner: Arc::new(Owner(shared)) };
        match first_rx.await {
            Ok(Ok(())) => Ok((socket, receiver)),
            Ok(Err(message)) => Err(SocketError::Io(message)),
            Err(_) => Err(SocketError::Io("socket task ended".into())),
        }
    }

    fn shared(&self) -> &Shared {
        &self.owner.0
    }

    pub fn is_connected(&self) -> bool {
        self.shared().connected.load(Ordering::Acquire)
    }

    pub async fn emit(&self, event: &str, data: Value) -> Result<(), SocketError> {
        self.shared().send(format!("42{}", Value::Array(vec![Value::from(event), data])))
    }

    /// `42<id>[event, data]`, resolved by the matching `43<id>[...]`.
    pub async fn emit_with_ack(&self, event: &str, data: Value, timeout: Duration) -> Result<Vec<Value>, SocketError> {
        let shared = self.shared();
        let id = shared.next_ack.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        shared.acks.lock().insert(id, sender);
        if let Err(err) = shared.send(format!("42{id}{}", Value::Array(vec![Value::from(event), data]))) {
            shared.acks.lock().remove(&id);
            return Err(err);
        }
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(args)) => Ok(args),
            Ok(Err(_)) => Err(SocketError::Io("disconnected before the ack".into())),
            Err(_) => {
                shared.acks.lock().remove(&id);
                Err(SocketError::AckTimeout)
            }
        }
    }

    /// Stops reconnecting and closes the websocket.
    pub async fn close(&self) {
        let shared = self.shared();
        shared.closed.send_replace(true);
        let task = shared.task.lock().take();
        if let Some(mut task) = task {
            if tokio::time::timeout(Duration::from_secs(2), &mut task).await.is_err() {
                task.abort();
            }
        }
    }
}

/// socket.io-client's Backoff: 1 s doubling to 5 s, each step moved up or down by up to half.
fn reconnect_delay(attempt: u32) -> Duration {
    let ms = RECONNECT_MIN_MS * 2f64.powi(attempt.min(16) as i32);
    let random = fastrand::f64();
    let deviation = (random * RECONNECT_JITTER * ms).floor();
    let ms = if ((random * 10.0).floor() as u32) & 1 == 0 { ms - deviation } else { ms + deviation };
    Duration::from_millis(ms.min(RECONNECT_MAX_MS) as u64)
}

async fn wait_closed(closed: &mut watch::Receiver<bool>) {
    let _ = closed.wait_for(|closed| *closed).await;
}

async fn run(shared: Arc<Shared>, url: String, events: mpsc::UnboundedSender<SocketEvent>, first: oneshot::Sender<Result<(), String>>) {
    let mut first = Some(first);
    let mut closed = shared.closed.subscribe();
    let mut attempt = 0;
    loop {
        let opened = tokio::select! {
            opened = tokio::time::timeout(CONNECT_TIMEOUT, open(&url)) => opened.unwrap_or_else(|_| Err("timeout".into())),
            _ = wait_closed(&mut closed) => return,
        };
        match opened {
            Ok((ws, ping_window)) => {
                attempt = 0;
                match first.take() {
                    Some(first) => {
                        let _ = first.send(Ok(()));
                    }
                    None => {
                        let _ = events.send(SocketEvent::Connected);
                    }
                }
                let reason = serve(&shared, ws, ping_window, &events).await;
                if *shared.closed.borrow() {
                    return;
                }
                let _ = events.send(SocketEvent::Disconnected(reason));
            }
            Err(message) => {
                if let Some(first) = first.take() {
                    let _ = first.send(Err(message));
                    return;
                }
                let _ = events.send(SocketEvent::ConnectError(message));
            }
        }
        let delay = reconnect_delay(attempt);
        attempt = attempt.saturating_add(1);
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = wait_closed(&mut closed) => return,
        }
    }
}

async fn next_text(ws: &mut Ws) -> Result<String, String> {
    loop {
        match ws.next().await {
            None => return Err("transport close".into()),
            Some(Err(err)) => return Err(err.to_string()),
            Some(Ok(Frame::Text(text))) => return Ok(text.to_string()),
            Some(Ok(Frame::Close(_))) => return Err("transport close".into()),
            Some(Ok(_)) => {}
        }
    }
}

/// Websocket handshake, Engine.IO OPEN, then the namespace CONNECT. Returns the ping window.
async fn open(url: &str) -> Result<(Ws, Duration), String> {
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.map_err(|err| err.to_string())?;
    let handshake = next_text(&mut ws).await?;
    let Some(json) = handshake.strip_prefix('0') else {
        return Err(format!("unexpected handshake {handshake:?}"));
    };
    let handshake: Value = serde_json::from_str(json).map_err(|err| err.to_string())?;
    let millis = |key: &str, default: u64| handshake.get(key).and_then(Value::as_u64).unwrap_or(default);
    let ping_window = Duration::from_millis(millis("pingInterval", 25_000) + millis("pingTimeout", 20_000));
    ws.send(Frame::text("40")).await.map_err(|err| err.to_string())?;
    loop {
        let text = next_text(&mut ws).await?;
        if let Some(ping) = text.strip_prefix('2') {
            ws.send(Frame::text(format!("3{ping}"))).await.map_err(|err| err.to_string())?;
        } else if text.starts_with("40") {
            return Ok((ws, ping_window));
        } else if let Some(error) = text.strip_prefix("44") {
            return Err(connect_error_message(error));
        }
    }
}

fn connect_error_message(payload: &str) -> String {
    let (_, json) = split_packet(payload);
    let value: Option<Value> = serde_json::from_str(json).ok();
    value
        .as_ref()
        .and_then(|value| value.get("message").and_then(Value::as_str).or_else(|| value.as_str()))
        .unwrap_or("connect error")
        .to_owned()
}

/// Namespace and ack id off the front of a socket.io packet body: `/nsp,12[...]` gives `(Some(12), "[...]")`.
fn split_packet(payload: &str) -> (Option<u64>, &str) {
    let payload = match payload.strip_prefix('/') {
        Some(_) => payload.split_once(',').map(|(_, rest)| rest).unwrap_or(""),
        None => payload,
    };
    let digits = payload.bytes().take_while(u8::is_ascii_digit).count();
    (payload[..digits].parse().ok(), &payload[digits..])
}

enum Action {
    Nothing,
    Reply(String),
    Close(String),
}

fn handle_text(text: &str, shared: &Shared, events: &mpsc::UnboundedSender<SocketEvent>) -> Action {
    let mut chars = text.chars();
    match chars.next() {
        Some('2') => Action::Reply(format!("3{}", chars.as_str())),
        Some('1') => Action::Close("transport close".into()),
        Some('4') => {
            let body = chars.as_str();
            let (kind, rest) = body.split_at(body.len().min(1));
            match kind {
                "1" => Action::Close("io server disconnect".into()),
                "4" => Action::Close(connect_error_message(rest)),
                "2" => {
                    let (id, json) = split_packet(rest);
                    let Ok(Value::Array(mut items)) = parse_json::<Value>(json.as_bytes()) else {
                        return Action::Nothing;
                    };
                    if !items.is_empty() {
                        if let Value::String(name) = items.remove(0) {
                            let _ = events.send(SocketEvent::Event { name, args: items });
                        }
                    }
                    id.map_or(Action::Nothing, |id| Action::Reply(format!("43{id}[]")))
                }
                "3" => {
                    let (id, json) = split_packet(rest);
                    let args = match parse_json::<Value>(json.as_bytes()) {
                        Ok(Value::Array(items)) => items,
                        _ => Vec::new(),
                    };
                    if let Some(sender) = id.and_then(|id| shared.acks.lock().remove(&id)) {
                        let _ = sender.send(args);
                    }
                    Action::Nothing
                }
                _ => Action::Nothing,
            }
        }
        _ => Action::Nothing,
    }
}

/// Runs one live connection until it drops or `close` is called, and returns why it ended.
async fn serve(shared: &Shared, ws: Ws, ping_window: Duration, events: &mpsc::UnboundedSender<SocketEvent>) -> String {
    let (mut sink, mut stream) = ws.split();
    let (outgoing, mut queue) = mpsc::unbounded_channel::<String>();
    *shared.outgoing.lock() = Some(outgoing);
    shared.connected.store(true, Ordering::Release);
    let mut closed = shared.closed.subscribe();
    let mut deadline = Instant::now() + ping_window;

    let reason = loop {
        tokio::select! {
            _ = wait_closed(&mut closed) => {
                let _ = sink.send(Frame::Close(None)).await;
                break "io client disconnect".to_owned();
            }
            _ = tokio::time::sleep_until(deadline) => break "ping timeout".to_owned(),
            Some(text) = queue.recv() => {
                if let Err(err) = sink.send(Frame::text(text)).await {
                    break err.to_string();
                }
            }
            frame = stream.next() => {
                deadline = Instant::now() + ping_window;
                let text = match frame {
                    None | Some(Ok(Frame::Close(_))) => break "transport close".to_owned(),
                    Some(Err(err)) => break err.to_string(),
                    Some(Ok(Frame::Text(text))) => text,
                    Some(Ok(_)) => continue,
                };
                match handle_text(&text, shared, events) {
                    Action::Nothing => {}
                    Action::Reply(reply) => {
                        if let Err(err) = sink.send(Frame::text(reply)).await {
                            break err.to_string();
                        }
                    }
                    Action::Close(reason) => break reason,
                }
            }
        }
    };

    shared.connected.store(false, Ordering::Release);
    *shared.outgoing.lock() = None;
    // Dropping the senders fails every ack still waiting on this connection.
    shared.acks.lock().clear();
    reason
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn builds_the_websocket_url_from_the_server_url() {
        assert_eq!(
            socket_url("http://mac.local:1234", "p w").unwrap().as_str(),
            "ws://mac.local:1234/socket.io/?EIO=4&transport=websocket&password=p+w"
        );
        assert!(socket_url("https://example.com/", "x").unwrap().as_str().starts_with("wss://example.com/socket.io/?"));
    }

    #[test]
    fn splits_namespace_and_ack_id() {
        assert_eq!(split_packet("12[\"a\"]"), (Some(12), "[\"a\"]"));
        assert_eq!(split_packet("/admin,3[1]"), (Some(3), "[1]"));
        assert_eq!(split_packet("[1]"), (None, "[1]"));
    }

    #[test]
    fn backoff_stays_between_half_a_second_and_five() {
        for attempt in 0..10 {
            let delay = reconnect_delay(attempt);
            assert!(delay >= Duration::from_millis(500) && delay <= Duration::from_secs(5), "{delay:?}");
        }
    }

    async fn server_text(ws: &mut WebSocketStream<TcpStream>) -> String {
        loop {
            if let Frame::Text(text) = ws.next().await.unwrap().unwrap() {
                return text.to_string();
            }
        }
    }

    async fn accept(listener: &TcpListener) -> WebSocketStream<TcpStream> {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        ws.send(Frame::text(r#"0{"sid":"s","upgrades":[],"pingInterval":25000,"pingTimeout":20000,"maxPayload":1000000}"#)).await.unwrap();
        assert_eq!(server_text(&mut ws).await, "40");
        ws.send(Frame::text(r#"40{"sid":"n"}"#)).await.unwrap();
        ws
    }

    #[tokio::test]
    async fn talks_socket_io_to_a_local_server_and_reconnects() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let mut ws = accept(&listener).await;
            ws.send(Frame::text("2")).await.unwrap();
            assert_eq!(server_text(&mut ws).await, "3");
            ws.send(Frame::text(r#"42["new-message",{"guid":"g","text":"hi \ud800"}]"#)).await.unwrap();
            let emitted = server_text(&mut ws).await;
            assert_eq!(emitted, r#"420["stopped-typing",{"chatGuid":"c"}]"#);
            ws.send(Frame::text(r#"430[{"ok":true}]"#)).await.unwrap();
            ws.send(Frame::text("41")).await.unwrap();
            let mut again = accept(&listener).await;
            again.send(Frame::text(r#"42["typing-indicator",{"display":true,"guid":"c"}]"#)).await.unwrap();
            let _ = again.next().await;
        });

        let (socket, mut events) = SocketIo::connect(&url, "pw").await.unwrap();
        assert!(socket.is_connected());
        match events.recv().await.unwrap() {
            SocketEvent::Event { name, args } => {
                assert_eq!(name, "new-message");
                assert_eq!(args[0]["text"], "hi \u{FFFD}");
            }
            other => panic!("{other:?}"),
        }
        let ack = socket.emit_with_ack("stopped-typing", serde_json::json!({ "chatGuid": "c" }), Duration::from_secs(5)).await.unwrap();
        assert_eq!(ack, vec![serde_json::json!({ "ok": true })]);
        assert_eq!(events.recv().await.unwrap(), SocketEvent::Disconnected("io server disconnect".into()));
        assert_eq!(events.recv().await.unwrap(), SocketEvent::Connected);
        assert!(matches!(events.recv().await.unwrap(), SocketEvent::Event { name, .. } if name == "typing-indicator"));
        socket.close().await;
        assert!(!socket.is_connected());
        assert!(matches!(socket.emit("x", Value::Null).await, Err(SocketError::NotConnected)));
        server.abort();
    }

    #[tokio::test]
    async fn a_refused_first_connect_fails_without_retrying() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        assert!(SocketIo::connect(&url, "pw").await.is_err());
    }

    #[tokio::test]
    async fn a_rejected_namespace_connect_reports_the_server_message() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            ws.send(Frame::text(r#"0{"sid":"s","pingInterval":25000,"pingTimeout":20000}"#)).await.unwrap();
            let _ = server_text(&mut ws).await;
            ws.send(Frame::text(r#"44{"message":"Authentication failed"}"#)).await.unwrap();
            let _ = ws.next().await;
        });
        match SocketIo::connect(&url, "wrong").await {
            Err(SocketError::Io(message)) => assert_eq!(message, "Authentication failed"),
            other => panic!("{:?}", other.map(|_| ())),
        }
    }
}
