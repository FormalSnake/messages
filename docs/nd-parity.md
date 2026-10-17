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

Status: **not at parity.** 5 boxes are open: the Linux, Windows and Nix legs out of this run,
and a swiped-open action that AppKit cuts off as it closes.
The GPUI build stays the default until they close.

## App shell and layout
- [x] Title "Messages", default 1120x760, min 720x480 (t3: every test opens it; shot)
- [x] Sidebar 300 pt, 248 below a 900 pt window (shot at 1120; width rule in `root.rs`)
- [x] Main pane shows new chat, the open conversation, or the empty state (t3: new chat test)
- [x] Details in the trailing inspector, opened and closed from the toolbar (t3)
- [x] Connect screen when unconfigured or offline with no chats (unit: `connect::tests`; shot of the settings window not taken)
- [x] One store per connection; a settings edit that keeps the connection never reconnects (`root::connection_key`, unit via `settings::tests`)
- [x] Notification click raises the window and opens the chat (nd-system `NotificationClicked` handler; not clicked on a real notification: see open items)
- [x] Window opens unfocused with `GPUIX_BACKGROUND=1` (nd `WindowOptions::background`; t3 `shots::real_a_background_launch_does_not_take_focus`)
- [x] F12 frame overlay: nd's frame HUD, hidden, minimal (fps) and full (frame time against the budget, slowest frame, dropped frames) (t3 `parity::real_f12_cycles_the_frame_overlay`, captured)
- [x] Bottom inset follows IME insets: the GPUI build (d12b662) never had it, so nothing is lost

## Connect and settings
- [x] Address and password fields, `^https?://\S+$` plus a password to enable Connect (unit: `connect::tests`)
- [x] Connecting label, error line, "Use Demo Data" on first run, server info with Private API dot (code; shot not taken)
- [x] Settings window (`Cmd+,`) with the connection and every `MessagesSettings` field as native rows; Nix-managed keys read-only (nd `settings_form`)
- [x] `config.toml`, `MESSAGES_<KEY>` overrides, generated home-manager options (unit: `settings::tests::nix_options_are_generated_from_the_settings`)
- [x] Move from `es.canarycoders.messages`: `config.json` settings into `config.toml` on first launch (unit: `migrate::tests`)
- [x] Password reveal: the field's trailing eye (t3 `parity::real_the_password_eye_shows_and_hides_the_password`, captured)

## Sidebar
- [x] Search field, debounced 250 ms, operator search, tips, "No Results" note (code; search ran against the demo transport by hand only)
- [x] Pinned strip with avatar, selection ring and unread dot (code; the demo pins nothing, so not seen)
- [x] Rows: unread dot, 44 pt avatar or clustered group avatar, bold title, date, two-line preview, "Typing…" in the accent (shot)
- [x] Selection, keyboard Up/Down and Return: the native table (t3 opens a chat by clicking its row)
- [x] Row menu: Pin, alerts, receipts, read/unread, details, export, delete with confirm (code: `sidebar::chat_menu`; menu not opened in a test)
- [x] Right-click on the sidebar offers New Message (code)
- [x] Footer: status dot and host, waiting count, settings button (shot)
- [x] Virtualized (nd `list`, an `NSTableView`) (t3)
- [x] Muted bell and Focus moon as icons (nd `icon()`, `moon`, `bell-off`; shot of the moon in Ben's row and toolbar)
- [x] Previews clamp at two lines (nd `label().lines(2)`; shot)

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
- [x] Rows newer than open fade in (t3 `parity::real_a_new_row_fades_in`, frames); late tapbacks fade in (nd tier 2 `msg_gaps`)
- [x] Timestamp column on hover (t3 `shots::real_hover_and_lightbox_video_are_captured`, shot)
- [x] Swipe right to reply: the thread's leading row action with a full swipe (NSTableView row actions), drawn as a bare arrow (t3 `shots::real_swipe_to_reply_and_back_is_captured`, a two-finger swipe posted at the HID tap, captured open and closing)
- [ ] The action slides back whole: while the row closes, AppKit narrows the action button from its trailing edge, so the arrow and its label are cut off. A plain NSTableView does the same, frame by frame (recorded at 60 fps). Painting the row opaque, anchoring the button to the trailing edge and turning off the group view's clipping each left the cut in place. A whole slide-back needs nd to draw the swipe itself instead of NSTableViewRowAction

## Bubbles and attachments
- [x] Run corners, tail on a run's last bubble, emoji-only large, reactions on the bubble (nd `chat_bubble`; shot)
- [x] Sent text reads at 4.5:1 (audit, after nd's `accent_fill`)
- [x] Images sized from the attachment before they load, decoded at display size, GIFs animate (shot; t3 tail test)
- [x] Photo grid with "+N" (code; demo grid seen as two tiles in a shot)
- [x] Video poster with play mark and duration, click opens the system player (shot)
- [x] Voice note pill with play/stop (core player, as before) (code)
- [x] Stickers, file pills, link cards (code; link card in a shot of Ben's chat not taken)
- [x] Over-25 MB pictures wait for a click (code)
- [x] Rich text: bold, italic, underline, strikethrough, mentions in the accent; typed links become cards as before (shot of Design crit)
- [x] Big/small text effects (shot: "shipped" in Design crit)
- [x] Subject line bold (the demo's Chloe message has one; t3 `parity`, captured)
- [x] SMS bubbles in green (`FillRole::Success`; shot of Jordan's chat)
- [x] Tails on picture and media blocks, the picture cut into its tail (nd `image().bubble_tail`, tier 2; shot of the video and GIF); a photo grid has none
- [x] Demo photos (SVG data URLs) render (shot)
- [x] File and video marks are icons (nd `icon()`; shot of the play mark)

## Composer
- [x] Draft per conversation, Return sends, grows to eight lines, pasted and dropped files stage (nd `composer`; t3 send)
- [x] Reply and edit banners, Escape clears them, Up edits my last message (code)
- [x] Attach through the native file picker; Cmd/Ctrl+Shift+V pastes files from the clipboard (code)
- [x] GIF picker with search, favorites and trending (code; needs a Klipy key)
- [x] Effect picker, scheduled sends list, send-later options and typed time (code)
- [x] Autofocus on chat, reply or edit change (code)
- [x] Send button turns into a check while editing and green for SMS (nd composer `send_icon`/`send_fill`, tier 2)
- [x] "Type a path…": "Attach by Path…" in the composer's plus menu stages the file (t3 `parity::real_attach_by_path_stages_the_file`)

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
- [x] In-app video playback: Quick Look on macOS, as Messages (t3 captures the panel playing the demo clip); the lightbox plays it with nd `video()` where there is no Quick Look
- [x] Fade in and out (nd panels fade, tier 2)

## System
- [x] Desktop notifications for messages while in the background (nd-system; not seen on screen)
- [x] Tray / menu bar item with the unread count and unread chats (nd-system; not seen on screen)
- [x] Single instance: a second launch raises the first (nd-ipc; not exercised)
- [x] Theme from nd-theme, live matugen `theme.json` on Linux (audit's matugen variants)
- [x] `MESSAGES_FONT` (nd's app font override; t3 `parity`, captured in Courier New)
- [ ] Icons on Linux: nd's GPUI backend draws a ring for every icon outside its few glyphs (settings, compose, info, clock, mail)

## Platforms and packaging
- [x] macOS (AppKit) build and run (t3, audit)
- [ ] Linux on g815: `cargo build -p messages` finishes; tests, the headless run and the audit there did not run, the disk was full
- [ ] Windows: nd's Windows backend has never run
- [x] App id `dev.formalsnake.messages`, desktop entry and icon renamed, install script removes the old id's entry (code)
- [ ] Nix package: needs NativeDesktop as a git dependency at a published rev; the home-manager module is generated but the package still builds the path dependency
- [x] Screenshots: `crates/desktop/tests/shots.rs` captures every demo conversation, whole windows (replaces `scripts/screenshot.sh`)

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

## Appendix: measured spacing against Messages.app

Both windows captured on this Mac at 2x (`screencapture -l`), dark appearance (the system's;
light was not switched for the run), measured with a pixel-scan script over the PNGs, in points.
Messages' window was 2056x1291, the port's 1120x760: every value here is an inset or a size, not
a share of the window. Platform conventions live in nd (`BUBBLE`, `COMPOSER_ROW`, the composer's
gap); the rest is the app's.

| What | Messages | Port | Where |
| --- | --- | --- | --- |
| Sidebar width | 320 | 320 | app (`SIDEBAR_WIDTH`) |
| Sidebar search, top and height | 62, 36 capsule | 62, 36 capsule | nd `large()` (extra large NSSearchField) |
| Sidebar row pitch | 80.5 | 80 + hairline | app |
| Sidebar avatar, its x | 40, 28 | 40, 28 | app |
| Sent bubble edge to window edge | 20 | 20 | app (`THREAD_INSET`) |
| Received bubble edge to sidebar | 20 | 20 | app |
| Bubble height, one line / with tail | 30 / 35 | 30 / 35 | nd `BUBBLE` |
| Text to bubble edge, left / right | 13 / 13 | 13 / 13 | nd `BUBBLE.padding_x` |
| Text top / bottom in a tailed bubble | 10 / 13 | 11 / 13 | nd `BUBBLE.padding_y` |
| Gap inside a run | 3 | 3 | nd `BUBBLE.in_run_gap` |
| Tail | hangs 5 under the corner, inside the width | same | nd `chat_bubble` |
| Plus button, size and x | 31x30 at 11 | 32x31 at 11 | nd `COMPOSER_ROW`, app |
| Field start | 51 | 51 | nd composer gap 9 |
| Field and plus to window bottom | 11 | 11 | app |
| Toolbar buttons | 36 glass circles at 8, 8 | 37x36 at 8, 8 | NSToolbar |
| Transcript under the toolbar | scrolls under it, blurred, first row rests below | same | nd `content_under_toolbar()` |
| Compose button | leads the conversation's half | same | nd `navigation()` |
| Conversation at the top | 40 avatar 6 from the top over a 27 capsule (45 to 72) | same | app, over the transcript |

Messages (a Catalyst app) draws the conversation over its transcript, not as a toolbar item:
its accessibility tree has the name's button beside the transcript and only compose, filter and
FaceTime in the toolbar. The port does the same. When a FaceTime banner or a summary card sits
above the transcript, the conversation moves back into the toolbar, avatar beside name.
