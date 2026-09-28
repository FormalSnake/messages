# Messages

iMessage client for Linux. The Mac is the gateway: the open-source BlueBubbles
server (Apache-2.0) reads chat.db, drives Messages.app, and with SIP disabled
injects its helper into Messages.app for the Private API. This repo is the
client side only.

## Layout

Cargo workspace for the client, Bun for the Mac agent.

- `crates/core` (`messages-core`): everything that is not pixels, no GPUI.
  Model types (`model.rs`), the `Transport` trait (`transport.rs`), the
  BlueBubbles implementation (`bluebubbles/client.rs`, `map.rs`, and
  `socket.rs`, a socket.io v4 client on tokio-tungstenite and rustls, since
  `rust_socketio` needs OpenSSL), fixtures (`demo.rs`), the store
  (`store.rs`), config, formatting, notifications, clipboard, URL opening.
  Everything that does I/O runs on tokio.
- `crates/desktop` (binary `messages`): the GPUI window on gpui-kit 0.6.6
  (gpui-pre 0.3.6). `main.rs` is the entry, one file per screen beside it.
  `bridge.rs` is the only piece that turns store events into GPUI notifies.
- `apps/mac-agent` (`@messages/mac-agent`): TypeScript, runs on the Mac.

The seam is `MessagesStore`: the UI calls its methods and reads state under a
short `RwLock` guard; every change sends a narrow `StoreEvent` (`Chat(guid)`,
`Thread(guid)`, `Message{..}`, `Typing(guid)`, `Draft(guid)`, ...) on a
broadcast channel, and `bridge.rs` notifies only the entities watching that
topic. Nothing in the UI talks to the transport directly except search and
FaceTime, which own no state. `docs/rust-architecture.md` has the threading
model and the measured performance budget; `docs/rust-parity.md` is the
checklist the client was held to against the TS client it replaced.

## Why a store and a reconcile loop

BlueBubbles' own client shows stale threads until you switch chats. Here every
socket event lands in one in-memory store, sends are optimistic with a temp
guid that the server echo replaces, and `store.reconcile()` re-reads the chat
list, messages created since the last pass, and the open thread every 30s and
after every reconnect. The UI never refetches on navigation.

chat.db keeps one chat per address, so one person can be two rows. The store
folds one-to-one chats that share a contact (or an address) into one
conversation (`conversations.rs`): the most recently active chat is the
primary the sidebar lists and sends go to, `state.primary_of` and
`state.merged` map the rest, and the `conversation_*` helpers give the UI the
merged thread, unread and typing state. Message actions use the message's own
`chatGuid`; conversation state (drafts, replying, editing) keys on the primary.

Sends go through an outbox in the store: one at a time, in order, queued
while the connection is down and flushed on reconnect. The queue travels in
the state cache with everything else, so a relaunch while offline puts the
waiting rows back and sends them once the connection is up. A send the server
refused (`TransportError`) fails at once; one the network dropped is retried
a few times. The connection itself is retried with backoff until it comes up.
The reconcile sweep asks for ten messages at a time and keeps paging, so a
long absence catches up progressively instead of in one request that hangs.
It re-reads the open thread before the sweep, so what is on screen never waits
for the backlog. Threads nobody has opened are paged in the background after
every pass (`warm_threads`), newest conversation first, one page at a time with
a gap between them, so opening a chat left alone for days paints from memory
instead of waiting on a fifty-message request. The same pass downloads the
media of the last dozen messages in the fifteen most recent chats
(`warm_media`), so a thread opens with its photos, videos and voice notes on
disk rather than a row of placeholders. `warm_budget` decides what is worth
pulling: images and audio up to 25 MB, video up to 60 MB, everything else (a
PDF, a zip) left to a click, which is the bargain the thread already makes.
`warmChats: 0` in the config turns both off.

A chat is read when it is opened or used (a click, a scroll, a key in its
pane), and the read is remembered with the date it covered (`read_locally`,
in the state cache too): chat.db only records a read when the Mac sends the
receipt, so without that the next pass over the chat list would put the dot
back. A newer incoming message makes the chat unread again.

A Focus on the other end shows up twice. The message carries it: chat.db's
`was_delivered_quietly` and `did_notify_recipient` become `delivered_quietly`
and `notified`, and the receipt under the last thing I sent reads "Delivered
Quietly" with a Notify Anyway button (`store.notify_silenced`, which is
`POST /message/:guid/notify`). The person carries it too:
`GET /handle/:address/focus` answers `silenced`, `none` or `unknown`, so
`refresh_focus` asks about the open conversation on open and every minute after,
and the same pass that warms threads keeps an answer for the fifteen most
recent one-to-one chats (`warm_focus`, a ten minute TTL) so the sidebar and the
header can show the moon without each thread being opened first. Answers are
keyed by `focus_key`, one entry per person rather than per number, and
`conversation_focus` reads them back.

None of that is re-pulled after a restart. `StateCache` (`cache.rs`) keeps the
chat list, contacts and the last 100 messages per chat in
`$XDG_CACHE_HOME/messages/state.json`, written debounced and atomically and
parsed on the blocking pool (about 2 ms for a megabyte), and the window paints
from it before the server answers; the messages carry the
`localPath` of anything already downloaded, and the client resolves that path
again for every message it maps, so a file in the attachment cache is never
fetched twice. A message the server sends again keeps the attachment size the
client read from the file header (`measured`): the server's own size ignores
EXIF, so without that the open thread recropped its photos every sweep. What
a restart does cost is one page of the open thread and the sweep since
`savedAt`.

`apps/mac-agent` (`@messages/mac-agent`) runs on the Mac as a launchd user
agent (`scripts/install-mac-agent.sh`, label `es.canarycoders.messages.agent`,
port 1236, token in `~/.config/messages/agent.json`). It decrypts the Find My
caches with keys from `~/.config/messages/findmy/` and serves
`/findmy/friends` and `/findmy/devices`; `/health` says which keys exist and
whether Find My is running. It also keeps `~/.config/messages/prefs.json`, the
pinned and muted state and GIF favorites shared between clients (`PUT /prefs`,
newest entry per chat or gif id wins, an unfavorite kept as a tombstone), and
reports the chats pinned in Messages.app itself, read from
`~/Library/Preferences/com.apple.messages.pinning.plist`. The details panel's
map is `GET /findmy/snapshot` (`maps/snapshot.ts`): `MKMapSnapshotter` centred
on the coordinate, run through a Swift helper compiled once into
`~/.config/messages/bin` and keyed by its source hash, cached ten minutes
by rounded request. No agent means a neutral placeholder, never a tile
server. The client talks to it through `MacAgentClient` in `crates/core/src/agent.rs` when
`config.agent` is set; `is_pinned` in the same file decides between a Mac pin
and a client change, comparing the Mac's list against `pinnedAt`, which only a
pin moves. `updatedAt` is the whole entry's clock, so a draft syncing while you
type used to read as a pin change and unpin the chat.

Nothing on the Mac refreshes those caches unless FindMy.app is running, and it
only refreshes them for about five minutes after it launches: hidden, it then
goes quiet. With the app closed `Devices.data` never changes at all and
findmylocateagent keeps only the friend whose push arrived last, which is how a
panel opened on Monday shows Wednesday's location. So `findmy/refresher.ts`
keeps the app running hidden (`open -g -j`) and watches the source mtimes,
restarting it (SIGTERM, since an `osascript` quit would need automation rights
the agent cannot ask for) whenever they stop moving for 45 seconds. A Find My
someone is using is refreshing on its own and is never restarted under them.
Setting `keepFindMyOpen` to false in `agent.json` leaves the app alone.

Both payloads are memoized on their source files' mtimes rather than a TTL, and
`GET /findmy/stream` (`findmy/feed.ts`) watches those files and pushes a
snapshot whenever one changes: `MacAgentClient::stream_findmy` reads that stream
and the store applies it while the details panel is open, so someone who moves
shows up in seconds. The minute poll stays as the fallback, and an answer to it
that arrives after a push has landed is dropped rather than allowed to
overwrite it.

Colours live in `crates/desktop/src/theme.rs`, a GPUI `Global`. A flat JSON of
palette tokens at `~/.config/messages/theme.json` overrides them and is
polled every second off the UI thread, which is how matugen drives the app on
Linux (template in the nix config, `matugen-templates/messages.json.tmpl`).
A change calls `refresh_windows`, which redraws cached views too.

The composer's GIF picker talks to the Klipy GIF API through `KlipyClient` in
`crates/core/src/gifs.rs`, shown only when `config.klipy` (an `apiKey`) is set.

Videos play in the lightbox, not in an external player. `crates/core/src/video.rs`
probes the file with ffprobe (size with the container rotation applied, frame
rate, duration, whether there is sound) and runs two ffmpeg processes per
`Playback`: one decodes the picture to raw BGRA frames at the size the lightbox
shows them, piped through stdout; the other decodes the sound to f32 at the
device's own rate for `audio.rs`, a cpal output stream on its own thread. The
frame task paces itself on the sample frames the device has played (wall time
when there is no track or no device), drops frames it is late for, and hands
the rest to `crates/desktop/src/video.rs` through a bounded channel, where each
becomes a `RenderImage` painted on a canvas and the previous texture is
released. Pausing stops the clock and stops reading the pipes, so ffmpeg
blocks on a full pipe; a seek is a new `Playback` at the position, and the
first frame goes through even while paused so the picture lands. Dropping the
`Playback` kills both processes. On Linux cpal links `alsa-lib`, which the
dev shell provides; without a device the video still plays, silent.

`crates/core/src/assistant.rs` (`CanaryLlmClient`) backs the summarize,
translate and transcribe buttons in the desktop UI; every call is triggered by
a click, never a timer or an incoming message, sends at most the last 200
messages with attachment bytes stripped, and only ever displays its result.

Find My keys come from `manonstreet/findmy-key-extractor`, driven by
`scripts/findmy-keys-mac.sh`. Two things bit us: the extractor needs Apple's
`stat`/`id` (the script puts `/usr/bin` first because Nix coreutils shadow
them), and the decrypted `LocalStorage.db` must have its WAL header bytes
reset to 1 or SQLite refuses a read-only open (`localstorage.ts`).

## Commands

```
cargo run --release -p messages                        # against ~/.config/messages/config.json
MESSAGES_DEMO=1 cargo run --release -p messages        # fixtures, no Mac needed
cargo test --workspace                                 # core tests, then GPUI app tests on the demo transport
scripts/screenshot.sh                                  # screenshots/messages.png from the demo data
bun run agent                                          # the Mac agent, on the Mac
```

Env overrides: `MESSAGES_SERVER_URL` + `MESSAGES_SERVER_PASSWORD`,
`MESSAGES_DEMO=1`, `MESSAGES_FONT`, `MESSAGES_TRACE=1` (first paint,
key-to-paint, thread-open-to-paint, per-second frame and render counts,
granted window decorations). Cargo features: `screenshot` (offscreen Metal
capture, macOS only) and `frame-overlay` (F12 frame timings; it turns on
GPUI's profiler, so it stays out of normal builds). Config lives in
`$XDG_CONFIG_HOME/messages/config.json`, the attachment cache in
`$XDG_CACHE_HOME/messages/attachments`.

On NixOS, build and run inside the dev shell (`nix develop -c cargo run ...`):
it provides pkg-config, the headers, alsa-lib for cpal, and the dlopened
Wayland, Vulkan and fontconfig libraries on `LD_LIBRARY_PATH`. Keep `flake.nix` git-tracked or
the flake is invisible. `scripts/install-linux.sh` installs the desktop entry
and icon so GNOME and Hyprland match the window's app id
(`es.canarycoders.messages`).

Linux boxes: `ssh e1504g` (NixOS, GNOME on `wayland-0`, Intel iGPU) and
`ssh g815` (NixOS, Hyprland on `wayland-1`; dual-boots into Windows as
`desktop-vjmk52d`, `ssh windows`). Their shell is fish, so pipe scripts
through `bash -s`. Sync with `rsync -a --exclude node_modules --exclude .git
--exclude target . <host>:~/Developer/messages-rust/`, never into
`~/Developer/messages` there. `scripts/linux-headless.sh <bin> <png>` runs the
app in its own headless sway and prints RSS and CPU, so nothing touches the
desktop session; GNOME refuses screenshots from outside its own tools. On
Windows the toolchain is rustup plus the VS 2022 C++ build tools, a GUI
process has to be started in the console session (a `schtasks /IT` task), and
GPUI answers the caption hit test from the last mouse move it saw, so a
`WM_NCHITTEST` probe has to move the cursor there first.

## GPUI rules that bit us

- A cached view (`AnyView::cached`) re-renders only when its own entity is
  notified. Anything it reads from another entity in render needs a notify on
  change, and a cached root that only has `flex_grow` collapses to zero
  height (give it `flex_basis(0)` and `h_full`).
- gpui-pre 0.3.6 drops a cached view's window control areas when it reuses
  the view's frame, so the drag strip (`chrome.rs`) lives in the uncached
  root view, painted beneath the panes. The Windows caption hit test sees
  every hitbox under the cursor, so anything clickable in a title row must
  `occlude()` the strip, and the strip must not reach under the caption
  buttons.
- Caption buttons are gpui-kit's `TitleBar`, pinned top-right in the root.
  It draws them on Windows and when Linux is client-decorated (GNOME); under
  server decorations it draws nothing.
- Colour emoji on Linux: cosmic-text falls back to Noto Sans Symbols 2 and
  DejaVu before any colour font and only paints colour for a face named
  `NotoColorEmoji`. `emoji_font.rs` registers the system colour emoji font a
  second time under a name GPUI accepts, mapped rather than copied, and falls
  back to plain rendering on any failure.
- `img()` given a `String` looks it up as an embedded asset. Files and data
  URLs go through `attachments::image_source`, and anything shown smaller
  than its pixels through `sized_image_source` (`stills.rs`), which decodes
  at on-screen size off the UI thread into an LRU. GPUI's SVG renderer has
  no system fonts, so SVG text names the app's UI font.
- GIFs are decoded once per shared file and box size (device pixels, a 64 px
  step), never above the file's own size, and stepped by one clock per entry;
  only the row showing the GIF is notified, and only while the window is
  active. Three decodes run at a time, paints ahead of the warms the thread
  queues for its GIFs, a decode over 48 MB keeps every other frame with the
  delays folded, and a failed decode is retried after a few seconds.
- A lone UTF-16 surrogate in server text is repaired before parsing
  (`repair_lone_surrogates`); graphemes, not bytes, are indexed
  (`first_grapheme`).
- A file already on disk paints at once; a header re-read (`measured`)
  happens beside it, never before it.
- Bubble text is `selectable.rs`, a `StyledText` that registers with the
  window text selection gpui-base runs under gpui-component's `Root` (the
  layer lives there, so a test window has to be wrapped in `Root` too).
  Cmd+C copies through `Root`, Escape clears. Reading order comes from the
  message date, so a drag down the thread copies in thread order.

## Server quirks worth knowing

- macOS 26 chat guids start with `any;`; the service comes from participants.
- `POST /message/query` with `attributedBody` or `payloadData` in `with` is
  slow per message. Ask for 10 at a time; a request for 150 hung the server
  for two minutes.
- Attachments: download without `original=true` so HEIC becomes JPEG and CAF
  audio becomes AAC (labelled mp3). See `download_plan` in `map.rs`. Stickers
  are the exception: an iOS 17 sticker is HEIC with an alpha plane and the
  server's sips conversion flattens it onto black, so the client fetches the
  original and runs it through ffmpeg (`heif_to_png`, alphamerge of the second
  video stream), caching the result as `<guid>.png`.
- Every cache entry is a symlink to a file named by its SHA-1
  (`share_by_content` in `dedupe.rs`), and the store hands back that
  shared path. GPUI keys decoded images on the path string, so the same GIF
  sent forty times is one decode and one set of textures instead of forty;
  previews, tiles and tail cuts key on the file name for the same reason.
  `cached_file` follows only the link itself, never the directories above it,
  because `realpath` would spell the same file two ways on macOS.
- The attachment `width`/`height` the server reports ignore EXIF
  orientation, so a portrait iPhone photo arrives as a landscape box and the
  renderer, which does turn the pixels upright, paints past it into the next
  row. `image_size` in `image.rs` reads the file header, orientation included,
  and `attachment_src` trusts it over the server.
- Editing a message through the helper on macOS 26 calls an `IMChat`
  selector that no longer exists, Messages.app crashes, and the helper is gone
  for 30 s. `capabilities_for` turns `edit` off when the reported macOS major
  is 26 or later. Unsend and tapbacks are fine.
- Private API events (`typing-indicator`, `chat-read-status-changed`) can
  name a chat with the old `iMessage;-;` prefix while chat.db on macOS 26
  says `any;-;`; the store resolves them by identifier.
- FaceTime: `POST /facetime/answer/:uuid` makes the Mac answer, mint a link,
  admit the first joiner and hang up its own side 15 s later
  (bluebubbles-helper#38). With "FaceTime Calling" off in the server settings
  only the legacy `incoming-facetime` event fires and nothing can be answered.
  The FaceTime helper needs `enable_ft_private_api` and does not inject on
  macOS 26 (bluebubbles-server#776).
- Focus status (`GET /handle/:address/focus`) is Monterey and newer and needs
  the private API, so `capabilities_for` gates it on both; the same goes for
  `wasDeliveredQuietly` and `didNotifyRecipient`, which the server also leaves
  out of the message it serializes for a notification. Someone who does not
  share their Focus with this Apple ID answers `unknown`, not an error.
- Scheduled sends (`POST /message/schedule`) are `Transport::schedule_text`,
  `list_scheduled` and `cancel_scheduled`; the server holds and fires them, not
  the client.
