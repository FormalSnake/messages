# Rust client architecture

The Rust rewrite replaces `packages/core` and `apps/desktop`. `apps/mac-agent`
stays TypeScript on the Mac and is reached over HTTP exactly as before. The
feature list to hit is `docs/rust-parity.md`.

## Crates

| Crate | Path | Depends on | Owns |
| --- | --- | --- | --- |
| `messages-core` | `crates/core` | tokio, reqwest (rustls), serde, tokio-tungstenite (rustls), image, parking_lot | model, transport, BlueBubbles client, store, cache, config, every aux client, media derivation. Never depends on gpui. |
| `messages` (bin) | `crates/desktop` | messages-core, gpui-kit 0.6.6 (gpui-pre 0.3.6) | windows, views, theme, motion, input. No network or disk code of its own beyond calling core. |

Contracts that are frozen (change only through the orchestrator):
`crates/core/src/{model,transport,store}.rs` public items, `lib.rs`, both
`Cargo.toml` files. `store.rs` bodies belong to the store chunk.

## Threading model

- `main` builds one tokio multi-thread runtime (2 worker threads, named
  `messages-rt`) before GPUI starts, and keeps it alive for the process.
  Every network call, timer, file read or write, JSON parse, image decode and
  ffmpeg run happens on it. A `reqwest::Client` is built once and cloned into
  every client (BlueBubbles, agent, Klipy, CanaryLLM, map tiles).
- The GPUI foreground thread renders and handles input. It never awaits I/O.
  It reads state with `store.state()` (a `parking_lot` read guard, held only for
  the length of one render, never across an await) and calls store methods:
  the synchronous ones (`set_draft`, `set_replying_to`, `set_editing`, `send`,
  `send_attachment`, `toggle_*`, `dismiss_facetime`, `clear_error`) mutate in
  memory and return in microseconds; the async ones go through
  `store.spawn(async move { store.select_chat(..).await })`.
- Writers hold the write lock only to mutate; no I/O under it. Timers from the
  TS store (`setTimeout`/`setInterval`) become tokio tasks whose `AbortHandle`
  lives in the store's private state.
- Desktop-only work that is not pixels (GIF decode, previews, tiles, posters,
  tail cuts) goes through `messages_core::media::MediaWorker` on the runtime,
  and the result is handed back to the view with `cx.spawn` + `this.update`.

## How a change reaches a view

1. A socket event lands in the transport, which sends a `TransportEvent` on its
   mpsc channel. The store task applies it under the write lock and records the
   `StoreEvent`s it caused (`Chat(guid)`, `Thread(guid)`, `Message{..}`, ...).
   Inside `batch` (a page, a reconcile pass) duplicates are coalesced.
2. After the lock is released the events go out on a `tokio::sync::broadcast`
   channel (capacity 1024). tokio's sync channels are executor-agnostic.
3. `crates/desktop/src/bridge.rs` runs one foreground task:
   `cx.spawn(async move |cx| while let Ok(ev) = rx.recv().await { ... })`. It
   maps member chat guids to their conversation with `conversation_guid` and
   calls `entity.update(cx, |_, cx| cx.notify())` on only the views watching
   that topic. `RecvError::Lagged` notifies every registered view once.
4. Views register in their constructor:
   `bridge.watch(Topic::Chat(primary), cx.entity().downgrade())`. Dead weak
   handles are pruned on dispatch. Topics mirror `StoreEvent`: `ChatList`,
   `Chat(primary)`, `Thread(primary)`, `Message(message_guid)`,
   `Typing(primary)`, `Selection`, `Draft(primary)`, `ComposerMode(primary)`,
   `Connection`, `Contacts`, `Scheduled`, `FaceTime`, `Locations`, `Focus`,
   `Error`, `Exporting`, `GifFavorites`. `Incoming` goes to `app.rs`, which
   posts the desktop notification when the window is not focused.

Granularity this buys: a tapback repaints one bubble entity, a new message in
chat A repaints A's sidebar row and (if open) A's thread list, typing in the
composer repaints the composer only. Chats and messages are `Arc`, so a view
that keeps what it rendered skips work when the pointer is unchanged.

## Performance budget

| Metric | Budget | How |
| --- | --- | --- |
| Cold start to first painted sidebar and thread from `state.json` | under 150 ms | Window opens while the runtime loads the cache in parallel; serde_json parse of ~40 chats x 100 messages is ~10 to 20 ms; images paint as they decode. Switch to postcard only if a profile shows parse above 30 ms. |
| Chat switch | under 1 frame (16 ms at 60 Hz) | Threads are already in memory (warm pass), `select_chat` sets the selection synchronously and the network follows; the thread is a virtualized `list` so only visible rows render. |
| Composer typing latency | under 1 frame | `set_draft` is synchronous and emits `Draft(guid)` only; typing indicator and draft sync are spawned and debounced (3 s idle stop, 2 s sync). The composer's text input keeps its own buffer; the store is told, not asked. |
| Idle CPU | about 0% | No per-frame work while nothing changes: no polling in views, no animation frames unless a motion is running. GIFs step from one clock per shared file that notifies only the bubbles showing that file, only on a frame change, only while in the painted range. theme.json is watched at 1 s on the runtime, not in a view. |
| Memory | bounded | Threads other than the open one keep 100 rows (`trim_messages`). Decoded images keyed on the shared SHA-1 path, one decode per file; GIF animations in an LRU capped by `GifAnimation::byte_size` (64 MB). Sidebar and thread are virtualized. Downloads stream to disk. |
| Socket event to repaint | under 1 frame after arrival | Event applied under a short write lock, one broadcast, one `notify` per affected entity. |

Rules that keep it there: no blocking I/O and no JSON on the foreground thread;
no whole-tree notify (the root view only re-renders on `Selection`,
`Connection` and overlay changes); never clone a thread's `Vec` in render,
index into it under the read guard; derived media is computed once per shared
path and cached on disk.

## Linux is first class

Targets: NixOS with Hyprland (e1504g), GNOME on Wayland, and macOS. Core must
build on Linux with no system TLS: reqwest and tokio-tungstenite use rustls
(ring) with webpki roots, and `cargo tree -i openssl-sys` is empty.
rust_socketio was rejected because it hard-depends on native-tls; `bluebubbles/socket.rs`
implements the socket.io v4 subset BlueBubbles uses.

Platform code sits behind `cfg` in core, with a Linux path for each:

| Concern | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Open URL or file | `open` crate (xdg-open) | `open` crate | `open` crate |
| Clipboard text | GPUI clipboard | GPUI clipboard | GPUI clipboard |
| Clipboard files and images | wl-copy / wl-paste, xclip on X11 (`clipboard.rs`) | osascript | PowerShell |
| Notifications | notify-send with `--action=open=Open` | osascript | PowerShell toast |
| File picker | GPUI `prompt_for_paths` (xdg portal via ashpd) | GPUI | GPUI |
| Audio playback | mpv, ffplay, paplay | afplay | not supported in TS either |

The window uses gpui-kit `TitleBar::window_options()` for client-side
decorations with drag and window controls, app id `es.canarycoders.messages`,
and ships `es.canarycoders.messages.desktop` plus the icon from
`apps/desktop/assets/icon.svg`.

### Linux build

System libraries the desktop crate needs (for `flake.nix`, which today only
serves the gpuix build):

- Build: `pkg-config`, `clang` / `libclang` (bindgen in gpui-pre; set
  `LIBCLANG_PATH`), `libxkbcommon` (xkbcommon crate links it, x11 and wayland
  features), `wayland` (wayland-sys), `fontconfig` (yeslogic-fontconfig-sys),
  `freetype` (freetype-sys via zed-font-kit), `libxcb` and `xorg.libX11`
  (x11rb raw connection).
- Runtime (`LD_LIBRARY_PATH`): `vulkan-loader` (wgpu/ash dlopen),
  `/run/opengl-driver/lib`, `wayland`, `libxkbcommon`, `libxcb`,
  `xorg.libX11`, `xorg.libXcursor`, `xorg.libXi`, `xorg.libXrandr`,
  `fontconfig.lib`, `freetype`.
- Tools the app shells out to: `ffmpeg` (posters, HEIF stickers),
  `wl-clipboard`, `xclip` (X11 only), `libnotify` (notify-send), `mpv`.

## Work split

Each chunk owns its files outright; nobody else edits them. A chunk that needs
something from a file it does not own asks the owner (or the orchestrator for
the frozen contracts). Tests live beside the code they test.

### Core

| Chunk | Owns | Notes |
| --- | --- | --- |
| C1 bluebubbles | `bluebubbles/{mod,client,map,socket}.rs`, `dedupe.rs`, `image.rs` | Port map.ts tests (map.test.ts) first; they are the spec. `repair_lone_surrogates` before every parse. |
| C2 store | `store.rs` (bodies and private state), `conversations.rs`, `cache.rs`, `config.rs`, `demo.rs` | Port store.test.ts against `DemoTransport`. Emits the right `StoreEvent` granularity; this is the performance contract. |
| C3 aux | `agent.rs`, `findmy.rs`, `gifs.rs`, `assistant.rs`, `notify.rs`, `open.rs`, `clipboard.rs`, `format.rs`, `search.rs`, `fuzzy.rs`, `export.rs`, `media.rs`, `windows.rs` | Pure functions have TS tests to port (format, search, fuzzy, gifs, export, agent, assistant, clipboard, image). |

C2 depends on C1 and C3 only through signatures already in place; all three
start at once.

### Desktop

| Chunk | Owns | Notes |
| --- | --- | --- |
| D0 shell (in progress) | `main.rs`, `app.rs`, `bridge.rs`, `theme.rs`, `live_theme.rs`, `motion.rs`, `icons.rs`, `primitives.rs`, `menus.rs`, `confirm.rs`, `connect.rs`, `toast.rs`, `assets/`, `.desktop` file | Runtime and store construction, `Topic` + `bridge.watch`, keyboard actions and their bindings, overlay stack and Escape order, the menu model (`MenuItem` data that screens build), Avatar and buttons. Lands first; others stub against it. |
| D1 sidebar | `sidebar.rs`, `sidebar_row.rs`, `switcher.rs`, `new_chat.rs`, `search_results.rs` | Row per conversation is an entity watching `Chat(primary)`, `Typing`, `Focus`. |
| D2 thread | `thread.rs`, `thread_rows.rs` (grouping, separators, receipts), `bubble.rs`, `attachments.rs`, `gif.rs`, `lightbox.rs`, `reply_thread.rs` | Bubble per message is an entity watching `Message(guid)`. `gif.rs` owns the per-file clock. |
| D3 header and composer | `header.rs`, `details.rs`, `location.rs`, `composer.rs`, `gif_picker.rs`, `scheduled.rs`, `facetime.rs` | Composer watches `Draft`, `ComposerMode`, `Scheduled`; header and details watch `Chat`, `Focus`, `Locations`, `Exporting`. |

`crates/desktop/src/main.rs` declares the modules; D0 adds each chunk's `mod`
line when the chunk's first file lands.
