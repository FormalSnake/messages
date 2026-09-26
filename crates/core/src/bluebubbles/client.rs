use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use futures_util::future::{BoxFuture, FutureExt, Shared, join_all};
use futures_util::StreamExt;
use parking_lot::{Mutex, RwLock};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;
use serde::de::{DeserializeOwned, IgnoredAny};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Semaphore, mpsc};
use url::Url;

use super::map::{
    ContactIndex, DownloadPlan, MapOptions, RawChat, RawContact, RawHandle, RawMessage, RawServerInfo, download_plan, repair_lone_surrogates,
    to_chat, to_contact, to_handle, to_message, to_server_info,
};
use super::socket::{SocketEvent, SocketIo};
use crate::dedupe::{cached_file, cached_file_blocking, sha1_hex, share_by_content};
use crate::image::{heif_to_png, is_heif, square_thumbnail};
use crate::model::{Chat, Contact, FocusStatus, Message, Millis, ScheduledMessage, ServerInfo, Service, TapbackKind};
use crate::transport::*;

#[derive(Clone, Debug)]
pub struct BlueBubblesOptions {
    pub url: String,
    pub password: String,
    pub attachments_dir: PathBuf,
}

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const ICON_TIMEOUT: Duration = Duration::from_secs(3);
const ICON_SLOTS: usize = 6;
/// Every contact photo is a full RGBA bitmap while it is cut square.
const CONTACT_AVATARS: usize = 4;
const THUMBNAIL_SIDE: u32 = 256;

const MESSAGE_EVENTS: [&str; 3] = ["new-message", "updated-message", "message-send-error"];
const GROUP_EVENTS: [&str; 6] =
    ["group-name-change", "participant-added", "participant-removed", "participant-left", "group-icon-changed", "group-icon-removed"];

/// `encodeURIComponent`.
const COMPONENT: &AsciiSet =
    &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'!').remove(b'~').remove(b'*').remove(b'\'').remove(b'(').remove(b')');

fn enc(segment: &str) -> String {
    utf8_percent_encode(segment, COMPONENT).to_string()
}

/// The server URL as typed, with `http://` assumed when no scheme is given.
pub(crate) fn server_base(url: &str) -> Result<Url, String> {
    let trimmed = url.trim();
    Url::parse(trimmed)
        .ok()
        .filter(|url| url.has_host() && matches!(url.scheme(), "http" | "https" | "ws" | "wss"))
        .or_else(|| Url::parse(&format!("http://{trimmed}")).ok().filter(Url::has_host))
        .ok_or_else(|| format!("Invalid server URL \"{trimmed}\""))
}

/// reqwest puts the URL, password included, in its message; the cause chain says what went wrong.
fn network(err: reqwest::Error) -> TransportError {
    let err = err.without_url();
    let mut message = err.to_string();
    let mut source = std::error::Error::source(&err);
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    TransportError::Network(message)
}

fn io_error(err: std::io::Error) -> TransportError {
    TransportError::Network(err.to_string())
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `JSON.stringify` drops undefined keys; the server treats a null `partIndex` differently from none.
fn object(entries: Vec<(&str, Option<Value>)>) -> Value {
    Value::Object(entries.into_iter().filter_map(|(key, value)| Some((key.to_owned(), value?))).collect::<Map<_, _>>())
}

#[derive(Deserialize)]
struct Envelope<T> {
    #[serde(default)]
    status: Option<Value>,
    #[serde(default)]
    message: Option<Value>,
    data: Option<T>,
    #[serde(default)]
    error: Option<Value>,
    #[serde(default)]
    encrypted: Option<bool>,
}

impl<T> Envelope<T> {
    fn status(&self) -> Option<u16> {
        self.status.as_ref().and_then(Value::as_u64).and_then(|status| u16::try_from(status).ok())
    }

    /// The error a failed or encrypted envelope stands for.
    fn failure(&self, http: reqwest::StatusCode) -> Option<TransportError> {
        let status = self.status().unwrap_or(http.as_u16());
        if self.encrypted == Some(true) {
            // "Encrypt communications" makes the server wrap every payload in its AES envelope.
            return Some(TransportError::Server {
                status,
                message: "Turn off \"Encrypt communications\" in the BlueBubbles server settings".into(),
            });
        }
        if http.is_success() && self.status().is_none_or(|status| status < 400) {
            return None;
        }
        let message = self
            .error
            .as_ref()
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .or_else(|| self.message.as_ref().and_then(Value::as_str))
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Request failed (status {})", http.as_u16()));
        Some(TransportError::Server { status, message })
    }
}

enum Body {
    None,
    Json(Value),
    Form(reqwest::multipart::Form),
}

/// `scheduledFor` comes back as an ISO string, not the epoch number that was sent.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawScheduledMessage {
    id: Value,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    payload: Value,
    #[serde(default)]
    scheduled_for: Value,
}

impl RawScheduledMessage {
    fn id_string(&self) -> String {
        match &self.id {
            Value::String(id) => id.clone(),
            other => other.to_string(),
        }
    }
}

fn to_scheduled_message(raw: &RawScheduledMessage) -> Option<ScheduledMessage> {
    let send_at = match &raw.scheduled_for {
        Value::String(text) => chrono::DateTime::parse_from_rfc3339(text).ok()?.timestamp_millis(),
        Value::Number(number) => number.as_i64().or_else(|| number.as_f64().map(|value| value as i64))?,
        _ => return None,
    };
    let text = |key: &str| raw.payload.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    Some(ScheduledMessage {
        id: raw.id_string(),
        chat_guid: text("chatGuid"),
        text: text("message"),
        send_at,
    })
}

/// packages/server/src/server/api/privateApi/eventHandlers/PrivateApiFaceTimeStatusHandler.ts
#[derive(Deserialize)]
struct RawFaceTimeStatus {
    #[serde(default)]
    uuid: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    handle: Option<RawHandle>,
}

struct Connection {
    socket: SocketIo,
    pump: tokio::task::AbortHandle,
}

type Download = Shared<BoxFuture<'static, TransportResult<PathBuf>>>;

struct Inner {
    base: Url,
    password: String,
    attachments_dir: PathBuf,
    http: reqwest::Client,
    connection: Mutex<Option<Connection>>,
    contacts: RwLock<Arc<ContactIndex>>,
    contacts_seeded: AtomicBool,
    server_info: RwLock<Option<ServerInfo>>,
    listeners: Mutex<Vec<mpsc::UnboundedSender<TransportEvent>>>,
    downloads: Mutex<HashMap<String, Download>>,
    /// `any;` chat guids on macOS 26 carry no service, and a message I sent has no
    /// handle to read one from, so every mapped chat records its service here.
    chat_services: RwLock<HashMap<String, Service>>,
    /// Groups whose icon answered 404 this session, so a chat with no photo is not asked again every pass.
    icon_misses: Mutex<HashSet<String>>,
    icon_slots: Semaphore,
    avatars_ready: tokio::sync::OnceCell<()>,
    ffmpeg: tokio::sync::OnceCell<bool>,
}

/// REST timeouts: 30 s, uploads and downloads 5 min. Downloads stream to
/// `<path>.part` then rename. Group icons: at most six in flight, 3 s each, 404s
/// remembered for the session. Contact avatars: four decoded at a time.
pub struct BlueBubblesTransport {
    inner: Arc<Inner>,
}

impl BlueBubblesTransport {
    pub fn new(options: BlueBubblesOptions, http: reqwest::Client) -> Self {
        let base = server_base(&options.url).unwrap_or_else(|_| Url::parse("http://invalid.invalid").expect("static URL"));
        BlueBubblesTransport {
            inner: Arc::new(Inner {
                base,
                password: options.password,
                attachments_dir: options.attachments_dir,
                http,
                connection: Mutex::new(None),
                contacts: RwLock::new(Arc::new(ContactIndex::default())),
                contacts_seeded: AtomicBool::new(false),
                server_info: RwLock::new(None),
                listeners: Mutex::new(Vec::new()),
                downloads: Mutex::new(HashMap::new()),
                chat_services: RwLock::new(HashMap::new()),
                icon_misses: Mutex::new(HashSet::new()),
                icon_slots: Semaphore::new(ICON_SLOTS),
                avatars_ready: tokio::sync::OnceCell::new(),
                ffmpeg: tokio::sync::OnceCell::new(),
            }),
        }
    }
}

impl Inner {
    fn emit(&self, event: TransportEvent) {
        let mut listeners = self.listeners.lock();
        listeners.retain(|listener| listener.send(event.clone()).is_ok());
    }

    fn url(&self, path: &str, query: &[(&str, String)]) -> Url {
        let mut url = self.base.clone();
        url.set_path(&format!("/api/v1{path}"));
        url.set_fragment(None);
        url.set_query(None);
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("password", &self.password);
            for (key, value) in query {
                pairs.append_pair(key, value);
            }
        }
        url
    }

    /// The envelope's `data`, None when the server sent none.
    async fn request<T: DeserializeOwned>(&self, method: reqwest::Method, path: &str, query: &[(&str, String)], body: Body) -> TransportResult<Option<T>> {
        let mut builder = self.http.request(method, self.url(path, query));
        builder = match body {
            Body::None => builder.timeout(REQUEST_TIMEOUT),
            Body::Json(json) => builder.timeout(REQUEST_TIMEOUT).json(&json),
            // An upload is as slow as the file is big.
            Body::Form(form) => builder.timeout(TRANSFER_TIMEOUT).multipart(form),
        };
        let response = builder.send().await.map_err(network)?;
        let http = response.status();
        let bytes = response.bytes().await.map_err(network)?;
        let body = repair_lone_surrogates(&bytes);
        match serde_json::from_slice::<Envelope<T>>(&body) {
            Ok(envelope) => match envelope.failure(http) {
                Some(err) => Err(err),
                None => Ok(envelope.data),
            },
            Err(shape) => match serde_json::from_slice::<Envelope<IgnoredAny>>(&body) {
                Ok(envelope) => Err(envelope.failure(http).unwrap_or_else(|| TransportError::Server {
                    status: http.as_u16(),
                    message: format!("Unexpected response from the server: {shape}"),
                })),
                Err(_) => Err(TransportError::Network(format!("Request failed (status {})", http.as_u16()))),
            },
        }
    }

    async fn data<T: DeserializeOwned>(&self, method: reqwest::Method, path: &str, query: &[(&str, String)], body: Body) -> TransportResult<T> {
        self.request(method, path, query, body).await?.ok_or_else(|| TransportError::Server { status: 200, message: "The server sent no data".into() })
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Body) -> TransportResult<()> {
        self.request::<IgnoredAny>(method, path, &[], body).await.map(|_| ())
    }

    async fn fetch_server_info(&self) -> TransportResult<ServerInfo> {
        let raw: RawServerInfo = self.data(reqwest::Method::GET, "/server/info", &[], Body::None).await?;
        Ok(to_server_info(&raw))
    }

    fn send_method(&self) -> &'static str {
        match &*self.server_info.read() {
            Some(info) if info.private_api && info.helper_connected => "private-api",
            _ => "apple-script",
        }
    }

    fn contacts(&self) -> Arc<ContactIndex> {
        self.contacts.read().clone()
    }

    fn chat_service(&self, chat_guid: Option<&str>) -> Option<Service> {
        chat_guid.and_then(|guid| self.chat_services.read().get(guid).copied())
    }

    fn cache_path(&self, guid: &str, extension: &str) -> PathBuf {
        self.attachments_dir.join(format!("{guid}{extension}"))
    }

    /// Cached attachment paths for a page of messages, all looked up in one blocking call.
    async fn resolve_attachment_paths<'a>(&self, messages: impl IntoIterator<Item = &'a RawMessage>) -> HashMap<String, PathBuf> {
        let candidates: Vec<(String, PathBuf)> = messages
            .into_iter()
            .flat_map(|message| message.attachments.iter().flatten())
            .map(|attachment| {
                let plan = download_plan(attachment.transfer_name.as_deref(), attachment.mime_type.as_deref(), attachment.is_sticker.unwrap_or(false));
                (attachment.guid.clone(), self.cache_path(&attachment.guid, &plan.extension))
            })
            .collect();
        if candidates.is_empty() {
            return HashMap::new();
        }
        tokio::task::spawn_blocking(move || {
            candidates.into_iter().filter_map(|(guid, path)| Some((guid, cached_file_blocking(&path)?))).collect()
        })
        .await
        .unwrap_or_default()
    }

    async fn map_messages(&self, raws: &[RawMessage], chat_guid: Option<&str>) -> Vec<Message> {
        let paths = self.resolve_attachment_paths(raws).await;
        let contacts = self.contacts();
        raws.iter()
            .map(|raw| {
                let resolved = raw.chats.as_ref().and_then(|chats| chats.first()).map(|chat| chat.guid.as_str()).or(chat_guid);
                let options = MapOptions {
                    contacts: Some(&contacts),
                    attachment_paths: Some(&paths),
                    chat_service: self.chat_service(resolved),
                    chat_icon: None,
                };
                to_message(raw, chat_guid, &options)
            })
            .collect()
    }

    async fn map_message(&self, raw: &RawMessage, chat_guid: Option<&str>) -> Message {
        self.map_messages(std::slice::from_ref(raw), chat_guid).await.remove(0)
    }

    async fn map_chats(&self, raws: &[RawChat]) -> Vec<Chat> {
        let paths = self.resolve_attachment_paths(raws.iter().filter_map(|chat| chat.last_message.as_deref())).await;
        let icons = join_all(raws.iter().map(|raw| async move {
            if raw.style == 43 { self.fetch_chat_icon(&raw.guid).await } else { None }
        }))
        .await;
        let contacts = self.contacts();
        let chats: Vec<Chat> = raws
            .iter()
            .zip(icons)
            .map(|(raw, chat_icon)| {
                to_chat(raw, &MapOptions { contacts: Some(&contacts), attachment_paths: Some(&paths), chat_service: None, chat_icon })
            })
            .collect();
        let mut services = self.chat_services.write();
        for chat in &chats {
            services.insert(chat.guid.clone(), chat.service);
        }
        chats
    }

    async fn get_chat(&self, chat_guid: &str) -> TransportResult<Chat> {
        let query = [("with", "participants".to_owned())];
        let raw: RawChat = self.data(reqwest::Method::GET, &format!("/chat/{}", enc(chat_guid)), &query, Body::None).await?;
        Ok(self.map_chats(std::slice::from_ref(&raw)).await.remove(0))
    }

    fn avatars_dir(&self) -> PathBuf {
        match self.attachments_dir.parent() {
            Some(parent) => parent.join("avatars"),
            None => self.attachments_dir.join("..").join("avatars"),
        }
    }

    async fn ensure_avatars_dir(&self) -> std::io::Result<()> {
        self.avatars_ready.get_or_try_init(|| tokio::fs::create_dir_all(self.avatars_dir())).await.map(|_| ())
    }

    /// The square cut is what the window shows; without one the original is used.
    async fn write_square(&self, bytes: Vec<u8>, square: &Path, fallback: &Path) -> String {
        let png = tokio::task::spawn_blocking(move || square_thumbnail(&bytes, THUMBNAIL_SIDE)).await.ok().flatten();
        if let Some(png) = png {
            if tokio::fs::write(square, png).await.is_ok() {
                return path_string(square);
            }
        }
        path_string(fallback)
    }

    /// The photo as Contacts hands it over is kept as the change marker for the square cut.
    async fn save_contact_avatar(&self, raw: &RawContact) -> Option<String> {
        let encoded: String = raw.avatar.as_deref()?.chars().filter(|c| !c.is_whitespace()).collect();
        if encoded.is_empty() {
            return None;
        }
        let engine = &base64::engine::general_purpose::STANDARD;
        let bytes = engine.decode(&encoded).or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(encoded.trim_end_matches('='))).ok()?;
        let id = raw.id_string();
        let path = self.avatars_dir().join(format!("contact-{id}.jpg"));
        let square = self.avatars_dir().join(format!("contact-{id}.png"));
        let unchanged = tokio::fs::metadata(&path).await.is_ok_and(|info| info.len() == bytes.len() as u64);
        if unchanged && tokio::fs::try_exists(&square).await.unwrap_or(false) {
            return Some(path_string(&square));
        }
        self.ensure_avatars_dir().await.ok()?;
        tokio::fs::write(&path, &bytes).await.ok()?;
        Some(self.write_square(bytes, &square, &path).await)
    }

    async fn fetch_raw_contacts(&self) -> TransportResult<Vec<RawContact>> {
        // extraProperties=avatar is an opt-in some server builds reject.
        match self.data(reqwest::Method::GET, "/contact", &[("extraProperties", "avatar".to_owned())], Body::None).await {
            Ok(contacts) => Ok(contacts),
            Err(_) => self.data(reqwest::Method::GET, "/contact", &[], Body::None).await,
        }
    }

    async fn list_contacts(&self) -> TransportResult<Vec<Contact>> {
        let raw = self.fetch_raw_contacts().await?;
        Ok(futures_util::stream::iter(raw)
            .map(|item| async move {
                let avatar = self.save_contact_avatar(&item).await;
                to_contact(&item, avatar)
            })
            .buffered(CONTACT_AVATARS)
            .collect()
            .await)
    }

    async fn exists(path: &Path) -> bool {
        tokio::fs::try_exists(path).await.unwrap_or(false)
    }

    /// `GET /chat/:guid/icon` 404s for a group with no photo, and a slow icon must never hold up the chat list.
    async fn fetch_chat_icon(&self, chat_guid: &str) -> Option<String> {
        if self.icon_misses.lock().contains(chat_guid) {
            return None;
        }
        let hash = sha1_hex(chat_guid);
        let path = self.avatars_dir().join(format!("chat-{hash}.jpg"));
        let square = self.avatars_dir().join(format!("chat-{hash}.png"));
        let cached = || async {
            if Self::exists(&square).await {
                Some(path_string(&square))
            } else if Self::exists(&path).await {
                Some(path_string(&path))
            } else {
                None
            }
        };
        if let Some(found) = cached().await {
            return Some(found);
        }
        let _slot = self.icon_slots.acquire().await.ok()?;
        if let Some(found) = cached().await {
            return Some(found);
        }
        let url = self.url(&format!("/chat/{}/icon", enc(chat_guid)), &[]);
        let fetched = tokio::time::timeout(ICON_TIMEOUT, async {
            let response = self.http.get(url).timeout(ICON_TIMEOUT).send().await.ok()?;
            let status = response.status();
            let bytes = if status.is_success() { response.bytes().await.ok() } else { None };
            Some((status, bytes))
        })
        .await
        .ok()
        .flatten();
        let (status, bytes) = fetched?;
        if status == reqwest::StatusCode::NOT_FOUND {
            self.icon_misses.lock().insert(chat_guid.to_owned());
            return None;
        }
        let bytes = bytes?.to_vec();
        self.ensure_avatars_dir().await.ok()?;
        tokio::fs::write(&path, &bytes).await.ok()?;
        Some(self.write_square(bytes, &square, &path).await)
    }

    async fn has_ffmpeg(&self) -> bool {
        *self.ffmpeg.get_or_init(|| async { tokio::task::spawn_blocking(|| which::which("ffmpeg").is_ok()).await.unwrap_or(false) }).await
    }

    /// Streams straight to disk: the warm pass fetches sixty megabyte videos in the background.
    async fn download(&self, route: &str, original: bool, target: &Path) -> TransportResult<()> {
        let url = self.url(route, &[("original", original.to_string())]);
        let response = self.http.get(url).timeout(TRANSFER_TIMEOUT).send().await.map_err(network)?;
        let status = response.status();
        if !status.is_success() {
            return Err(TransportError::Server { status: status.as_u16(), message: format!("Failed to download attachment (status {})", status.as_u16()) });
        }
        let file = tokio::fs::File::create(target).await.map_err(io_error)?;
        let mut writer = tokio::io::BufWriter::with_capacity(256 * 1024, file);
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            writer.write_all(&chunk.map_err(network)?).await.map_err(io_error)?;
        }
        writer.flush().await.map_err(io_error)?;
        writer.into_inner().sync_all().await.map_err(io_error)
    }

    /// A sticker comes down as the original HEIC and is decoded here, alpha and all;
    /// when that cannot happen the server's flattened JPEG is taken instead.
    async fn fetch_attachment(&self, guid: &str, path: &Path, plan: &DownloadPlan) -> TransportResult<()> {
        tokio::fs::create_dir_all(&self.attachments_dir).await.map_err(io_error)?;
        let route = format!("/attachment/{}/download", enc(guid));
        let mut part = path.as_os_str().to_owned();
        part.push(".part");
        let part = PathBuf::from(part);
        let convert = plan.sticker && self.has_ffmpeg().await;
        self.download(&route, convert || plan.original, &part).await?;
        if convert {
            let mut head = Vec::with_capacity(16);
            if let Ok(file) = tokio::fs::File::open(&part).await {
                let _ = file.take(16).read_to_end(&mut head).await;
            }
            if is_heif(&head) {
                let mut png = part.as_os_str().to_owned();
                png.push(".png");
                let png = PathBuf::from(png);
                if heif_to_png(&part, &png).await {
                    tokio::fs::rename(&png, &part).await.map_err(io_error)?;
                } else {
                    self.download(&route, false, &part).await?;
                }
            }
        }
        tokio::fs::rename(&part, path).await.map_err(io_error)
    }

    async fn fill_cache(&self, guid: &str, plan: DownloadPlan) -> TransportResult<PathBuf> {
        let path = self.cache_path(guid, &plan.extension);
        match cached_file(&path).await {
            Some(existing) if existing != path => return Ok(existing),
            Some(_) => {}
            None => self.fetch_attachment(guid, &path, &plan).await?,
        }
        share_by_content(&path, &plan.extension).await.map_err(io_error)
    }

    async fn refresh_server_info(self: Arc<Self>) {
        match self.fetch_server_info().await {
            Ok(info) => {
                *self.server_info.write() = Some(info.clone());
                self.emit(TransportEvent::Server(info));
            }
            Err(err) => tracing::error!("BlueBubbles: failed to refresh server info: {err}"),
        }
    }

    fn online(self: &Arc<Self>) {
        self.emit(TransportEvent::Connection { status: ConnectionStatus::Online, error: None });
        tokio::spawn(self.clone().refresh_server_info());
    }

    fn offline(&self, error: String) {
        self.emit(TransportEvent::Connection { status: ConnectionStatus::Offline, error: Some(error) });
    }

    async fn open_socket(self: &Arc<Self>) -> TransportResult<()> {
        // A retry after a failed attempt must not leave the old socket reconnecting beside the new one.
        let previous = self.connection.lock().take();
        if let Some(previous) = previous {
            previous.pump.abort();
            previous.socket.close().await;
        }
        let (socket, events) = SocketIo::connect(self.base.as_str(), &self.password).await.map_err(|err| {
            TransportError::Network(match err {
                super::socket::SocketError::Io(message) => message,
                other => other.to_string(),
            })
        })?;
        self.online();
        let pump = tokio::spawn(pump(Arc::downgrade(self), events)).abort_handle();
        *self.connection.lock() = Some(Connection { socket, pump });
        Ok(())
    }

    fn socket(&self) -> Option<SocketIo> {
        self.connection.lock().as_ref().map(|connection| connection.socket.clone())
    }

    async fn handle_event(self: &Arc<Self>, name: &str, args: Vec<Value>) {
        let Some(data) = args.into_iter().next() else {
            return;
        };
        if MESSAGE_EVENTS.contains(&name) || GROUP_EVENTS.contains(&name) {
            let raw: RawMessage = match serde_json::from_value(data) {
                Ok(raw) => raw,
                Err(err) => {
                    tracing::error!("BlueBubbles: failed to handle \"{name}\": {err}");
                    return;
                }
            };
            let message = self.map_message(&raw, None).await;
            let chat_guid = message.chat_guid.clone();
            self.emit(TransportEvent::Message(message));
            if GROUP_EVENTS.contains(&name) {
                let inner = self.clone();
                let name = name.to_owned();
                tokio::spawn(async move {
                    match inner.get_chat(&chat_guid).await {
                        Ok(chat) => inner.emit(TransportEvent::Chat(chat)),
                        Err(err) => tracing::error!("BlueBubbles: failed to handle \"{name}\": {err}"),
                    }
                });
            }
            return;
        }
        match name {
            "chat-read-status-changed" => {
                if let (Some(chat_guid), Some(read)) = (data.get("chatGuid").and_then(Value::as_str), data.get("read").and_then(Value::as_bool)) {
                    self.emit(TransportEvent::Read { chat_guid: chat_guid.to_owned(), read });
                }
            }
            "typing-indicator" => {
                if let (Some(chat_guid), Some(typing)) = (data.get("guid").and_then(Value::as_str), data.get("display").and_then(Value::as_bool)) {
                    self.emit(TransportEvent::Typing { chat_guid: chat_guid.to_owned(), typing });
                }
            }
            "ft-call-status-changed" => {
                let Ok(raw) = serde_json::from_value::<RawFaceTimeStatus>(data) else {
                    return;
                };
                // Outgoing, answered and unknown are not worth an event.
                let status = match raw.status.as_str() {
                    "incoming" => FaceTimeStatus::Incoming,
                    "disconnected" => FaceTimeStatus::Ended,
                    _ => return,
                };
                let contacts = self.contacts();
                self.emit(TransportEvent::FaceTime {
                    call_uuid: raw.uuid,
                    status,
                    from: raw.handle.as_ref().map(|handle| to_handle(handle, Some(&contacts))),
                    can_answer: status == FaceTimeStatus::Incoming,
                });
            }
            // With "FaceTime Calling" off: a JSON string naming the caller, with no call uuid to answer.
            "incoming-facetime" => {
                let parsed = match &data {
                    Value::String(text) => serde_json::from_str::<Value>(text).unwrap_or(Value::Null),
                    other => other.clone(),
                };
                let caller = parsed.get("caller").and_then(Value::as_str).filter(|caller| !caller.is_empty());
                let contacts = self.contacts();
                let from = caller.map(|address| {
                    let raw = RawHandle { original_rowid: 0, address: address.to_owned(), service: "iMessage".into(), country: None, uncanonicalized_id: None };
                    to_handle(&raw, Some(&contacts))
                });
                self.emit(TransportEvent::FaceTime { call_uuid: String::new(), status: FaceTimeStatus::Incoming, from, can_answer: false });
            }
            _ => {}
        }
    }
}

/// Socket events in arrival order; only the chat refetch after a group event runs beside it.
async fn pump(inner: Weak<Inner>, mut events: mpsc::UnboundedReceiver<SocketEvent>) {
    while let Some(event) = events.recv().await {
        let Some(inner) = inner.upgrade() else {
            return;
        };
        match event {
            SocketEvent::Connected => inner.online(),
            SocketEvent::ConnectError(error) | SocketEvent::Disconnected(error) => inner.offline(error),
            SocketEvent::Event { name, args } => inner.handle_event(&name, args).await,
        }
    }
}

/// One `POST /message/query` where clause.
#[derive(Clone, Debug, PartialEq)]
pub struct WhereClause {
    pub statement: String,
    pub args: Option<Value>,
}

impl WhereClause {
    fn new(statement: &str, args: Option<Value>) -> Self {
        WhereClause { statement: statement.to_owned(), args }
    }

    fn to_json(&self) -> Value {
        object(vec![("statement", Some(Value::from(self.statement.clone()))), ("args", self.args.clone())])
    }
}

/// Search filters to where clauses. Dates stay in the top-level `after`/`before` fields (Cocoa epoch in chat.db).
/// Column and alias names are the server's TypeORM ones (message, handle, chat, attachment);
/// `chat` is joined because the search always asks for "chats".
pub fn build_search_where(filters: &SearchFilters) -> Vec<WhereClause> {
    let mut clauses = Vec::new();
    if filters.from_me {
        clauses.push(WhereClause::new("message.is_from_me = :fromMe", Some(json!({ "fromMe": 1 }))));
    }
    if !filters.senders.is_empty() {
        clauses.push(WhereClause::new("handle.id IN (:...senders)", Some(json!({ "senders": filters.senders }))));
    }
    match filters.attachments {
        Some(AttachmentFilter::Image) => clauses.push(WhereClause::new("attachment.mime_type LIKE 'image/%'", None)),
        Some(AttachmentFilter::Video) => clauses.push(WhereClause::new("attachment.mime_type LIKE 'video/%'", None)),
        Some(AttachmentFilter::File) => clauses.push(WhereClause::new(
            "attachment.mime_type IS NOT NULL AND attachment.mime_type NOT LIKE 'image/%' AND attachment.mime_type NOT LIKE 'video/%'",
            None,
        )),
        None => {}
    }
    if filters.links {
        clauses.push(WhereClause::new(
            "(message.balloon_bundle_id = :linkBundle OR message.text LIKE :linkText)",
            Some(json!({ "linkBundle": "com.apple.messages.URLBalloonProvider", "linkText": "%http%" })),
        ));
    }
    if let Some(chat_guids) = &filters.chat_guids {
        clauses.push(if chat_guids.is_empty() {
            WhereClause::new("1 = 0", None)
        } else {
            WhereClause::new("chat.guid IN (:...chatGuids)", Some(json!({ "chatGuids": chat_guids })))
        });
    }
    clauses
}

fn service_name(service: Service) -> &'static str {
    match service {
        Service::IMessage => "iMessage",
        Service::Sms => "SMS",
        Service::Rcs => "RCS",
    }
}

async fn file_part(path: &Path, name: String) -> TransportResult<reqwest::multipart::Part> {
    let file = tokio::fs::File::open(path).await.map_err(io_error)?;
    let length = file.metadata().await.map_err(io_error)?.len();
    Ok(reqwest::multipart::Part::stream_with_length(file, length).file_name(name))
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

#[async_trait]
impl Transport for BlueBubblesTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::BlueBubbles
    }

    async fn connect(&self) -> TransportResult<ServerInfo> {
        let inner = &self.inner;
        inner.emit(TransportEvent::Connection { status: ConnectionStatus::Connecting, error: None });
        let result: TransportResult<ServerInfo> = async {
            let info = inner.fetch_server_info().await?;
            *inner.server_info.write() = Some(info.clone());
            inner.emit(TransportEvent::Server(info.clone()));

            // The contact list carries every avatar as base64, so it is the slow
            // request. With a seeded index the socket opens first; only a cold start waits for names.
            let loader = inner.clone();
            let contacts = tokio::spawn(async move {
                match loader.list_contacts().await {
                    Ok(list) => {
                        *loader.contacts.write() = Arc::new(ContactIndex::new(&list));
                        loader.emit(TransportEvent::Contacts(list));
                    }
                    Err(err) => tracing::error!("BlueBubbles: failed to load contacts: {err}"),
                }
            });
            if !inner.contacts_seeded.load(Ordering::Acquire) {
                let _ = contacts.await;
            }
            inner.open_socket().await?;
            Ok(info)
        }
        .await;
        if let Err(err) = &result {
            inner.offline(err.to_string());
        }
        result
    }

    async fn disconnect(&self) {
        let connection = self.inner.connection.lock().take();
        if let Some(connection) = connection {
            connection.pump.abort();
            let was_connected = connection.socket.is_connected();
            connection.socket.close().await;
            if was_connected {
                self.inner.offline("io client disconnect".into());
            }
        }
    }

    fn subscribe(&self) -> mpsc::UnboundedReceiver<TransportEvent> {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.inner.listeners.lock().push(sender);
        receiver
    }

    fn seed_contacts(&self, contacts: &[Contact]) {
        if contacts.is_empty() {
            return;
        }
        *self.inner.contacts.write() = Arc::new(ContactIndex::new(contacts));
        self.inner.contacts_seeded.store(true, Ordering::Release);
    }

    async fn list_chats(&self, options: ListChatsOptions) -> TransportResult<Page<Chat>> {
        let limit = options.limit.unwrap_or(200);
        let offset = options.offset.unwrap_or(0);
        // Participants come back by default; "lastmessage" also flips the sort.
        let body = json!({ "with": ["participants", "lastmessage"], "sort": "lastmessage", "limit": limit, "offset": offset });
        let raw: Vec<RawChat> = self.inner.data(reqwest::Method::POST, "/chat/query", &[], Body::Json(body)).await?;
        let has_more = raw.len() == limit as usize;
        Ok(Page { items: self.inner.map_chats(&raw).await, has_more })
    }

    async fn get_chat(&self, chat_guid: &str) -> TransportResult<Chat> {
        self.inner.get_chat(chat_guid).await
    }

    async fn load_messages(&self, chat_guid: &str, options: LoadMessagesOptions) -> TransportResult<Page<Message>> {
        // The bare "payloadData" is ignored on this route, and attributedBody is
        // only matched under the prefixed name on some builds, so both go.
        let mut query = vec![
            ("with", "attachments,message.payloadData,message.attributedBody,attributedBody".to_owned()),
            ("sort", "DESC".to_owned()),
            ("limit", options.limit.to_string()),
        ];
        if let Some(before) = options.before {
            query.push(("before", before.to_string()));
        }
        let raw: Vec<RawMessage> = self.inner.data(reqwest::Method::GET, &format!("/chat/{}/message", enc(chat_guid)), &query, Body::None).await?;
        let has_more = raw.len() == options.limit as usize;
        let mut items = self.inner.map_messages(&raw, Some(chat_guid)).await;
        items.sort_by_key(|message| message.date);
        Ok(Page { items, has_more })
    }

    async fn search_messages(&self, query: &str, filters: &SearchFilters) -> TransportResult<Vec<Message>> {
        // An empty query with `after` is the reconcile sweep: everything since then, oldest first.
        let sweep = query.is_empty() && filters.after.is_some();
        let mut clauses: Vec<Value> = build_search_where(filters).iter().map(WhereClause::to_json).collect();
        if !query.is_empty() {
            clauses.push(json!({ "statement": "message.text LIKE :text COLLATE NOCASE", "args": { "text": format!("%{query}%") } }));
        }
        let body = object(vec![
            ("with", Some(json!(["chats", "attachments", "payloadData", "attributedBody"]))),
            ("sort", Some(Value::from(if sweep { "ASC" } else { "DESC" }))),
            ("limit", Some(Value::from(filters.limit.unwrap_or(50)))),
            ("chatGuid", filters.chat_guid.clone().map(Value::from)),
            ("after", filters.after.map(Value::from)),
            ("before", filters.before.map(Value::from)),
            ("where", (!clauses.is_empty()).then_some(Value::Array(clauses))),
        ]);
        let raw: Vec<RawMessage> = self.inner.data(reqwest::Method::POST, "/message/query", &[], Body::Json(body)).await?;
        Ok(self.inner.map_messages(&raw, filters.chat_guid.as_deref()).await)
    }

    async fn list_contacts(&self) -> TransportResult<Vec<Contact>> {
        self.inner.list_contacts().await
    }

    async fn send_text(&self, chat_guid: &str, text: &str, options: SendTextOptions) -> TransportResult<Message> {
        let replying = options.reply_to.is_some();
        let body = object(vec![
            ("chatGuid", Some(Value::from(chat_guid))),
            ("message", Some(Value::from(text))),
            ("method", Some(Value::from(self.inner.send_method()))),
            ("tempGuid", options.temp_guid.map(Value::from)),
            ("effectId", options.effect.map(Value::from)),
            ("subject", options.subject.map(Value::from)),
            ("selectedMessageGuid", options.reply_to.map(Value::from)),
            ("partIndex", replying.then(|| Value::from(0))),
        ]);
        let raw: RawMessage = self.inner.data(reqwest::Method::POST, "/message/text", &[], Body::Json(body)).await?;
        Ok(self.inner.map_message(&raw, Some(chat_guid)).await)
    }

    async fn send_attachment(&self, chat_guid: &str, path: &Path, options: SendAttachmentOptions) -> TransportResult<Message> {
        let name = options.name.unwrap_or_else(|| file_name(path));
        let mut form = reqwest::multipart::Form::new()
            .part("attachment", file_part(path, name.clone()).await?)
            .text("chatGuid", chat_guid.to_owned())
            .text("method", self.inner.send_method())
            .text("name", name);
        if let Some(temp_guid) = options.temp_guid {
            form = form.text("tempGuid", temp_guid);
        }
        if options.is_audio {
            form = form.text("isAudioMessage", "true");
        }
        let raw: RawMessage = self.inner.data(reqwest::Method::POST, "/message/attachment", &[], Body::Form(form)).await?;
        Ok(self.inner.map_message(&raw, Some(chat_guid)).await)
    }

    /// Every entry ends up as a link to a file named by its content, so the same
    /// GIF sent forty times is one file and one decode, and that shared path is returned.
    async fn attachment_path(&self, attachment_guid: &str, options: AttachmentPathOptions) -> TransportResult<PathBuf> {
        let plan = download_plan(options.name.as_deref(), options.mime.as_deref(), options.sticker);
        let job = {
            let mut downloads = self.inner.downloads.lock();
            match downloads.get(attachment_guid) {
                Some(pending) => pending.clone(),
                None => {
                    let inner = self.inner.clone();
                    let guid = attachment_guid.to_owned();
                    // Spawned so a caller that goes away does not stall the others waiting on it.
                    let task = tokio::spawn(async move {
                        let result = inner.fill_cache(&guid, plan).await;
                        inner.downloads.lock().remove(&guid);
                        result
                    });
                    let job = async move { task.await.unwrap_or_else(|err| Err(TransportError::Network(err.to_string()))) }.boxed().shared();
                    downloads.insert(attachment_guid.to_owned(), job.clone());
                    job
                }
            }
        };
        job.await
    }

    async fn create_chat(&self, addresses: &[String], first_message: &str, service: Option<Service>) -> TransportResult<Chat> {
        let body = object(vec![
            ("addresses", Some(json!(addresses))),
            ("message", Some(Value::from(first_message))),
            ("service", service.map(|service| Value::from(service_name(service)))),
        ]);
        let raw: RawChat = self.inner.data(reqwest::Method::POST, "/chat/new", &[], Body::Json(body)).await?;
        Ok(self.inner.map_chats(std::slice::from_ref(&raw)).await.remove(0))
    }

    async fn mark_read(&self, chat_guid: &str) -> TransportResult<()> {
        self.inner.call(reqwest::Method::POST, &format!("/chat/{}/read", enc(chat_guid)), Body::None).await
    }

    async fn delete_chat(&self, chat_guid: &str) -> TransportResult<()> {
        self.inner.call(reqwest::Method::DELETE, &format!("/chat/{}", enc(chat_guid)), Body::None).await
    }

    async fn schedule_text(&self, chat_guid: &str, text: &str, send_at: Millis) -> TransportResult<ScheduledMessage> {
        let body = json!({
            "type": "send-message",
            "payload": { "chatGuid": chat_guid, "message": text, "method": self.inner.send_method() },
            "scheduledFor": send_at,
            "schedule": { "type": "once" },
        });
        let raw: RawScheduledMessage = self.inner.data(reqwest::Method::POST, "/message/schedule", &[], Body::Json(body)).await?;
        Ok(to_scheduled_message(&raw).unwrap_or_else(|| ScheduledMessage {
            id: raw.id_string(),
            chat_guid: chat_guid.to_owned(),
            text: text.to_owned(),
            send_at,
        }))
    }

    async fn list_scheduled(&self) -> TransportResult<Vec<ScheduledMessage>> {
        let raw: Vec<RawScheduledMessage> = self.inner.data(reqwest::Method::GET, "/message/schedule", &[], Body::None).await?;
        Ok(raw.iter().filter(|item| item.kind == "send-message").filter_map(to_scheduled_message).collect())
    }

    async fn cancel_scheduled(&self, id: &str) -> TransportResult<()> {
        self.inner.call(reqwest::Method::DELETE, &format!("/message/schedule/{}", enc(id)), Body::None).await
    }

    async fn react(&self, chat_guid: &str, message_guid: &str, kind: TapbackKind, options: ReactOptions) -> TransportResult<()> {
        if kind == TapbackKind::Emoji {
            // The server's possibleReactions is fixed to the six named tapbacks and their removals.
            return Err(TransportError::Server { status: 400, message: "BlueBubbles does not support custom emoji tapbacks".into() });
        }
        let reaction = if options.remove { format!("-{}", kind.as_str()) } else { kind.as_str().to_owned() };
        let body = object(vec![
            ("chatGuid", Some(Value::from(chat_guid))),
            ("selectedMessageGuid", Some(Value::from(message_guid))),
            ("reaction", Some(Value::from(reaction))),
            ("partIndex", options.part_index.map(Value::from)),
        ]);
        self.inner.call(reqwest::Method::POST, "/message/react", Body::Json(body)).await
    }

    async fn set_typing(&self, chat_guid: &str, typing: bool) -> TransportResult<()> {
        if typing {
            return self.inner.call(reqwest::Method::POST, &format!("/chat/{}/typing", enc(chat_guid)), Body::None).await;
        }
        // Server 1.9.9's DELETE /chat/:guid/typing calls startTyping, so the
        // other side keeps the bubble; the socket route is wired to the real stop.
        let Some(socket) = self.inner.socket().filter(SocketIo::is_connected) else {
            return Ok(());
        };
        socket
            .emit_with_ack("stopped-typing", json!({ "chatGuid": chat_guid }), Duration::from_secs(5))
            .await
            .map(|_| ())
            .map_err(|err| TransportError::Network(err.to_string()))
    }

    async fn mark_unread(&self, chat_guid: &str) -> TransportResult<()> {
        self.inner.call(reqwest::Method::POST, &format!("/chat/{}/unread", enc(chat_guid)), Body::None).await
    }

    async fn edit_message(&self, chat_guid: &str, message_guid: &str, text: &str, options: EditOptions) -> TransportResult<Message> {
        let body = object(vec![
            ("editedMessage", Some(Value::from(text))),
            ("backwardsCompatibilityMessage", Some(Value::from(options.backwards_compat_text.unwrap_or_else(|| text.to_owned())))),
            ("partIndex", options.part_index.map(Value::from)),
        ]);
        let raw: RawMessage = self.inner.data(reqwest::Method::POST, &format!("/message/{}/edit", enc(message_guid)), &[], Body::Json(body)).await?;
        Ok(self.inner.map_message(&raw, Some(chat_guid)).await)
    }

    async fn unsend_message(&self, _chat_guid: &str, message_guid: &str, part_index: Option<u32>) -> TransportResult<()> {
        let body = object(vec![("partIndex", part_index.map(Value::from))]);
        self.inner.call(reqwest::Method::POST, &format!("/message/{}/unsend", enc(message_guid)), Body::Json(body)).await
    }

    async fn rename_group(&self, chat_guid: &str, name: &str) -> TransportResult<()> {
        self.inner.call(reqwest::Method::PUT, &format!("/chat/{}", enc(chat_guid)), Body::Json(json!({ "displayName": name }))).await
    }

    async fn add_participant(&self, chat_guid: &str, address: &str) -> TransportResult<()> {
        let path = format!("/chat/{}/participant/add", enc(chat_guid));
        self.inner.call(reqwest::Method::POST, &path, Body::Json(json!({ "address": address }))).await
    }

    async fn remove_participant(&self, chat_guid: &str, address: &str) -> TransportResult<()> {
        let path = format!("/chat/{}/participant/remove", enc(chat_guid));
        self.inner.call(reqwest::Method::POST, &path, Body::Json(json!({ "address": address }))).await
    }

    async fn leave_group(&self, chat_guid: &str) -> TransportResult<()> {
        self.inner.call(reqwest::Method::POST, &format!("/chat/{}/leave", enc(chat_guid)), Body::None).await
    }

    async fn set_group_icon(&self, chat_guid: &str, path: &Path) -> TransportResult<()> {
        let form = reqwest::multipart::Form::new().part("icon", file_part(path, file_name(path)).await?);
        self.inner.call(reqwest::Method::POST, &format!("/chat/{}/icon", enc(chat_guid)), Body::Form(form)).await
    }

    /// The helper answers "unknown" for a Focus the person does not share, the same as none from here.
    async fn focus_status(&self, address: &str) -> TransportResult<FocusStatus> {
        let data: Value = self.inner.data(reqwest::Method::GET, &format!("/handle/{}/focus", enc(address)), &[], Body::None).await?;
        Ok(match data.get("status").and_then(Value::as_str) {
            Some("silenced") => FocusStatus::Silenced,
            Some("none") => FocusStatus::None,
            _ => FocusStatus::Unknown,
        })
    }

    async fn notify_silenced(&self, _chat_guid: &str, message_guid: &str) -> TransportResult<()> {
        self.inner.call(reqwest::Method::POST, &format!("/message/{}/notify", enc(message_guid)), Body::None).await
    }

    async fn create_facetime_link(&self) -> TransportResult<String> {
        let data: Value = self.inner.data(reqwest::Method::POST, "/facetime/session", &[], Body::None).await?;
        link(&data)
    }

    /// The Mac answers, mints a link, admits the first joiner and hangs up its own
    /// side about 15 s later (bluebubbles-helper#38); the link still joins from a browser.
    async fn answer_facetime(&self, call_uuid: &str) -> TransportResult<String> {
        let data: Value = self.inner.data(reqwest::Method::POST, &format!("/facetime/answer/{}", enc(call_uuid)), &[], Body::None).await?;
        link(&data)
    }

    async fn leave_facetime(&self, call_uuid: &str) -> TransportResult<()> {
        self.inner.call(reqwest::Method::POST, &format!("/facetime/leave/{}", enc(call_uuid)), Body::None).await
    }
}

fn link(data: &Value) -> TransportResult<String> {
    data.get("link")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| TransportError::Server { status: 200, message: "The server sent no FaceTime link".into() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filters() -> SearchFilters {
        SearchFilters::default()
    }

    #[test]
    fn no_filters_produces_no_clauses() {
        assert_eq!(build_search_where(&filters()), vec![]);
    }

    #[test]
    fn from_me() {
        assert_eq!(
            build_search_where(&SearchFilters { from_me: true, ..filters() }),
            vec![WhereClause::new("message.is_from_me = :fromMe", Some(json!({ "fromMe": 1 })))]
        );
    }

    #[test]
    fn senders_join_on_the_handle_table() {
        let senders = vec!["+14155550134".to_owned(), "alex@example.com".to_owned()];
        assert_eq!(
            build_search_where(&SearchFilters { senders: senders.clone(), ..filters() }),
            vec![WhereClause::new("handle.id IN (:...senders)", Some(json!({ "senders": senders })))]
        );
    }

    #[test]
    fn an_empty_senders_array_adds_no_clause() {
        assert_eq!(build_search_where(&SearchFilters { senders: vec![], ..filters() }), vec![]);
    }

    #[test]
    fn attachments_image_video_and_file() {
        let with = |kind| build_search_where(&SearchFilters { attachments: Some(kind), ..filters() });
        assert_eq!(with(AttachmentFilter::Image), vec![WhereClause::new("attachment.mime_type LIKE 'image/%'", None)]);
        assert_eq!(with(AttachmentFilter::Video), vec![WhereClause::new("attachment.mime_type LIKE 'video/%'", None)]);
        assert!(with(AttachmentFilter::File)[0].statement.contains("NOT LIKE 'image/%'"));
    }

    #[test]
    fn links_matches_a_link_preview_or_a_bare_url_in_the_text() {
        assert_eq!(
            build_search_where(&SearchFilters { links: true, ..filters() }),
            vec![WhereClause::new(
                "(message.balloon_bundle_id = :linkBundle OR message.text LIKE :linkText)",
                Some(json!({ "linkBundle": "com.apple.messages.URLBalloonProvider", "linkText": "%http%" })),
            )]
        );
    }

    #[test]
    fn chat_guids_filters_to_the_given_chats() {
        assert_eq!(
            build_search_where(&SearchFilters { chat_guids: Some(vec!["a".into(), "b".into()]), ..filters() }),
            vec![WhereClause::new("chat.guid IN (:...chatGuids)", Some(json!({ "chatGuids": ["a", "b"] })))]
        );
    }

    #[test]
    fn an_empty_chat_guids_array_returns_nothing_rather_than_everything() {
        assert_eq!(build_search_where(&SearchFilters { chat_guids: Some(vec![]), ..filters() }), vec![WhereClause::new("1 = 0", None)]);
    }

    #[test]
    fn filters_combine_into_one_clause_per_filter() {
        let clauses = build_search_where(&SearchFilters {
            from_me: true,
            attachments: Some(AttachmentFilter::Video),
            chat_guids: Some(vec!["a".into()]),
            ..filters()
        });
        assert_eq!(clauses.len(), 3);
        assert_eq!(clauses[2].to_json(), json!({ "statement": "chat.guid IN (:...chatGuids)", "args": { "chatGuids": ["a"] } }));
        assert_eq!(clauses[1].to_json(), json!({ "statement": "attachment.mime_type LIKE 'video/%'" }));
    }

    #[test]
    fn server_base_assumes_http_and_keeps_https() {
        assert_eq!(server_base("mac.local:1234").unwrap().as_str(), "http://mac.local:1234/");
        assert_eq!(server_base(" https://x.ngrok.app ").unwrap().as_str(), "https://x.ngrok.app/");
        assert!(server_base("").is_err());
    }

    fn transport() -> BlueBubblesTransport {
        BlueBubblesTransport::new(
            BlueBubblesOptions { url: "http://mac.local:1234/ignored".into(), password: "p&w".into(), attachments_dir: "/cache/attachments".into() },
            reqwest::Client::new(),
        )
    }

    #[test]
    fn rest_urls_carry_the_password_and_encode_guids() {
        let url = transport().inner.url(&format!("/chat/{}/message", enc("any;-;+15555550100")), &[("limit", "10".into())]);
        assert_eq!(url.as_str(), "http://mac.local:1234/api/v1/chat/any%3B-%3B%2B15555550100/message?password=p%26w&limit=10");
        assert_eq!(transport().inner.avatars_dir(), PathBuf::from("/cache/avatars"));
    }

    #[test]
    fn envelopes_unwrap_to_data_or_the_servers_message() {
        let ok: Envelope<Vec<u32>> = serde_json::from_str(r#"{"status":200,"message":"Success","data":[1,2]}"#).unwrap();
        assert!(ok.failure(reqwest::StatusCode::OK).is_none());
        assert_eq!(ok.data, Some(vec![1, 2]));
        let failed: Envelope<IgnoredAny> =
            serde_json::from_str(r#"{"status":404,"message":"Not found","error":{"type":"Database Error","message":"Chat does not exist"}}"#).unwrap();
        assert_eq!(
            failed.failure(reqwest::StatusCode::NOT_FOUND),
            Some(TransportError::Server { status: 404, message: "Chat does not exist".into() })
        );
        let encrypted: Envelope<IgnoredAny> = serde_json::from_str(r#"{"status":200,"encrypted":true,"data":"abc"}"#).unwrap();
        assert!(matches!(encrypted.failure(reqwest::StatusCode::OK), Some(TransportError::Server { message, .. }) if message.contains("Encrypt communications")));
    }

    #[test]
    fn scheduled_messages_read_iso_and_epoch_dates() {
        let raw: RawScheduledMessage = serde_json::from_value(json!({
            "id": 7, "type": "send-message", "payload": { "chatGuid": "c", "message": "hi", "method": "apple-script" },
            "scheduledFor": "2026-09-25T10:00:00.000Z", "schedule": { "type": "once" }, "status": "pending",
        }))
        .unwrap();
        let scheduled = to_scheduled_message(&raw).unwrap();
        assert_eq!(scheduled.id, "7");
        assert_eq!(scheduled.send_at, 1_790_330_400_000);
        let epoch: RawScheduledMessage = serde_json::from_value(json!({ "id": "8", "type": "send-message", "payload": {}, "scheduledFor": 5 })).unwrap();
        assert_eq!(to_scheduled_message(&epoch).unwrap().send_at, 5);
    }

    #[test]
    fn optional_body_fields_are_left_out_not_null() {
        assert_eq!(object(vec![("a", Some(Value::from(1))), ("partIndex", None)]), json!({ "a": 1 }));
    }

    /// Read-only calls against the server in `~/.config/messages/config.json`:
    /// server info, a few chats, one page of messages, a sweep query, one
    /// attachment download, then 30 s of socket events. Prints counts and event
    /// names only. `cargo test -p messages-core live_server -- --ignored --nocapture`
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore]
    async fn live_server() {
        let config_dir = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| dirs::home_dir().unwrap().join(".config"));
        let config: Value = serde_json::from_slice(&std::fs::read(config_dir.join("messages/config.json")).unwrap()).unwrap();
        let url = config["server"]["url"].as_str().unwrap().to_owned();
        let password = config["server"]["password"].as_str().unwrap().to_owned();
        let dir = crate::dedupe::tests::TestDir::new("live");
        let transport = BlueBubblesTransport::new(
            BlueBubblesOptions { url, password, attachments_dir: dir.join("attachments") },
            reqwest::Client::new(),
        );
        let mut events = transport.subscribe();

        let started = std::time::Instant::now();
        let info = transport.connect().await.unwrap();
        println!("connected in {:?}: server {} macOS {:?} private_api {} helper {}", started.elapsed(), info.version, info.macos_version, info.private_api, info.helper_connected);

        let chats = transport.list_chats(ListChatsOptions { limit: Some(10), offset: None }).await.unwrap();
        let groups = chats.items.iter().filter(|chat| chat.is_group).count();
        let icons = chats.items.iter().filter(|chat| chat.icon.is_some()).count();
        println!("chats: {} (groups {groups}, icons {icons}), services {:?}", chats.items.len(), chats.items.iter().map(|chat| chat.service).collect::<Vec<_>>());

        let first = &chats.items[0];
        let page = transport.load_messages(&first.guid, LoadMessagesOptions { limit: 10, before: None }).await.unwrap();
        let sorted = page.items.windows(2).all(|pair| pair[0].date <= pair[1].date);
        println!("page: {} messages, has_more {}, ascending {sorted}, with parts {}", page.items.len(), page.has_more, page.items.iter().filter(|m| m.parts.is_some()).count());

        let now = chrono::Utc::now().timestamp_millis();
        let sweep = transport
            .search_messages("", &SearchFilters { after: Some(now - 7 * 24 * 3_600_000), limit: Some(10), ..Default::default() })
            .await
            .unwrap();
        println!("sweep: {} messages since a week ago", sweep.len());

        let attachment = chats
            .items
            .iter()
            .filter_map(|chat| chat.last_message.as_ref())
            .chain(page.items.iter())
            .chain(sweep.iter())
            .flat_map(|message| message.attachments.iter())
            .find(|attachment| attachment.bytes < 20_000_000);
        if let Some(attachment) = attachment {
            let options = AttachmentPathOptions { name: Some(attachment.name.clone()), mime: Some(attachment.mime.clone()), sticker: attachment.is_sticker };
            let path = transport.attachment_path(&attachment.guid, options).await.unwrap();
            let size = std::fs::metadata(&path).unwrap().len();
            let header = crate::image::image_size(&path.to_string_lossy()).await;
            println!("attachment {} ({}): {size} bytes, header size {header:?}, server size {:?}x{:?}", attachment.mime, attachment.bytes, attachment.width, attachment.height);
        } else {
            println!("attachment: none in the sample");
        }

        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        let mut seen = Vec::new();
        while let Ok(Some(event)) = tokio::time::timeout_at(deadline, events.recv()).await {
            seen.push(match event {
                TransportEvent::Connection { status, error } => format!("connection {status:?} {error:?}"),
                TransportEvent::Server(_) => "server".into(),
                TransportEvent::Message(message) => format!("message reaction={} attachments={}", message.reaction.is_some(), message.attachments.len()),
                TransportEvent::Chat(_) => "chat".into(),
                TransportEvent::ChatRemoved { .. } => "chat removed".into(),
                TransportEvent::Contacts(list) => format!("contacts {}", list.len()),
                TransportEvent::Typing { typing, .. } => format!("typing {typing}"),
                TransportEvent::Read { read, .. } => format!("read {read}"),
                TransportEvent::FaceTime { status, .. } => format!("facetime {status:?}"),
            });
        }
        println!("events over the run: {seen:?}");
        transport.disconnect().await;
    }
}
