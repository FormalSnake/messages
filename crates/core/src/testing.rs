//! A scripted HTTP/1.1 server on localhost for tests of the HTTP clients.

use std::sync::Arc;

use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    /// Path and query as sent.
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }

    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("")
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(status: u16, value: serde_json::Value) -> Self {
        Response { status, content_type: "application/json", body: value.to_string().into_bytes() }
    }

    pub fn bytes(status: u16, body: &[u8]) -> Self {
        Response { status, content_type: "application/octet-stream", body: body.to_vec() }
    }
}

pub struct Server {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Request>>>,
}

/// Answers every request with `answer`, one connection per request, and records what was sent.
pub async fn serve(answer: impl Fn(&Request) -> Response + Send + Sync + 'static) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let requests: Arc<Mutex<Vec<Request>>> = Arc::default();
    let log = requests.clone();
    let answer = Arc::new(answer);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let (log, answer) = (log.clone(), answer.clone());
            tokio::spawn(async move {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 8192];
                let header_end = loop {
                    let Ok(read) = socket.read(&mut chunk).await else { return };
                    if read == 0 {
                        return;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                    if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                        break end + 4;
                    }
                };
                let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
                let mut lines = head.lines();
                let mut first = lines.next().unwrap_or("").split_whitespace();
                let method = first.next().unwrap_or("").to_owned();
                let target = first.next().unwrap_or("/").to_owned();
                let headers: Vec<(String, String)> =
                    lines.filter_map(|line| line.split_once(':')).map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned())).collect();
                let length = headers.iter().find(|(key, _)| key.eq_ignore_ascii_case("content-length")).and_then(|(_, value)| value.parse().ok()).unwrap_or(0);
                while buffer.len() < header_end + length {
                    let Ok(read) = socket.read(&mut chunk).await else { return };
                    if read == 0 {
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                }
                let request = Request { method, target, headers, body: buffer[header_end..].to_vec() };
                let response = answer(&request);
                log.lock().push(request);
                let head = format!(
                    "HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.status,
                    response.content_type,
                    response.body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&response.body).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    Server { url, requests }
}

/// A fresh directory under the system temp dir.
pub fn temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("messages-{label}-{}", fastrand::u64(..)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
