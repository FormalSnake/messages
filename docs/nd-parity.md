# NativeDesktop port: parity checklist

Every desktop feature of the GPUI build (`d12b662`, the desktop half of
`docs/rust-parity.md`) against the NativeDesktop build on `nd-port`. A box is ticked only when
the feature was seen working, and the line says how:

- **t3**: `crates/desktop/tests/thread.rs`, the real app on the demo data over nd's automation
  socket (macOS, `ND_REAL=1`).
- **audit**: `crates/desktop/tests/appearance.rs`, nd's appearance audit on the demo data with
  a thread open (AppKit light and dark, GPUI light, dark and matugen).
- **unit**: a unit test in `crates/desktop/src` (named in the line).
- **shot**: seen in a screenshot of the running app on the demo data (macOS).

An unticked box is a loss or not yet seen working, with the reason. The core crate is
unchanged, so the core half of `rust-parity.md` still holds.

Status: **not at parity.** 26 boxes are open, most of them on missing NativeDesktop
APIs (listed at the end). The GPUI build stays the default until they close.

## App shell and layout
- [x] Title "Messages", default 1120x760, min 720x480 (t3: every test opens it; shot)
- [x] Sidebar 300 pt, 248 below a 900 pt window (shot at 1120; width rule in `root.rs`)
- [x] Main pane shows new chat, the open conversation, or the empty state (t3: new chat test)
- [x] Details in the trailing inspector, opened and closed from the toolbar (t3)
- [x] Connect screen when unconfigured or offline with no chats (unit: `connect::tests`; shot of the settings window not taken)
- [x] One store per connection; a settings edit that keeps the connection never reconnects (`root::connection_key`, unit via `settings::tests`)
- [x] Notification click raises the window and opens the chat (nd-system `NotificationClicked` handler; not clicked on a real notification: see open items)
- [ ] Window opens unfocused with `GPUIX_BACKGROUND=1`: nd's `WindowOptions` has no focus flag
- [ ] F12 frame overlay: replaced by nd's dev HUD and `nd diagnose`, not the same readout
- [ ] Bottom inset follows IME insets: open in the GPUI build too

## Connect and settings
- [x] Address and password fields, `^https?://\S+$` plus a password to enable Connect (unit: `connect::tests`)
- [x] Connecting label, error line, "Use Demo Data" on first run, server info with Private API dot (code; shot not taken)
- [x] Settings window (`Cmd+,`) with the connection and every `MessagesSettings` field as native rows; Nix-managed keys read-only (nd `settings_form`)
- [x] `config.toml`, `MESSAGES_<KEY>` overrides, generated home-manager options (unit: `settings::tests::nix_options_are_generated_from_the_settings`)
- [x] Move from `es.canarycoders.messages`: `config.json` settings into `config.toml` on first launch (unit: `migrate::tests`)
- [ ] Password reveal button: nd's `secure_field` has none

## Sidebar
- [x] Search field, debounced 250 ms, operator search, tips, "No Results" note (code; search ran against the demo transport by hand only)
- [x] Pinned strip with avatar, selection ring and unread dot (code; the demo pins nothing, so not seen)
- [x] Rows: unread dot, 44 pt avatar or clustered group avatar, bold title, date, two-line preview, "Typing…" in the accent (shot)
- [x] Selection, keyboard Up/Down and Return: the native table (t3 opens a chat by clicking its row)
- [x] Row menu: Pin, alerts, receipts, read/unread, details, export, delete with confirm (code: `sidebar::chat_menu`; menu not opened in a test)
- [x] Right-click on the sidebar offers New Message (code)
- [x] Footer: status dot and host, waiting count, settings button (shot)
- [x] Virtualized (nd `list`, an `NSTableView`) (t3)
- [ ] Muted bell and Focus moon as icons: nd's icon set has neither, so they are text glyphs (☾, 🔕)
- [ ] Previews clamp at two lines: nd text has no line clamp, so previews are cut at 90 characters

## Switcher and new chat
- [x] Cmd/Ctrl+K palette, fuzzy over conversations, Return opens (t3)
- [x] New chat: suggestions, typed address fallback, chips, Backspace removes the last chip, send creates the chat (t3 for the typed address path)

## Thread
- [x] Bottom-aligned, opens at its newest message, follows the tail (t3)
- [x] Scrolling to the first message and back leaves the last row whole (t3)
- [x] Rows that grow after load (tall screenshots, GIF, video poster) never hide the last row (t3, on Alex's thread, which has all three)
- [x] Older pages load near the top without a jump (`on_near_start`; demo has one page, not exercised)
- [x] Date separators, runs, gaps, group sender names and avatars, event captions (unit: `rows::tests`; shot)
- [x] Receipts incl. Delivered Quietly and Notify Anyway, failed sends with Try Again (unit: `rows::tests`; shot)
- [x] Typing bubble with the stepping dot (code; not seen)
- [x] Reply quotes, click jumps with a highlight, up to twelve pages (shot of the quote; jump not driven)
- [x] Reply threads ("1 reply" opens it, Back) (shot of the link; panel not driven)
- [x] Translation and voice-note transcription through CanaryLLM (code; needs a key)
- [x] Double-click opens the tapback picker; the message menu lists who reacted (code; not driven)
- [x] Message menu: tapbacks, retry, reply, copy, translate, links, attachment, edit, undo send (code: `message::message_menu`)
- [x] Send (t3: sending shows the bubble and clears the field)
- [ ] Rows newer than open fade in; late tapbacks fade in: nd has no element animation
- [ ] Timestamp column on hover: nd has no hover; the bubble's tooltip shows the time instead
- [ ] Swipe right to reply: mapped to nd's page swipe, which only fires with "Swipe between pages" on

## Bubbles and attachments
- [x] Run corners, tail on a run's last bubble, emoji-only large, reactions on the bubble (nd `chat_bubble`; shot)
- [x] Sent text reads at 4.5:1 (audit, after nd's `accent_fill`)
- [x] Images sized from the attachment before they load, decoded at display size, GIFs animate (shot; t3 tail test)
- [x] Photo grid with "+N" (code; demo grid seen as two tiles in a shot)
- [x] Video poster with play mark and duration, click opens the system player (shot)
- [x] Voice note pill with play/stop (core player, as before) (code)
- [x] Stickers, file pills, link cards (code; link card in a shot of Ben's chat not taken)
- [x] Over-25 MB pictures wait for a click (code)
- [ ] Rich text: bold, italic, underline, strikethrough, mentions and links in the accent: nd text, labels and `chat_bubble` take one plain string
- [ ] Big/small text effects: same reason
- [ ] Subject line bold: same reason
- [ ] SMS bubbles in green: `chat_bubble` has no fill other than the accent
- [ ] Tails on picture and media blocks, picture tail cut: `chat_bubble` draws the only tail
- [ ] Demo photos (SVG data URLs) render: nd's image pipeline has no SVG; real attachments are raster
- [ ] File and video marks are text glyphs, not icons: no standalone icon element

## Composer
- [x] Draft per conversation, Return sends, grows to eight lines, pasted and dropped files stage (nd `composer`; t3 send)
- [x] Reply and edit banners, Escape clears them, Up edits my last message (code)
- [x] Attach through the native file picker; Cmd/Ctrl+Shift+V pastes files from the clipboard (code)
- [x] GIF picker with search, favorites and trending (code; needs a Klipy key)
- [x] Effect picker, scheduled sends list, send-later options and typed time (code)
- [x] Autofocus on chat, reply or edit change (code)
- [ ] Send button turns into a check while editing and green for SMS: nd's composer has no send-button variant
- [ ] "Type a path…" fallback when the picker fails: dropped, the picker exists on every target nd runs on

## Header, details, FaceTime, Find My
- [x] Title, 30 pt avatar, subtitle with service and "Sharing location", Focus moon (shot)
- [x] Catch Me Up summary card (code; needs a key)
- [x] FaceTime button and incoming-call banner with Answer, Join, Decline, Dismiss (code)
- [x] Details: switches for pin, alerts, receipts; mark unread; people with copy and remove; add and rename for groups; export, leave, delete (t3 opens it; shot)
- [x] Gallery of photos and files, three downloads at a time, Load Older (t3 sees "Photos and Files")
- [x] Find My map snapshot from the Mac agent per person (code; needs the agent)
- [x] Details scroll on their own (inspector pane)

## Lightbox
- [x] Every picture and video of the thread, previous and next (buttons, Left/Right, j/k), Copy, Open, Close (code; not driven)
- [ ] In-app video playback: nd has no surface to paint decoded frames on (`canvas` paints nothing yet); videos open in the system player
- [ ] Fade in and out: no element animation

## System
- [x] Desktop notifications for messages while in the background (nd-system; not seen on screen)
- [x] Tray / menu bar item with the unread count and unread chats (nd-system; not seen on screen)
- [x] Single instance: a second launch raises the first (nd-ipc; not exercised)
- [x] Theme from nd-theme, live matugen `theme.json` on Linux (audit's matugen variants)
- [ ] `MESSAGES_FONT`: nd's theme has no app font override
- [ ] Icons on Linux: nd's GPUI backend draws a ring for every icon outside its few glyphs (settings, compose, info, clock, mail)

## Platforms and packaging
- [x] macOS (AppKit) build and run (t3, audit)
- [ ] Linux on g815: `cargo build -p messages` finishes; tests, the headless run and the audit there did not run, the disk was full
- [ ] Windows: nd's Windows backend has never run
- [x] App id `dev.formalsnake.messages`, desktop entry and icon renamed, install script removes the old id's entry (code)
- [ ] Nix package: needs NativeDesktop as a git dependency at a published rev; the home-manager module is generated but the package still builds the path dependency
- [ ] `scripts/screenshot.sh`: removed with the GPUI `screenshot` feature; nd's automation `screenshot` replaces it in tests

## NativeDesktop gaps behind the open boxes
Fixed on `port-messages-fixes` in the NativeDesktop repo (this port builds against it):
a bottom-aligned list opened at its top (column setup and resizes were read as user scrolls);
`set_items` with unchanged rows re-rendered its owner forever; an action dispatched from a
click handler was dropped (the window is lent to the handler); semantic scroll on an AppKit
list; screenshots of a scaffold window were blank and upside down; sent bubbles under 4.5:1;
the Linux app id was the package name, not the bundle id.

Still open: rich text; element animation; hover; a paintable canvas or video surface; SVG
images; icons on the GPUI backend and a standalone icon element; a `chat_bubble` fill
variant; a send-button variant on the composer; an app font override; a focus API by element
id (the port searches the semantic tree); `WindowOptions` focus flag; an out-of-tree app
template that carries the GPUI patch table and the Apple linker setting; offscreen
`NSTableView` rows and collapsed panes still reported visible to automation; list rows
have no semantic click on AppKit (only a pointer click selects one) and toolbar items report
bounds in their own coordinates.
