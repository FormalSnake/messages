//! Port of `apps/desktop/src/ui/composer.tsx`. `app.rs` mounts `Composer` at
//! the bottom of the main pane.
//!
//! Typing latency is the hard budget (`docs/rust-architecture.md`): the text
//! buffer lives entirely in `TextareaState`, which GPUI updates without a
//! store round trip. Only `InputEvent::Change` tells the store, and that call
//! is `set_draft`, which is synchronous and touches only `Draft(guid)`
//! watchers, so a keystroke never wakes the thread or the sidebar.

use std::path::PathBuf;

use std::sync::Arc;

use gpui_kit::component::input::{Input, InputEvent, InputState, MoveUp, Textarea, TextareaState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::conversations::{conversation_guid, conversation_messages};
use messages_core::format::parse_schedule_time;
use messages_core::{Chat, Message, Service};

use crate::app::{AppRoot, Dismiss, root};
use crate::bridge::{Bridge, StoreHandle, Topic, config};
use crate::gif_picker::GifPicker;
use crate::icons::{Icon, IconName};
use crate::menus::{MenuItem, MenuRequest, shortcut};
use crate::motion::{self, DURATION_BASE, Presence};
use crate::primitives::{Button, IconButton, new_input_state};
use crate::scheduled::ScheduledList;
use crate::theme::{Palette, Theme, radius, spacing, type_scale};

/// My own messages stay editable this long (composer.tsx:48).
const EDIT_WINDOW_MS: i64 = 15 * 60_000;
const BANNER_HEIGHT: Pixels = px(34.);

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// The 13 iOS expressive-send effects, bundle id then label
/// (composer.tsx:32-46). The first four animate the bubble; the rest play
/// across the whole screen.
const EFFECTS: [(&str, &str); 13] = [
    ("impact", "Slam"),
    ("loud", "Loud"),
    ("gentle", "Gentle"),
    ("invisible", "Invisible Ink"),
    ("echo", "Echo"),
    ("spotlight", "Spotlight"),
    ("balloons", "Balloons"),
    ("confetti", "Confetti"),
    ("love", "Love"),
    ("lasers", "Lasers"),
    ("fireworks", "Fireworks"),
    ("shootingstar", "Shooting Star"),
    ("celebration", "Celebration"),
];

struct StagedAttachment {
    path: PathBuf,
    name: String,
    is_image: bool,
}

fn stage(path: PathBuf) -> StagedAttachment {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "attachment".to_owned());
    let is_image =
        matches!(path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref(), Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "bmp"));
    StagedAttachment { path, name, is_image }
}

pub struct Composer {
    input: Entity<TextareaState>,
    _input_sub: Subscription,
    current_chat: Option<String>,
    staged: Vec<StagedAttachment>,
    effect: Option<&'static str>,
    path_field: Option<Entity<InputState>>,
    schedule_field: Option<Entity<InputState>>,
    schedule_error: Option<String>,
    clipboard_notice: bool,
    gif_picker: Option<Entity<GifPicker>>,
    scheduled: Entity<ScheduledList>,
    /// The store's draft as this view last saw or wrote it; a different value
    /// in the store (a send, an edit, another client) is loaded into the field.
    last_draft: String,
    /// Chat, reply target and edit target the field was last focused for.
    focused_for: (Option<String>, Option<String>, Option<String>),
    reply_shown: Presence<Arc<Message>>,
    edit_shown: Presence<Arc<Message>>,
}

impl Composer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade();
        Bridge::watch(cx, Topic::Selection, weak.into());
        let input = cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 8).submit_on_enter(true).placeholder("iMessage"));
        let sub = cx.subscribe(&input, |this: &mut Self, _state, event: &InputEvent, cx| match event {
            InputEvent::Change => this.sync_draft(cx),
            InputEvent::PressEnter { shift: false, .. } => this.send(cx),
            _ => {}
        });
        let scheduled = cx.new(|cx| ScheduledList::new(String::new(), window, cx));

        Self {
            input,
            _input_sub: sub,
            current_chat: None,
            staged: Vec::new(),
            effect: None,
            path_field: None,
            schedule_field: None,
            schedule_error: None,
            clipboard_notice: false,
            gif_picker: None,
            scheduled,
            last_draft: String::new(),
            focused_for: (None, None, None),
            reply_shown: Presence::new(DURATION_BASE),
            edit_shown: Presence::new(DURATION_BASE),
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.input.focus_handle(cx), cx);
    }

    fn store(cx: &App) -> Option<messages_core::MessagesStore> {
        cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone())
    }

    /// Detects a chat switch and reloads the draft/banners for the new one.
    /// Called at the top of every render, which is where `Topic::Selection`
    /// notifications land; a no-op when nothing changed.
    fn sync_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = Self::store(cx) else { return };
        let state = store.state();
        let primary = state.selected_chat.as_deref().map(|guid| conversation_guid(&state.grouping, guid).to_owned());
        if primary == self.current_chat {
            return;
        }
        let draft = primary.as_deref().and_then(|guid| state.drafts.get(guid)).cloned().unwrap_or_default();
        let is_sms = primary.as_deref().and_then(|guid| state.chat(guid)).is_some_and(|chat| chat.service != Service::IMessage);
        drop(state);
        self.current_chat = primary.clone();
        self.staged.clear();
        self.effect = None;
        let placeholder: SharedString = if is_sms { "Text message".into() } else { "iMessage".into() };
        self.last_draft = draft.clone();
        self.input.update(cx, |state, cx| {
            state.set_value(draft, window, cx);
            state.set_placeholder(placeholder, window, cx);
        });
        if let Some(guid) = &primary {
            let weak = cx.entity().downgrade();
            Bridge::watch(cx, Topic::Draft(guid.clone()), weak.clone().into());
            Bridge::watch(cx, Topic::ComposerMode(guid.clone()), weak.clone().into());
            Bridge::watch(cx, Topic::Chat(guid.clone()), weak.into());
            let guid = guid.clone();
            self.scheduled.update(cx, |scheduled, cx| scheduled.set_chat(guid, cx));
        }
    }

    /// Loads a draft the store changed behind the field's back (a send clears
    /// it, starting an edit fills it), and refocuses the field whenever the
    /// chat, reply or edit changes (composer.tsx:138-140).
    fn sync_store(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = Self::store(cx) else { return };
        let Some(chat) = self.current_chat.clone() else { return };
        let (draft, reply, edit, is_sms) = {
            let state = store.state();
            (
                state.drafts.get(&chat).cloned().unwrap_or_default(),
                state.replying_to.get(&chat).cloned(),
                state.editing.get(&chat).cloned(),
                state.chat(&chat).is_some_and(|chat| chat.service != Service::IMessage),
            )
        };
        if draft != self.last_draft {
            self.last_draft = draft.clone();
            if self.input.read(cx).value().as_ref() != draft.as_str() {
                self.input.update(cx, |state, cx| state.set_value(draft, window, cx));
            }
        }
        let mode = (Some(chat), reply, edit.clone());
        if mode != self.focused_for {
            let placeholder: SharedString = if edit.is_some() {
                "Edit message".into()
            } else if is_sms {
                "Text message".into()
            } else {
                "iMessage".into()
            };
            self.input.update(cx, |state, cx| state.set_placeholder(placeholder, window, cx));
            self.focused_for = mode;
            window.focus(&self.input.focus_handle(cx), cx);
        }
    }

    /// Escape: the schedule panel, the path field, the GIF picker, then the
    /// reply and edit banners (composer.tsx:422-425). Anything else is the
    /// root's (menus, the info panel).
    fn on_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.schedule_field.take().is_some() {
            self.schedule_error = None;
        } else if self.path_field.take().is_some() {
        } else if self.gif_picker.take().is_some() {
        } else if let (Some(store), Some(chat)) = (Self::store(cx), self.current_chat.clone()) {
            let (replying, editing) = {
                let state = store.state();
                (state.replying_to.contains_key(&chat), state.editing.contains_key(&chat))
            };
            if !replying && !editing {
                cx.propagate();
                return;
            }
            if replying {
                store.set_replying_to(&chat, None);
            }
            if editing {
                store.set_editing(&chat, None);
            }
        } else {
            cx.propagate();
            return;
        }
        window.focus(&self.input.focus_handle(cx), cx);
        cx.notify();
    }

    /// Up on an empty draft edits my last message still inside the edit
    /// window (composer.tsx:426, 239-244).
    fn on_up(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.input.read(cx).value().is_empty() {
            return false;
        }
        let Some(store) = Self::store(cx) else { return false };
        let Some(chat) = self.current_chat.clone() else { return false };
        let target = {
            let state = store.state();
            if !state.capabilities.edit {
                return false;
            }
            let now = now_ms();
            conversation_messages(&state, &chat)
                .into_iter()
                .rev()
                .find(|message| message.from_me && message.error.is_none() && message.date_retracted.is_none() && message.attachments.is_empty() && now - message.date < EDIT_WINDOW_MS)
                .map(|message| message.guid.clone())
        };
        let Some(target) = target else { return false };
        store.set_editing(&chat, Some(&target));
        true
    }

    fn sync_draft(&mut self, cx: &mut Context<Self>) {
        let Some(store) = Self::store(cx) else { return };
        let Some(chat) = &self.current_chat else { return };
        let text = self.input.read(cx).value().to_string();
        if text != self.last_draft {
            self.last_draft = text.clone();
            store.set_draft(chat, &text);
        }
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let Some(store) = Self::store(cx) else { return };
        let Some(chat) = self.current_chat.clone() else { return };
        let text = self.input.read(cx).value().to_string();
        if !text.trim().is_empty() {
            store.send(&chat, &text, self.effect);
        }
        for attachment in self.staged.drain(..) {
            store.send_attachment(&chat, &attachment.path);
        }
        self.effect = None;
        cx.notify();
    }

    fn stage_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.staged.extend(paths.into_iter().map(stage));
        cx.notify();
    }

    fn open_file_picker(&mut self, cx: &mut Context<Self>) {
        let options = PathPromptOptions { files: true, directories: false, multiple: true, prompt: None };
        let rx = cx.prompt_for_paths(options);
        cx.spawn(async move |this, cx| match rx.await {
            Ok(Ok(Some(paths))) => {
                let _ = this.update(cx, |this, cx| this.stage_paths(paths, cx));
            }
            Ok(Ok(None)) => {}
            // No native picker on this system; fall back to the typed-path field.
            failure => {
                if let Ok(Err(error)) = &failure {
                    eprintln!("picker: {error}");
                }
                let _ = this.update(cx, |this, cx| this.want_path_field(cx));
            }
        })
        .detach();
    }

    fn want_path_field(&mut self, cx: &mut Context<Self>) {
        if self.path_field.is_some() {
            return;
        }
        let Some(window) = cx.active_window() else { return };
        let this = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    let field = new_input_state(window, cx, "Path to a file, for example ~/Pictures/photo.jpg", false);
                    window.focus(&field.focus_handle(cx), cx);
                    this.path_field = Some(field);
                    cx.notify();
                });
            });
        });
    }

    fn paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(store) = Self::store(cx) else { return };
        let (tx, rx) = tokio::sync::oneshot::channel();
        store.spawn(async move {
            let paths = messages_core::clipboard::clipboard_attachments().await;
            let _ = tx.send(paths);
        });
        cx.spawn(async move |this, cx| {
            let Ok(paths) = rx.await else { return };
            if paths.is_empty() {
                let _ = this.update(cx, |this, cx| {
                    this.clipboard_notice = true;
                    cx.notify();
                });
                cx.background_executor().timer(std::time::Duration::from_millis(2500)).await;
                let _ = this.update(cx, |this, cx| {
                    this.clipboard_notice = false;
                    cx.notify();
                });
            } else {
                let _ = this.update(cx, |this, cx| this.stage_paths(paths, cx));
            }
        })
        .detach();
    }

    fn remove_staged(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.staged.len() {
            self.staged.remove(index);
            cx.notify();
        }
    }

    fn schedule_at(&mut self, send_at: i64, cx: &mut Context<Self>) {
        let Some(store) = Self::store(cx) else { return };
        let Some(chat) = self.current_chat.clone() else { return };
        let text = self.input.read(cx).value().to_string();
        let task_store = store.clone();
        store.spawn(async move { task_store.schedule_send(&chat, &text, send_at).await });
    }

    fn submit_schedule_panel(&mut self, cx: &mut Context<Self>) {
        let Some(field) = self.schedule_field.clone() else { return };
        let input = field.read(cx).value().to_string();
        match parse_schedule_time(&input, now_ms()) {
            Ok(at) => {
                self.schedule_field = None;
                self.schedule_error = None;
                self.schedule_at(at, cx);
            }
            Err(error) => {
                self.schedule_error = Some(error);
                cx.notify();
            }
        }
    }

    fn toggle_gif_picker(&mut self, anchor: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        if self.gif_picker.take().is_some() {
            cx.notify();
            return;
        }
        let Some(chat) = self.current_chat.clone() else { return };
        let weak = cx.entity().downgrade();
        let picker = cx.new(|cx| {
            GifPicker::new(
                chat,
                anchor,
                move |_window: &mut Window, cx: &mut App| {
                    let _ = weak.update(cx, |this, cx| {
                        this.gif_picker = None;
                        cx.notify();
                    });
                },
                window,
                cx,
            )
        });
        self.gif_picker = Some(picker);
        cx.notify();
    }

    /// Opens `items` through the root's overlay stack, so Escape and a click
    /// outside close it like every other menu.
    fn open_menu(&mut self, position: Point<Pixels>, items: Vec<MenuItem>, above: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(app) = root(cx) else { return };
        let request = MenuRequest::at(position, items);
        AppRoot::open_menu(&app, if above { request.above() } else { request }, window, cx);
    }

    /// Attach menu: "Choose files…", "Paste from clipboard", "Type a path…".
    fn open_attach_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let choose = weak.clone();
        let paste = weak.clone();
        let path = weak.clone();
        let items = vec![
            MenuItem::item("Choose files…", move |_window, cx| {
                let _ = choose.update(cx, |this, cx| this.open_file_picker(cx));
            })
            .icon(IconName::File),
            MenuItem::item("Paste from clipboard", move |_window, cx| {
                let _ = paste.update(cx, |this, cx| this.paste_from_clipboard(cx));
            })
            .icon(IconName::Copy)
            .shortcut(shortcut("V", true, false)),
            MenuItem::item("Type a path…", move |window, cx| {
                let _ = path.update_in(cx, |this, window, cx| {
                    let field = new_input_state(window, cx, "Path to a file, for example ~/Pictures/photo.jpg", false);
                    window.focus(&field.focus_handle(cx), cx);
                    this.path_field = Some(field);
                    cx.notify();
                });
                let _ = window;
            })
            .icon(IconName::Edit),
        ];
        self.open_menu(position, items, true, window, cx);
    }

    fn add_typed_path(&mut self, cx: &mut Context<Self>) {
        let Some(field) = self.path_field.take() else { return };
        let raw = field.read(cx).value().to_string();
        let expanded = match raw.strip_prefix('~') {
            Some(rest) => std::env::var("HOME").map(|home| PathBuf::from(home).join(rest.trim_start_matches('/'))).unwrap_or_else(|_| PathBuf::from(&raw)),
            None => PathBuf::from(&raw),
        };
        if !raw.trim().is_empty() {
            self.stage_paths(vec![expanded], cx);
        } else {
            cx.notify();
        }
    }

    fn open_effect_picker(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let mut items = vec![{
            let weak = weak.clone();
            MenuItem::item("No effect", move |_window, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.effect = None;
                    cx.notify();
                });
            })
        }];
        for (id, label) in EFFECTS {
            let weak = weak.clone();
            items.push(MenuItem::item(label, move |_window, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.effect = Some(id);
                    cx.notify();
                });
            }));
        }
        self.open_menu(position, items, true, window, cx);
    }

    /// Right-click on the send button: quick schedule options plus "Send at…".
    fn open_schedule_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let now = now_ms();
        let weak = cx.entity().downgrade();
        let mut items = Vec::new();
        {
            let weak = weak.clone();
            let at = now + 3_600_000;
            items.push(MenuItem::item("Send in 1 hour", move |_window, cx| {
                let _ = weak.update(cx, |this, cx| this.schedule_at(at, cx));
            }));
        }
        if let Ok(at) = parse_schedule_time("20:00", now).or_else(|_| parse_schedule_time("tomorrow 20:00", now)) {
            let weak = weak.clone();
            items.push(MenuItem::item("Send tonight at 20:00", move |_window, cx| {
                let _ = weak.update(cx, |this, cx| this.schedule_at(at, cx));
            }));
        }
        if let Ok(at) = parse_schedule_time("tomorrow 09:00", now) {
            let weak = weak.clone();
            items.push(MenuItem::item("Send tomorrow at 09:00", move |_window, cx| {
                let _ = weak.update(cx, |this, cx| this.schedule_at(at, cx));
            }));
        }
        {
            let weak = weak.clone();
            items.push(MenuItem::item("Send at…", move |window, cx| {
                let _ = weak.update_in(cx, |this, window, cx| {
                    let field = new_input_state(window, cx, "HH:MM, tomorrow HH:MM, or YYYY-MM-DD HH:MM", false);
                    window.focus(&field.focus_handle(cx), cx);
                    this.schedule_field = Some(field);
                    this.schedule_error = None;
                    cx.notify();
                });
                let _ = window;
            }));
        }
        self.open_menu(position, items, true, window, cx);
    }
}

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::trace::render("Composer");
        self.sync_selection(window, cx);
        self.sync_store(window, cx);
        let palette = Theme::get(cx);
        let Some(store) = Self::store(cx) else {
            return div().w_full().h(px(52.)).flex_shrink_0().border_t_1().border_color(palette.sidebar_border).into_any_element();
        };
        let state = store.state();
        let chat: Option<Chat> = self.current_chat.as_deref().and_then(|guid| state.chat(guid)).map(|arc| (**arc).clone());
        let capabilities = state.capabilities;
        let replying_guid = self.current_chat.as_deref().and_then(|guid| state.replying_to.get(guid)).cloned();
        let editing_guid = self.current_chat.as_deref().and_then(|guid| state.editing.get(guid)).cloned();
        let messages = self.current_chat.as_deref().map(|guid| conversation_messages(&state, guid)).unwrap_or_default();
        let replying_message = replying_guid.as_deref().and_then(|guid| messages.iter().find(|message| message.guid == guid)).cloned();
        let editing_message = editing_guid.as_deref().and_then(|guid| messages.iter().find(|message| message.guid == guid)).cloned();
        let editing = editing_guid.is_some();
        drop(state);
        self.reply_shown.set(replying_message, |this: &mut Self| &mut this.reply_shown, cx);
        self.edit_shown.set(editing_message, |this: &mut Self| &mut this.edit_shown, cx);
        let klipy = config(cx).is_some_and(|config| config.klipy.is_some());

        let is_sms = chat.as_ref().is_some_and(|chat| chat.service != Service::IMessage);
        let send_color = if is_sms { palette.sms } else { palette.accent };
        let ready = self.current_chat.is_some() && (!self.input.read(cx).value().trim().is_empty() || !self.staged.is_empty());

        div()
            .id("composer")
            .debug_selector(|| "composer".into())
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .flex_shrink_0()
            .border_t_1()
            .border_color(palette.sidebar_border)
            .bg(palette.canvas)
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| this.on_escape(window, cx)))
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                if this.on_up(cx) {
                    cx.stop_propagation();
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let keys = &event.keystroke;
                // The field keeps plain Cmd/Ctrl+V for its own text paste, so
                // files and images come in on the shifted chord.
                if keys.key.eq_ignore_ascii_case("v") && keys.modifiers.shift && (keys.modifiers.platform || keys.modifiers.control) {
                    this.paste_from_clipboard(cx);
                }
            }))
            .on_drop::<ExternalPaths>(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
                this.stage_paths(paths.paths().to_vec(), cx);
            }))
            .child(self.scheduled.clone())
            .children(self.reply_shown.current().cloned().map(|message| {
                let who: SharedString = if message.from_me {
                    "yourself".into()
                } else {
                    message.sender.as_ref().map(|h| messages_core::handle_name(h).to_owned()).unwrap_or_else(|| "message".to_owned()).into()
                };
                let quote: SharedString = if !message.text.is_empty() { message.text.clone().into() } else { "Attachment".into() };
                let view = banner(
                    &palette,
                    "reply-banner",
                    format!("Replying to {who}").into(),
                    quote,
                    cx.listener(|this, _, _, cx| {
                        if let (Some(store), Some(chat)) = (Self::store(cx), this.current_chat.clone()) {
                            store.set_replying_to(&chat, None);
                        }
                    }),
                );
                motion::reveal(div().overflow_hidden().flex_shrink_0().px(spacing::X4).pt(spacing::X2).child(view), self.reply_shown.id("reply-reveal"), self.reply_shown.is_open(), BANNER_HEIGHT + spacing::X2)
            }))
            .children(self.edit_shown.current().cloned().map(|message| {
                let view = banner(
                    &palette,
                    "edit-banner",
                    "Editing".into(),
                    message.text.clone().into(),
                    cx.listener(|this, _, _, cx| {
                        if let (Some(store), Some(chat)) = (Self::store(cx), this.current_chat.clone()) {
                            store.set_editing(&chat, None);
                        }
                    }),
                );
                motion::reveal(div().overflow_hidden().flex_shrink_0().px(spacing::X4).pt(spacing::X2).child(view), self.edit_shown.id("edit-reveal"), self.edit_shown.is_open(), BANNER_HEIGHT + spacing::X2)
            }))
            .when(!self.staged.is_empty(), |el| {
                el.child(
                    div().flex().flex_row().flex_wrap().gap(spacing::X2).px(spacing::X4).py(spacing::X2).children(self.staged.iter().enumerate().map(
                        |(index, item)| staged_chip(&palette, item, cx.listener(move |this, _, _, cx| this.remove_staged(index, cx))),
                    )),
                )
            })
            .when(self.clipboard_notice, |el| {
                el.child(
                    div()
                        .px(spacing::X4)
                        .pb(spacing::X1)
                        .text_size(type_scale::CAPTION.font_size)
                        .text_color(palette.tertiary)
                        .child("Nothing to attach on the clipboard"),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(spacing::X2)
                    .px(spacing::X4)
                    .py(spacing::X3)
                    .child(IconButton::new("attach", IconName::Paperclip, "Attach").on_click(cx.listener(|this, _, window, cx| {
                        let position = window.mouse_position();
                        this.open_attach_menu(position, window, cx);
                    })))
                    .when(klipy, |el| {
                        el.child(IconButton::new("gif", IconName::Gif, "GIF").on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                            let mut position = event.mouse_position().unwrap_or_default();
                            position.y -= spacing::X2;
                            this.toggle_gif_picker(position, window, cx)
                        })))
                    })
                    .when(capabilities.effects && chat.as_ref().is_some_and(|chat| chat.service == Service::IMessage), |el| {
                        el.child(
                            IconButton::new("effect", IconName::Effect, "iMessage effects").active(self.effect.is_some()).on_click(cx.listener(
                                |this, _, window, cx| {
                                    let position = window.mouse_position();
                                    this.open_effect_picker(position, window, cx);
                                },
                            )),
                        )
                    })
                    .child(div().flex_grow(1.).min_w(px(0.)).child(Textarea::new(&self.input).bordered(true)))
                    .child(
                        div()
                            .id("send")
                            .debug_selector(|| "send".into())
                            .w(px(28.))
                            .h(px(28.))
                            .rounded(px(14.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .flex_shrink_0()
                            .when(ready, |el| el.bg(send_color))
                            .when(!ready, |el| el.opacity(0.4))
                            .when(ready, |el| el.on_click(cx.listener(|this, _, _, cx| this.send(cx))))
                            .when(ready && capabilities.scheduled_messages, |el| {
                                el.on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                        this.open_schedule_menu(event.position, window, cx);
                                    }),
                                )
                            })
                            .child(Icon::new(if editing { IconName::Check } else { IconName::Send }).size(px(14.)).color(palette.on_accent)),
                    ),
            )
            .when_some(self.path_field.clone(), |el, field| {
                el.child(path_panel(
                    &palette,
                    &field,
                    cx.listener(|this, _, _, cx| this.add_typed_path(cx)),
                    cx.listener(|this, _, _, cx| {
                        this.path_field = None;
                        cx.notify();
                    }),
                ))
            })
            .when_some(self.schedule_field.clone(), |el, field| {
                let weak = cx.entity().downgrade();
                let cancel: std::rc::Rc<dyn Fn(&mut Window, &mut App)> = std::rc::Rc::new(move |_window, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        this.schedule_field = None;
                        this.schedule_error = None;
                        cx.notify();
                    });
                });
                el.child(schedule_panel(&palette, &field, self.schedule_error.clone(), cx.listener(|this, _, _, cx| this.submit_schedule_panel(cx)), cancel))
            })
            .when_some(self.gif_picker.clone(), |el, picker| el.child(picker))
            .into_any_element()
    }
}

fn banner(palette: &Palette, id: &'static str, title: SharedString, body: SharedString, on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> impl IntoElement {
    div()
        .id(id)
        .debug_selector(|| id.into())
        .flex()
        .flex_row()
        .items_center()
        .gap(spacing::X2)
        .h(BANNER_HEIGHT)
        .pl(spacing::X2)
        .pr(spacing::X1)
        .rounded(radius::CONTROL)
        .bg(palette.raised)
        .flex_shrink_0()
        .child(div().w(px(2.)).h(px(26.)).rounded(px(1.)).bg(palette.accent).flex_shrink_0())
        .child(
            div()
                .flex()
                .flex_col()
                .flex_grow(1.)
                .min_w(px(0.))
                .child(div().text_size(type_scale::MICRO.font_size).line_height(type_scale::MICRO.line_height).font_weight(FontWeight::SEMIBOLD).text_color(palette.accent).text_ellipsis().child(title))
                .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).text_ellipsis().child(body)),
        )
        .child(IconButton::new(SharedString::from(format!("{id}-close")), IconName::Close, "Cancel").size(px(12.)).hit(px(24.)).on_click(on_close))
}

fn staged_chip(palette: &Palette, item: &StagedAttachment, on_remove: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> impl IntoElement {
    div()
        .relative()
        .w(px(56.))
        .h(px(56.))
        .rounded(radius::CONTROL)
        .overflow_hidden()
        .bg(palette.raised)
        .flex_shrink_0()
        .when(item.is_image, |el| el.child(img(crate::attachments::sized_image_source(&item.path, px(56.), px(56.), ObjectFit::Cover)).w(px(56.)).h(px(56.)).object_fit(ObjectFit::Cover)))
        .when(!item.is_image, |el| {
            el.flex().items_center().justify_center().p(spacing::X1).child(
                div().flex().flex_col().items_center().gap(px(2.)).child(Icon::new(IconName::File).size(px(16.)).color(palette.secondary)).child(
                    div().text_size(type_scale::MICRO.font_size).line_height(type_scale::MICRO.line_height).text_color(palette.secondary).child(item.name.clone()),
                ),
            )
        })
        .child(
            div()
                .id(ElementId::Name(format!("remove-{}", item.name).into()))
                .absolute()
                .top(px(2.))
                .right(px(2.))
                .w(px(16.))
                .h(px(16.))
                .rounded(px(8.))
                .bg(hsla(0., 0., 0., 0.6))
                .flex()
                .items_center()
                .justify_center()
                .on_click(on_remove)
                .child(Icon::new(IconName::Close).size(px(10.)).color(white())),
        )
}

fn path_panel(
    palette: &Palette,
    field: &Entity<InputState>,
    on_add: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(spacing::X2)
        .px(spacing::X4)
        .py(spacing::X2)
        .bg(palette.raised)
        .child(div().flex_grow(1.).min_w(px(0.)).child(Input::new(field).bordered(true)))
        .child(Button::new("path-add", "Add file").on_click(on_add))
        .child(Button::new("path-cancel", "Cancel").on_click(on_cancel))
}

fn schedule_panel(
    palette: &Palette,
    field: &Entity<InputState>,
    error: Option<String>,
    on_submit: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: std::rc::Rc<dyn Fn(&mut Window, &mut App)>,
) -> impl IntoElement {
    let cancel_for_click = on_cancel.clone();
    let cancel_for_outside = on_cancel.clone();
    div()
        .id("schedule-panel")
        .on_mouse_down_out(move |_, window, cx| cancel_for_outside(window, cx))
        .flex()
        .flex_col()
        .gap(spacing::X1)
        .px(spacing::X4)
        .py(spacing::X2)
        .bg(palette.raised)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(spacing::X2)
                .child(div().flex_grow(1.).min_w(px(0.)).child(Input::new(field).bordered(true)))
                .child(Button::new("schedule-add", "Schedule").on_click(on_submit))
                .child(Button::new("schedule-cancel", "Cancel").on_click(move |_, window, cx| cancel_for_click(window, cx))),
        )
        .when_some(error, |el, error| {
            el.child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.danger).child(error))
        })
}
