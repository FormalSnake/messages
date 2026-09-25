# Desktop conventions

How a `crates/desktop` view talks to the store, the bridge and the theme.
Read `docs/rust-architecture.md` first for the why; this is the how.

## Registering with the bridge

A view that shows store state watches one or more `Topic`s so it repaints
only when its own data changes, never on every event:
```rust
bridge::Bridge::watch(cx, bridge::Topic::Chat(primary.clone()), cx.entity().downgrade().into());
```
Call this once, in the view's constructor; `Bridge::drain` turns a matching
`StoreEvent` into `cx.notify()` on that entity, so you never read the
broadcast channel yourself. Topics mirror `StoreEvent` one-to-one (`ChatList`,
`Chat(guid)`, `Message(guid)`, `Draft(guid)`, ...); pick the narrowest one.

## Reading state

Get the store from `app::AppRoot` (one per window) and thread a
`MessagesStore` handle down through constructors. Read state with:
```rust
let state = store.state(); // parking_lot read guard; drop before an await
```
Never clone a `Vec` out of `AppState` to iterate; index into it under the
guard. Chats and messages are `Arc`, so keeping the `Arc` you last rendered
and comparing pointers is how a view skips work when nothing changed.

## Calling store actions

Synchronous methods (`set_draft`, `set_replying_to`, `toggle_pin`, `send`, ...)
mutate in memory and return immediately: call them straight from a click or
keystroke handler. Async methods (`select_chat`, `react`, `mark_read`, ...) go
through `store.spawn(async move { store.select_chat(&guid).await })`; never
`.await` a store method directly on the GPUI foreground thread.

## Primitives

Use `primitives::{Button, IconButton, avatar, divider, section_label}` and
`icons::{Icon, IconName}` instead of a hand-rolled styled `div()`; they
already carry the right hover/press washes and focus ring. `menus::MenuItem`
is the data a screen builds for a context menu or tapback picker;
`confirm::ConfirmRequest` is the same for a yes/no dialog. Both render
through `app::AppRoot`'s own overlay stack: open one by updating the root
entity, not by mounting your own overlay.

## Theme access

`theme::Theme::get(cx)` returns the current `Palette` by value (`Copy`); call
it once per render and reuse the local. Never read `theme.json` yourself:
`live_theme.rs` already polls it and calls `Theme::set`, repainting every window.

## The idle-frame rule

No polling in a view, no per-frame `cx.notify()`, no animation unless one is
actually running (idle-CPU budget in `docs/rust-architecture.md`: about 0%).
A background poll belongs on the tokio runtime or the GPUI background
executor, and touches the foreground only when what it polled changed
(`live_theme.rs`: one `stat` a second, a `cx.update` only on a changed mtime).
