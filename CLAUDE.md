# Messages

iMessage client for Linux. The Mac is the gateway: the open-source BlueBubbles
server (Apache-2.0) reads chat.db, drives Messages.app, and with SIP disabled
injects its helper into Messages.app for the Private API. This repo is the
client side only.

## Layout

Bun workspace. `bun install` at the root links both packages.

- `packages/core` (`@messages/core`): everything that is not pixels. No React,
  no gpuix. Model types (`model.ts`), the `Transport` interface
  (`transport.ts`), the BlueBubbles implementation (`bluebubbles/client.ts`,
  `bluebubbles/map.ts`), fixtures (`demo.ts`), the app store (`store.ts`),
  config, formatting, notifications, clipboard, URL opening. A second frontend
  imports this package and gets the whole backend.
- `apps/desktop` (`@messages/desktop`): the gpuix (React on GPUI) window.
  `app.tsx` is the entry; `src/ui/` holds the screens. The only React-facing
  piece of the store is `src/ui/use-app-state.ts`.

The seam between the two is `MessagesStore`: the frontend calls its methods and
subscribes to `AppState` snapshots. Nothing in the UI talks to the transport
directly except through the store (search and FaceTime go through
`store.transport` on purpose, they own no state).

## Why a store and a reconcile loop

BlueBubbles' own client shows stale threads until you switch chats. Here every
socket event lands in one in-memory store, sends are optimistic with a temp
guid that the server echo replaces, and `store.reconcile()` re-reads the chat
list, messages created since the last pass, and the open thread every 30s and
after every reconnect. The UI never refetches on navigation.

chat.db keeps one chat per address, so one person can be two rows. The store
folds one-to-one chats that share a contact (or an address) into one
conversation (`conversations.ts`): the most recently active chat is the
primary the sidebar lists and sends go to, `state.primaryOf` and
`state.merged` map the rest, and the `conversation*` helpers give the UI the
merged thread, unread and typing state. Message actions use the message's own
`chatGuid`; conversation state (drafts, replying, editing) keys on the primary.

Sends go through an outbox in the store: one at a time, in order, queued
while the connection is down and flushed on reconnect. A send the server
refused (`TransportError`) fails at once; one the network dropped is retried
a few times. The connection itself is retried with backoff until it comes up.
The reconcile sweep asks for ten messages at a time and keeps paging, so a
long absence catches up progressively instead of in one request that hangs.
It re-reads the open thread before the sweep, so what is on screen never waits
for the backlog. Threads nobody has opened are paged in the background after
every pass (`warmThreads`), newest conversation first, one page at a time with
a gap between them, so opening a chat left alone for days paints from memory
instead of waiting on a fifty-message request. The same pass downloads the
media of the last dozen messages in the fifteen most recent chats
(`warmMedia`), so a thread opens with its photos, videos and voice notes on
disk rather than a row of placeholders. `warmBudget` decides what is worth
pulling: images and audio up to 25 MB, video up to 60 MB, everything else (a
PDF, a zip) left to a click, which is the bargain the thread already makes.
`warmChats: 0` turns both off.

A Focus on the other end shows up twice. The message carries it: chat.db's
`was_delivered_quietly` and `did_notify_recipient` become `deliveredQuietly`
and `notified`, and the receipt under the last thing I sent reads "Delivered
Quietly" with a Notify Anyway button (`store.notifySilenced`, which is
`POST /message/:guid/notify`). The person carries it too:
`GET /handle/:address/focus` answers `silenced`, `none` or `unknown`, so
`refreshFocus` asks about the open conversation on open and every minute after,
and the same pass that warms threads keeps an answer for the fifteen most
recent one-to-one chats (`warmFocus`, a ten minute TTL) so the sidebar and the
header can show the moon without each thread being opened first. Answers are
keyed by `focusKey`, one entry per person rather than per number, and
`conversationFocus` reads them back.

None of that is re-pulled after a restart. `StateCache` (`cache.ts`) keeps the
chat list, contacts and the last 100 messages per chat in
`$XDG_CACHE_HOME/messages/state.json`, written debounced and atomically, and
the window paints from it before the server answers; the messages carry the
`localPath` of anything already downloaded, and the client resolves that path
again for every message it maps, so a file in the attachment cache is never
fetched twice. A message the server sends again keeps the attachment size the
client read from the file header (`measured`): the server's own size ignores
EXIF, so without that the open thread recropped its photos every sweep. What a restart does cost is one page of the open thread and the
sweep since `savedAt`.

`apps/mac-agent` (`@messages/mac-agent`) runs on the Mac as a launchd user
agent (`scripts/install-mac-agent.sh`, label `es.canarycoders.messages.agent`,
port 1236, token in `~/.config/messages/agent.json`). It decrypts the Find My
caches with keys from `~/.config/messages/findmy/` and serves
`/findmy/friends` and `/findmy/devices`; `/health` says which keys exist and
whether Find My is running. It also keeps `~/.config/messages/prefs.json`, the
pinned and muted state and GIF favorites shared between clients (`PUT /prefs`,
newest entry per chat or gif id wins, an unfavorite kept as a tombstone), and
reports the chats pinned in Messages.app itself, read from
`~/Library/Preferences/com.apple.messages.pinning.plist`. The client talks to
it through `MacAgentClient` in `packages/core/src/agent.ts` when
`config.agent` is set; `isPinned` in the same file decides between a Mac pin
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
snapshot whenever one changes: `MacAgentClient.streamFindMy` reads that stream
and the store applies it while the details panel is open, so someone who moves
shows up in seconds. The minute poll stays as the fallback, and an answer to it
that arrives after a push has landed is dropped rather than allowed to
overwrite it.

Colours live in `apps/desktop/src/ui/theme.ts` (`C`). A flat JSON of palette
tokens at `~/.config/messages/theme.json` overrides them and is polled every
second, which is how matugen drives the app on Linux (template in the nix
config, `matugen-templates/messages.json.tmpl`). The palette is mutated in
place and the tree remounts, so never capture a `C.*` value in a module-level
constant.

The composer's GIF picker talks to the Klipy GIF API through `KlipyClient` in
`packages/core/src/gifs.ts`, shown only when `config.klipy` (an `apiKey`) is set.

`packages/core/src/assistant.ts` (`CanaryLLMClient`) backs the summarize,
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
bun run demo        # desktop window on fixtures, no Mac needed
bun run dev         # desktop window against ~/.config/messages/config.json
bun run test        # core mapper tests, then the GPU-backed app tests (macOS only today)
bun run typecheck
bun run screenshot  # apps/desktop/screenshots/messages.png from the demo data
```

Env overrides: `MESSAGES_SERVER_URL` + `MESSAGES_SERVER_PASSWORD`,
`MESSAGES_DEMO=1`, `MESSAGES_FONT`. Config lives in
`$XDG_CONFIG_HOME/messages/config.json`, the attachment cache in
`$XDG_CACHE_HOME/messages/attachments`.

On NixOS the prebuilt renderer needs its runtime libraries on
`LD_LIBRARY_PATH`; `flake.nix` provides a dev shell that sets it, so run
`nix develop -c bun run demo` there (nix-ld does not help: Nix's bun never
consults `NIX_LD_LIBRARY_PATH`). Keep `flake.nix` git-tracked or the flake is
invisible.

Linux test box: `ssh e1504g` (NixOS, Hyprland on `wayland-1`, bun installed,
Vulkan via the iGPU). Its shell is fish, so pipe scripts through `bash -s`.
Sync with `rsync -a --exclude node_modules --exclude .git . e1504g:~/Developer/messages/`,
then on the box `bun install && WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000 nix develop -c bun run demo`.
`grim tmp/shot.png` captures the screen for a look.

## Mac gateway setup

1. Install the BlueBubbles server on the Mac and grant Full Disk Access.
2. For the Private API (tapbacks, typing, read receipts, replies, edit,
   unsend, effects, group management, FaceTime links): SIP off, then turn on
   Private API in BlueBubbles settings and confirm "helper connected".
   Docs: https://docs.bluebubbles.app/private-api/installation
3. Turn OFF "Encrypt communications" in BlueBubbles; the client does not
   implement its AES envelope.
4. Enter the server address and password in the app's connect screen.

Capabilities are derived from `/server/info` (`private_api` and
`helper_connected`), see `capabilitiesFor` in `model.ts`. The UI hides what the
server cannot do instead of failing on click.

## gpuix rules that bit us

- Every `<text>` needs a `color`; GPUI paints unstyled text black.
- One scroller per column: the sidebar scrolls, the thread is a
  `<virtual-list>`, nothing inside either may scroll.
- Overlays must be `<anchored deferred>` (or SelectContent) to paint above the
  virtual list; a positioned div ends up underneath it.
- `<img src>` takes a file path or data URL, so attachments are downloaded into
  the cache first.
- Lucide icons come from `lucide-static`; `currentColor` is replaced with a
  paint colour before GPUI tints the mask.
- A lone UTF-16 surrogate anywhere in a text prop makes the native batch
  parser reject the whole commit ("unexpected end of hex escape") and React
  then dies with "Should not already be working". Never index a string with
  `[0]` (use `firstGrapheme`), and run server strings through `wellFormed`.
- On Linux an `<img objectFit="cover">` whose scaled bitmap is bigger than
  its box is painted whole, not clipped (a portrait photo in a circle comes
  out as a tall pill); macOS clips it. Contact photos and group icons are
  therefore cut square on disk (`squareThumbnail` in `image.ts`) before the
  renderer sees them.
- Emoji on Linux: cosmic-text's fallback list is hardcoded and puts DejaVu,
  FreeSans and Noto Sans Symbols ahead of any emoji font, and GPUI only treats
  a glyph as emoji when the font's PostScript name is literally
  `NotoColorEmoji`. The nix config (`modules/nixos/mixins/hyprland.nix`)
  drops those fonts and installs Apple Color Emoji under that name. Never
  name an emoji family in a style on Linux: a run that asks for one, by any
  of its names, renders tofu, while an unnamed run reaches the colour font
  through the per-glyph fallback. `FONT_EMOJI` is therefore undefined off
  macOS and only the emoji-only nodes use it.
- A child with a background fill (`backgroundColor` or a gradient) swallows
  the click meant for an ancestor's `onClick`; a border, a shadow, opacity or
  a `<text>` do not. Give such decorations `pointerEvents: 'none'` (avatars,
  dots, badges) or put the handler on the filled element itself.
- Motion is `motion.div` from gpuix, driven natively: it animates `width`,
  `height`, `opacity`, `top/right/bottom/left` and `borderRadius`, nothing
  else (no transforms, no springs, no keyframes) and nothing on unmount.
  `src/ui/motion.tsx` holds the durations and easing plus `usePresence`,
  `useLeaving`, `Fade` and `Reveal`, which keep a closing element mounted
  long enough to animate out. A panel slides by animating a clipping box
  around content of fixed width, so nothing inside reflows mid-slide.
- A content mask is a rectangle. An image inside a box with `overflow: hidden`
  and a corner radius is cut to the box but keeps its square corners, so the
  tail lobe under a photo came out as a square nub. Only the element that
  paints rounds itself: the lobe is a 14x16 `<img>` with its own radius, fed a
  corner cut ffmpeg makes on disk (`generateTailCut`), the same bargain the
  photo tiles already make.

## Server quirks worth knowing

- macOS 26 chat guids start with `any;`; the service comes from participants.
- `POST /message/query` with `attributedBody` or `payloadData` in `with` is
  slow per message. Ask for 10 at a time; a request for 150 hung the server
  for two minutes.
- Attachments: download without `original=true` so HEIC becomes JPEG and CAF
  audio becomes AAC (labelled mp3). See `downloadPlan` in `map.ts`.
- The attachment `width`/`height` the server reports ignore EXIF
  orientation, so a portrait iPhone photo arrives as a landscape box and the
  renderer, which does turn the pixels upright, paints past it into the next
  row. `imageSize` in `image.ts` reads the file header, orientation included,
  and `attachmentSrc` trusts it over the server; `ImageAttachment` reads it
  once per mount for files already in the cache.
- Editing a message through the helper on macOS 26 calls an `IMChat`
  selector that no longer exists, Messages.app crashes, and the helper is gone
  for 30 s. `capabilitiesFor` turns `edit` off when the reported macOS major
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
  the private API, so `capabilitiesFor` gates it on both; the same goes for
  `wasDeliveredQuietly` and `didNotifyRecipient`, which the server also leaves
  out of the message it serializes for a notification. Someone who does not
  share their Focus with this Apple ID answers `unknown`, not an error.
- Scheduled sends (`POST /message/schedule`) are `Transport.scheduleText`,
  `listScheduled` and `cancelScheduled`; the server holds and fires them, not
  the client.
