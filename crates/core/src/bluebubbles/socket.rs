//! A socket.io v4 client (Engine.IO protocol 4, websocket transport only), just
//! enough for BlueBubbles: connect to the default namespace, receive `42` events,
//! answer pings, emit with an ack. Built on tokio-tungstenite with rustls because
//! rust_socketio hard-depends on native-tls (OpenSSL on Linux). Binary packets
//! are not used by BlueBubbles and are ignored.
//!
//! Reconnects on its own like socket.io-client does (1 s doubling to 5 s, with
//! jitter, forever) until `close`; the transport maps each transition to a
//! `TransportEvent::Connection`.

use std::time::Duration;

use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Clone, Debug, PartialEq)]
pub enum SocketEvent {
    /// The namespace CONNECT was acknowledged. Sent again after every reconnect.
    Connected,
    /// The first connect failed, or a later reconnect attempt failed.
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

/// Cheap to clone; clones share the connection.
#[derive(Clone)]
pub struct SocketIo {
    _private: (),
}

impl SocketIo {
    /// `ws(s)://<host>/socket.io/?EIO=4&transport=websocket&password=<password>` derived from the server URL.
    /// Returns once the first connect succeeded or failed; the receiver sees every later transition.
    pub async fn connect(server_url: &str, password: &str) -> Result<(Self, mpsc::UnboundedReceiver<SocketEvent>), SocketError> {
        let _ = (server_url, password);
        unimplemented!()
    }

    pub fn is_connected(&self) -> bool {
        unimplemented!()
    }

    pub async fn emit(&self, event: &str, data: Value) -> Result<(), SocketError> {
        let _ = (event, data);
        unimplemented!()
    }

    /// `42<id>[event, data]`, resolved by the matching `43<id>[...]`.
    pub async fn emit_with_ack(&self, event: &str, data: Value, timeout: Duration) -> Result<Vec<Value>, SocketError> {
        let _ = (event, data, timeout);
        unimplemented!()
    }

    /// Stops reconnecting and closes the websocket.
    pub async fn close(&self) {
        unimplemented!()
    }
}
