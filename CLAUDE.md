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

## Server quirks worth knowing

- macOS 26 chat guids start with `any;`; the service comes from participants.
- `POST /message/query` with `attributedBody` or `payloadData` in `with` is
  slow per message. Ask for 10 at a time; a request for 150 hung the server
  for two minutes.
- Attachments: download without `original=true` so HEIC becomes JPEG and CAF
  audio becomes AAC (labelled mp3). See `downloadPlan` in `map.ts`.
- FaceTime: `POST /facetime/answer/:uuid` makes the Mac answer, mint a link,
  admit the first joiner and hang up its own side 15 s later
  (bluebubbles-helper#38). With "FaceTime Calling" off in the server settings
  only the legacy `incoming-facetime` event fires and nothing can be answered.
  The FaceTime helper needs `enable_ft_private_api` and does not inject on
  macOS 26 (bluebubbles-server#776).
