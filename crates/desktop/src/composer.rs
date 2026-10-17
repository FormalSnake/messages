//! The message field under the thread: nd's `composer` (the platform's text view, growing to
//! eight lines, Return sends, pasted and dropped files stage), with the reply and edit banners,
//! the scheduled sends, and the GIF, effect and send-later popovers around it. A keystroke only
//! calls `set_draft`, which wakes nothing but the draft's own watchers.

use std::path::PathBuf;
use std::sync::Arc;

use messages_core::conversations::{conversation_guid, conversation_messages};
use messages_core::format::{format_scheduled_for, parse_schedule_time};
use messages_core::gifs::{Gif, KlipyClient, download_gif, favorite_gifs};
use messages_core::{Message, Service};
use nd::components::media::COMPOSER_ROW;
use nd::prelude::*;

use crate::actions::*;
use crate::bridge::{self, Bridge, Topic};
use crate::ui;

/// My own messages stay editable this long.
const EDIT_WINDOW_MS: i64 = 15 * 60_000;
const GIF_CELL: f32 = 84.;
const GIF_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(300);

/// The iOS expressive-send effects: bundle id, then label.
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

actions!(composer, [EditLast, PasteAttachments, ChooseFiles, ShowGifs, ShowEffects, ShowSendLater, AttachByPath]);

/// macOS opens the "+" as a native menu above the button, as Messages does; the drawn backends
/// keep the popover, which their look already gives a menu's insets.
const NATIVE_PLUS_MENU: bool = cfg!(target_os = "macos");

pub const CONTEXT: &str = "Composer";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", EditLast, Some(CONTEXT)),
        KeyBinding::new("secondary-shift-v", PasteAttachments, Some(CONTEXT)),
    ]);
}

#[derive(Clone, Copy, Default, PartialEq)]
enum Popover {
    #[default]
    None,
    Gif,
    Apps,
    Effects,
    Schedule,
    Path,
}

#[derive(Default)]
struct GifState {
    query: SharedString,
    epoch: u64,
    loading: bool,
    items: Vec<Gif>,
}

pub struct Composer {
    chat: Option<String>,
    draft: Draft,
    /// The store's draft as this view last saw or wrote it; a different one is loaded.
    last_draft: String,
    effect: Option<&'static str>,
    popover: Popover,
    schedule_text: SharedString,
    schedule_error: Option<String>,
    /// "Attach by Path": what is typed, and why the last one did not attach.
    path_text: SharedString,
    path_error: Option<String>,
    clipboard_notice: bool,
    gif: GifState,
    klipy: Option<Arc<KlipyClient>>,
    /// The field takes focus once the conversation, reply or edit it is for changes.
    focus_for: Option<(String, Option<String>, Option<String>)>,
}

impl Composer {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let weak = cx.weak_entity();
        for topic in [Topic::Selection, Topic::Scheduled, Topic::GifFavorites, Topic::Connection] {
            Bridge::watch(cx, topic, &weak);
        }
        Self {
            chat: None,
            draft: Draft::default(),
            last_draft: String::new(),
            effect: None,
            popover: Popover::None,
            schedule_text: SharedString::default(),
            schedule_error: None,
            path_text: SharedString::default(),
            path_error: None,
            clipboard_notice: false,
            gif: GifState::default(),
            klipy: None,
            focus_for: None,
        }
    }

    /// Follows the selected conversation and loads drafts the store changed (a send clears
    /// one, starting an edit fills it).
    fn sync(&mut self, cx: &mut Context<Self>) {
        let Some(store) = bridge::store(cx) else { return };
        let (primary, draft) = {
            let state = store.state();
            let primary =
                state.selected_chat.as_deref().map(|guid| conversation_guid(&state.grouping, guid).to_owned());
            let draft = primary.as_deref().and_then(|guid| state.drafts.get(guid)).cloned().unwrap_or_default();
            (primary, draft)
        };
        if primary != self.chat {
            self.chat = primary.clone();
            self.draft = Draft::default();
            self.effect = None;
            self.popover = Popover::None;
            if let Some(guid) = &primary {
                for topic in [Topic::Draft(guid.clone()), Topic::ComposerMode(guid.clone()), Topic::Chat(guid.clone())]
                {
                    {
                        let weak = cx.weak_entity();
                        Bridge::watch(cx, topic, &weak);
                    }
                }
            }
            self.last_draft = String::new();
        }
        if draft != self.last_draft {
            self.last_draft = draft.clone();
            self.draft.text = draft.into();
        }
        if self.klipy.is_none()
            && let Some(klipy) = bridge::config(cx).and_then(|c| c.klipy.clone())
        {
            self.klipy = Some(Arc::new(KlipyClient::new(klipy.api_key, reqwest::Client::new())));
        }
    }

    fn event(&mut self, event: &ComposerEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            ComposerEvent::Attach if NATIVE_PLUS_MENU => {
                self.plus_menu(window, cx);
                return;
            }
            ComposerEvent::Attach => {
                self.popover = if self.popover == Popover::Apps { Popover::None } else { Popover::Apps };
                cx.notify();
                return;
            }
            ComposerEvent::Send => {
                self.send(cx);
                return;
            }
            _ => {}
        }
        self.draft.apply(event);
        if let ComposerEvent::Edited(text) = event
            && let (Some(store), Some(chat)) = (bridge::store(cx), self.chat.clone())
            && text.as_ref() != self.last_draft
        {
            self.last_draft = text.to_string();
            store.set_draft(&chat, text);
        }
        cx.notify();
    }

    pub fn stage_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.draft.apply(&ComposerEvent::Files(paths));
        cx.notify();
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let (Some(store), Some(chat)) = (bridge::store(cx), self.chat.clone()) else { return };
        let editing = store.state().editing.get(&chat).cloned();
        let Some(out) = self.draft.apply(&ComposerEvent::Send) else { return };
        if let Some(target) = editing {
            if let Some(text) = out.text {
                let edit = store.clone();
                crate::rt::detach(async move { edit.edit(&chat, &target, &text).await });
            }
            store.set_editing(&self.chat.clone().unwrap_or_default(), None);
        } else {
            if let Some(text) = &out.text {
                store.send(&chat, text, self.effect);
            }
            for attachment in out.attachments {
                if let Some(path) = attachment.path {
                    store.send_attachment(&chat, &path, None);
                } else if let Some(bytes) = attachment.bytes {
                    // A pasted image has no file yet: it is written to the attachment cache first.
                    let dir = messages_core::config::attachments_dir();
                    let name = attachment.name.to_string();
                    let store = store.clone();
                    let chat = chat.clone();
                    crate::rt::spawn(
                        async move {
                            let path = dir.join(format!("pasted-{}-{name}", ui::now_ms()));
                            tokio::fs::create_dir_all(&dir).await.ok()?;
                            tokio::fs::write(&path, &*bytes).await.ok()?;
                            Some(path)
                        },
                        move |path, _| {
                            if let Some(path) = path {
                                store.send_attachment(&chat, &path, None);
                            }
                        },
                    );
                }
            }
        }
        self.last_draft = String::new();
        self.effect = None;
        cx.notify();
    }

    /// Up on an empty draft edits my last message still inside the edit window.
    fn edit_last(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.draft.text.is_empty() {
            return false;
        }
        let (Some(store), Some(chat)) = (bridge::store(cx), self.chat.clone()) else { return false };
        let target = {
            let state = store.state();
            if !state.capabilities.edit {
                return false;
            }
            let now = ui::now_ms();
            conversation_messages(&state, &chat)
                .into_iter()
                .rev()
                .find(|m| {
                    m.from_me
                        && m.error.is_none()
                        && m.date_retracted.is_none()
                        && m.attachments.is_empty()
                        && now - m.date < EDIT_WINDOW_MS
                })
                .map(|m| m.guid.clone())
        };
        let Some(target) = target else { return false };
        store.set_editing(&chat, Some(&target));
        true
    }

    fn paste_attachments(&mut self, cx: &mut Context<Self>) {
        crate::rt::spawn_for(cx.weak_entity(), messages_core::clipboard::clipboard_attachments(), |this, paths, cx| {
            if paths.is_empty() {
                this.clipboard_notice = true;
                let weak = cx.weak_entity();
                crate::rt::after(std::time::Duration::from_millis(2500), move |cx| {
                    weak.update(cx, |this, cx| {
                        this.clipboard_notice = false;
                        cx.notify();
                    });
                });
            } else {
                this.stage_files(paths, cx);
            }
            cx.notify();
        });
    }

    fn schedule_at(&mut self, at: i64, cx: &mut Context<Self>) {
        let (Some(store), Some(chat)) = (bridge::store(cx), self.chat.clone()) else { return };
        let text = self.draft.text.to_string();
        self.popover = Popover::None;
        cx.notify();
        crate::rt::detach(async move { store.schedule_send(&chat, &text, at).await });
    }

    fn submit_schedule(&mut self, cx: &mut Context<Self>) {
        match parse_schedule_time(&self.schedule_text, ui::now_ms()) {
            Ok(at) => {
                self.schedule_error = None;
                self.schedule_text = SharedString::default();
                self.schedule_at(at, cx);
            }
            Err(error) => {
                self.schedule_error = Some(error);
                cx.notify();
            }
        }
    }

    fn set_gif_query(&mut self, query: SharedString, cx: &mut Context<Self>) {
        self.gif.query = query;
        self.gif.epoch += 1;
        let epoch = self.gif.epoch;
        let weak = cx.weak_entity();
        crate::rt::after(GIF_DEBOUNCE, move |cx| {
            weak.update(cx, |this, cx| {
                if this.gif.epoch == epoch {
                    this.search_gifs(cx);
                }
            });
        });
        cx.notify();
    }

    fn search_gifs(&mut self, cx: &mut Context<Self>) {
        let Some(klipy) = self.klipy.clone() else { return };
        self.gif.loading = true;
        let term = self.gif.query.trim().to_owned();
        let epoch = self.gif.epoch;
        crate::rt::spawn_for(
            cx.weak_entity(),
            async move { if term.is_empty() { klipy.trending(1).await } else { klipy.search(&term, 1).await } },
            move |this, page, cx| {
                if this.gif.epoch == epoch {
                    this.gif.loading = false;
                    this.gif.items = page.map(|page| page.items).unwrap_or_default();
                    cx.notify();
                }
            },
        );
        cx.notify();
    }

    /// Downloads the full GIF and sends it, then closes the picker.
    fn pick_gif(&mut self, gif: Gif, cx: &mut Context<Self>) {
        self.popover = Popover::None;
        cx.notify();
        let (Some(store), Some(chat)) = (bridge::store(cx), self.chat.clone()) else { return };
        let size = (gif.width > 0 && gif.height > 0)
            .then_some(messages_core::image::ImageSize { width: gif.width, height: gif.height });
        crate::rt::spawn(
            async move { download_gif(&reqwest::Client::new(), &gif, &messages_core::config::attachments_dir()).await },
            move |path, _| {
                if let Ok(path) = path {
                    store.send_attachment(&chat, &path, size);
                }
            },
        );
    }

    fn banner(
        &self,
        id: &str,
        title: String,
        body: String,
        cx: &mut Context<Self>,
        clear: fn(&messages_core::MessagesStore, &str),
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let chat = self.chat.clone();
        h_flex()
            .id(id.to_owned())
            .gap_2()
            .px_3()
            .py_1()
            .mx_4()
            .mt_2()
            .rounded_lg()
            .bg(theme.surface.raised)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(text(title).font_weight(FontWeight::SEMIBOLD).text_size(theme.type_scale.caption))
                    .child(
                        text(ui::clamp_preview(&body, 80))
                            .text_size(theme.type_scale.caption)
                            .text_color(theme.text.secondary),
                    ),
            )
            .child(icon_button(format!("{id}-close"), Icon::X).tooltip("Cancel").on_click(move |_, _, cx| {
                if let (Some(store), Some(chat)) = (bridge::store(cx), chat.clone()) {
                    clear(&store, &chat);
                }
            }))
            .into_any_element()
    }

    fn gif_popover(&self, cx: &mut Context<Self>) -> AnyElement {
        let favorites = bridge::store(cx).map(|store| favorite_gifs(&store.state().gif_favorites)).unwrap_or_default();
        let favorite_ids: std::collections::HashSet<String> = favorites.iter().map(|g| g.id.clone()).collect();
        let show_favorites = self.gif.query.trim().is_empty();
        let cell = |gif: &Gif, cx: &mut Context<Self>| {
            let pick = gif.clone();
            let fav = gif.clone();
            let favorited = favorite_ids.contains(&gif.id);
            div()
                .id(format!("gif-{}", gif.id))
                .w(px(GIF_CELL))
                .h(px(GIF_CELL))
                .rounded_md()
                .on_tap(cx.listener(move |this, _: &TapEvent, _, cx| this.pick_gif(pick.clone(), cx)))
                .child(image(ImageSource::url(gif.preview_url.clone())).contain().size_full().rounded_md())
                .child(div().absolute().top_1().right_1().child(
                    button(format!("gif-fav-{}", fav.id), if favorited { "\u{2665}" } else { "\u{2661}" }).on_click(
                        move |_, _, cx| {
                            if let Some(store) = bridge::store(cx) {
                                store.toggle_gif_favorite(&fav);
                            }
                        },
                    ),
                ))
        };
        let mut sections: Vec<AnyElement> = Vec::new();
        let grid = |label_text: &str, gifs: &[Gif], cx: &mut Context<Self>| {
            v_flex()
                .gap_1()
                .child(label(label_text.to_owned()).text_style(TextStyle::Caption1).secondary())
                .child(div().flex().flex_row().flex_wrap().gap_1().children(gifs.iter().map(|g| cell(g, cx))))
                .into_any_element()
        };
        if show_favorites && !favorites.is_empty() {
            sections.push(grid("Favorites", &favorites, cx));
        }
        if self.gif.loading && self.gif.items.is_empty() {
            sections.push(label("Loading\u{2026}").secondary().into_any_element());
        } else if self.gif.items.is_empty() && !show_favorites {
            sections
                .push(label(format!("No GIFs for \u{201c}{}\u{201d}", self.gif.query)).secondary().into_any_element());
        } else if !self.gif.items.is_empty() {
            let items = self.gif.items.clone();
            sections.push(grid(if show_favorites { "Trending" } else { "Results" }, &items, cx));
        }
        popover("gif-picker")
            .placement(Placement::Above)
            .w(px(300.))
            .h(px(340.))
            .p_2()
            .on_dismiss(cx.listener(|this, _: &Dismissed, _, cx| {
                this.popover = Popover::None;
                cx.notify();
            }))
            .child(
                v_flex()
                    .size_full()
                    .gap_2()
                    .child(
                        text_field("gif-search")
                            .search()
                            .text(self.gif.query.clone())
                            .placeholder("Search KLIPY")
                            .on_change(
                                cx.listener(|this, e: &TextChanged, _, cx| this.set_gif_query(e.text.clone(), cx)),
                            )
                            .on_submit(cx.listener(|this, _: &TextSubmitted, _, cx| {
                                this.gif.epoch += 1;
                                this.search_gifs(cx);
                            })),
                    )
                    .child(
                        v_flex().id("gif-results").flex_1().min_h_0().overflow_y_scroll().gap_2().children(sections),
                    ),
            )
            .into_any_element()
    }

    fn effects_popover(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.effect;
        let mut col = v_flex().gap_0p5().child(
            button("effect-none", if current.is_none() { "\u{2713} No Effect" } else { "No Effect" }).on_click(
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.effect = None;
                    this.popover = Popover::None;
                    cx.notify();
                }),
            ),
        );
        for (id, title) in EFFECTS {
            let label_text = if current == Some(id) { format!("\u{2713} {title}") } else { title.to_owned() };
            col = col.child(button(format!("effect-{id}"), label_text).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.effect = Some(id);
                    this.popover = Popover::None;
                    cx.notify();
                },
            )));
        }
        popover("effects")
            .placement(Placement::Above)
            .p_2()
            .on_dismiss(cx.listener(|this, _: &Dismissed, _, cx| {
                this.popover = Popover::None;
                cx.notify();
            }))
            .child(col)
            .into_any_element()
    }

    fn choose_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = OpenDialog::file().multiple().title("Attach Files");
        let weak = cx.weak_entity();
        window.open_dialog(cx, dialog, {
            move |paths, cx| {
                if let Some(paths) = paths {
                    weak.update(cx, |this, cx| this.stage_files(paths, cx));
                }
            }
        });
    }

    /// What the "+" offers in the open conversation: GIFs, effects, send later.
    fn plus_offers(&self, cx: &App) -> (bool, bool, bool) {
        let Some(store) = bridge::store(cx) else { return (false, false, false) };
        let state = store.state();
        let sms = self.chat.as_deref().and_then(|c| state.chat(c)).is_some_and(|c| c.service != Service::IMessage);
        let editing = self.chat.as_ref().is_some_and(|c| state.editing.contains_key(c));
        (
            self.klipy.is_some(),
            state.capabilities.effects && !sms && !editing,
            state.capabilities.scheduled_messages && !self.draft.text.trim().is_empty(),
        )
    }

    /// Messages' "+" on macOS: a native menu over the button, one row per thing to add, each
    /// opening its panel beside the field as the drawn menu's rows do.
    fn plus_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (klipy, effects, can_schedule) = self.plus_offers(cx);
        let mut menu = ContextMenu::default()
            .above()
            .entry(MenuItem::action("Photos and Files\u{2026}", ChooseFiles).icon(Icon::Image));
        if klipy {
            menu = menu.entry(MenuItem::action("GIFs", ShowGifs).icon(Icon::Smile));
        }
        if can_schedule {
            menu = menu.entry(MenuItem::action("Send Later", ShowSendLater).icon(Icon::Clock));
        }
        if effects {
            let label = if self.effect.is_some() { "Message Effects \u{2022}" } else { "Message Effects" };
            menu = menu.entry(MenuItem::action(label, ShowEffects).icon(Icon::Star));
        }
        let menu =
            menu.separator().entry(MenuItem::action("Attach by Path\u{2026}", AttachByPath).icon(Icon::FileText));
        let window = window.id();
        // The semantic tree is out of reach while this handler holds the window: the next turn.
        nd::system::spawn(async {}, move |(), cx| {
            let Some(tree) = cx.semantic_window(window) else { return };
            let find = |id: &str| tree.root.iter().find(|n| n.test_id.as_deref() == Some(id)).map(|n| (n.id, n.bounds));
            // The "+" is drawn inside the composer's own view, so the menu hangs off the
            // composer at the button's offset in it.
            let (Some((node, Some(composer))), Some((_, Some(plus)))) = (find("composer"), find("composer.attach"))
            else {
                return;
            };
            // The rows dispatch from the focused node, so the field takes focus first.
            if let Some((field, _)) = find("composer.field") {
                cx.focus(window, field);
            }
            let (x, y) = (plus.origin.x - composer.origin.x, plus.origin.y - composer.origin.y);
            nd::components::services::show_context_menu(cx, window, node, x, y, menu);
        });
    }

    fn open_panel(&mut self, to: Popover, cx: &mut Context<Self>) {
        self.popover = to;
        if to == Popover::Gif && self.gif.items.is_empty() {
            self.search_gifs(cx);
        }
        cx.notify();
    }

    /// The "+" menu: files, GIFs, effects, send later and attach by path, each opening its own
    /// panel in the same place.
    fn apps_popover(&self, klipy: bool, effects: bool, can_schedule: bool, cx: &mut Context<Self>) -> AnyElement {
        let go = |to: Popover| {
            cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
                this.popover = to;
                if to == Popover::Gif && this.gif.items.is_empty() {
                    this.search_gifs(cx);
                }
                cx.notify();
            })
        };
        let item = |id: &'static str, glyph: Icon, title: &'static str| {
            button(id, title).icon(glyph).bezel(Bezel::Borderless).w_full()
        };
        popover("composer-apps-menu")
            .placement(Placement::Above)
            .w(px(220.))
            .p_1()
            .on_dismiss(cx.listener(|this, _: &Dismissed, _, cx| {
                if this.popover == Popover::Apps {
                    this.popover = Popover::None;
                    cx.notify();
                }
            }))
            .child(
                v_flex()
                    .gap_0p5()
                    .items_start()
                    .child(item("apps-files", Icon::Image, "Photos and Files\u{2026}").on_click(cx.listener(
                        |this, _: &ClickEvent, window, cx| {
                            this.popover = Popover::None;
                            this.choose_files(window, cx);
                            cx.notify();
                        },
                    )))
                    .when(klipy, |col| col.child(item("gif", Icon::Smile, "GIFs").on_click(go(Popover::Gif))))
                    .when(effects, |col| {
                        col.child(
                            item(
                                "effect",
                                Icon::Star,
                                if self.effect.is_some() { "Message Effect \u{2022}" } else { "Message Effect" },
                            )
                            .on_click(go(Popover::Effects)),
                        )
                    })
                    .when(can_schedule, |col| {
                        col.child(item("send-later", Icon::Clock, "Send Later").on_click(go(Popover::Schedule)))
                    })
                    .child(item("attach-path", Icon::FileText, "Attach by Path\u{2026}").on_click(go(Popover::Path))),
            )
            .into_any_element()
    }

    /// Stages the typed path (`~` is the home folder) when it names a file.
    fn submit_path(&mut self, cx: &mut Context<Self>) {
        let raw = self.path_text.trim().to_owned();
        if raw.is_empty() {
            return;
        }
        let path = match raw.strip_prefix('~') {
            Some(rest) => std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(rest.trim_start_matches('/')))
                .unwrap_or_else(|| PathBuf::from(&raw)),
            None => PathBuf::from(&raw),
        };
        if !path.is_file() {
            self.path_error = Some(format!("No file at {}", path.display()));
            cx.notify();
            return;
        }
        self.path_text = SharedString::default();
        self.path_error = None;
        self.popover = Popover::None;
        self.stage_files(vec![path], cx);
    }

    fn path_popover(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        popover("attach-path")
            .placement(Placement::Above)
            .w(px(360.))
            .p_3()
            .on_dismiss(cx.listener(|this, _: &Dismissed, _, cx| {
                this.popover = Popover::None;
                cx.notify();
            }))
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        text_field("attach-path-field")
                            .text(self.path_text.clone())
                            .placeholder("Path to a file, for example ~/Pictures/photo.jpg")
                            .label("Path")
                            .on_change(cx.listener(|this, e: &TextChanged, _, cx| {
                                this.path_text = e.text.clone();
                                this.path_error = None;
                                cx.notify();
                            }))
                            .on_submit(cx.listener(|this, _: &TextSubmitted, _, cx| this.submit_path(cx))),
                    )
                    .children(self.path_error.clone().map(|e| text(e).text_color(theme.text.danger))),
            )
            .into_any_element()
    }

    fn schedule_popover(&self, cx: &mut Context<Self>) -> AnyElement {
        let now = ui::now_ms();
        let theme = cx.theme().clone();
        let mut options: Vec<(&str, i64)> = vec![("Send in 1 Hour", now + 3_600_000)];
        if let Ok(at) = parse_schedule_time("20:00", now).or_else(|_| parse_schedule_time("tomorrow 20:00", now)) {
            options.push(("Send Tonight at 20:00", at));
        }
        if let Ok(at) = parse_schedule_time("tomorrow 09:00", now) {
            options.push(("Send Tomorrow at 09:00", at));
        }
        popover("schedule")
            .placement(Placement::Above)
            .w(px(320.))
            .p_3()
            .on_dismiss(cx.listener(|this, _: &Dismissed, _, cx| {
                this.popover = Popover::None;
                cx.notify();
            }))
            .child(
                v_flex()
                    .gap_2()
                    .children(options.into_iter().map(|(title, at)| {
                        button(format!("schedule-{at}"), title)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.schedule_at(at, cx)))
                    }))
                    .child(
                        text_field("schedule-at")
                            .text(self.schedule_text.clone())
                            .placeholder("HH:MM, tomorrow HH:MM, or YYYY-MM-DD HH:MM")
                            .label("Send at")
                            .on_change(cx.listener(|this, e: &TextChanged, _, cx| {
                                this.schedule_text = e.text.clone();
                                cx.notify();
                            }))
                            .on_submit(cx.listener(|this, _: &TextSubmitted, _, cx| this.submit_schedule(cx))),
                    )
                    .children(self.schedule_error.clone().map(|e| text(e).text_color(theme.text.danger))),
            )
            .into_any_element()
    }
}

/// The reply banner's line for a message.
fn replying_to(message: &Message) -> String {
    if message.from_me {
        "Replying to yourself".to_owned()
    } else {
        format!(
            "Replying to {}",
            message
                .sender
                .as_ref()
                .map(|h| messages_core::handle_name(h).to_owned())
                .unwrap_or_else(|| "message".into())
        )
    }
}

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync(cx);
        let Some(store) = bridge::store(cx) else { return div().into_any_element() };
        let Some(chat) = self.chat.clone() else { return div().into_any_element() };
        let (sms, capabilities, replying, editing, scheduled) = {
            let state = store.state();
            let messages = conversation_messages(&state, &chat);
            let find = |guid: Option<&String>| guid.and_then(|g| messages.iter().find(|m| &m.guid == g).cloned());
            (
                state.chat(&chat).is_some_and(|c| c.service != Service::IMessage),
                state.capabilities,
                find(state.replying_to.get(&chat)),
                find(state.editing.get(&chat)),
                state.scheduled.iter().filter(|s| s.chat_guid == chat).cloned().collect::<Vec<_>>(),
            )
        };
        let mode = (chat.clone(), replying.as_ref().map(|m| m.guid.clone()), editing.as_ref().map(|m| m.guid.clone()));
        let is_editing = editing.is_some();
        // Texts to an SMS or RCS conversation go out green, as their bubbles do.
        let sms_fill = sms.then(|| cx.theme().fill(nd::theme::FillRole::Success).0);
        // A new conversation, reply or edit puts the keyboard in the field, as in Messages. The
        // window applies the request once the field is mounted, at launch too.
        if self.focus_for.as_ref() != Some(&mode) {
            window.focus("composer.field");
            self.focus_for = Some(mode);
        }
        let theme = cx.theme().clone();
        let placeholder = if editing.is_some() {
            "Edit message"
        } else if sms {
            "Text Message"
        } else {
            "iMessage"
        };
        let klipy = self.klipy.is_some();
        let effects = capabilities.effects && !sms && editing.is_none();
        let can_schedule = capabilities.scheduled_messages && !self.draft.text.trim().is_empty();
        let now = ui::now_ms();

        let scheduled_rows = scheduled.into_iter().map(|item| {
            let id = item.id.clone();
            h_flex()
                .id(format!("scheduled-{}", item.id))
                .gap_2()
                .px_4()
                .py_1()
                .child(icon(Icon::Clock, 14.).text_color(theme.text.secondary))
                .child(
                    v_flex().flex_1().min_w_0().child(text(ui::clamp_preview(&item.text, 80))).child(
                        text(format!("Sends at {}", format_scheduled_for(item.send_at, now)))
                            .text_size(theme.type_scale.caption)
                            .text_color(theme.text.secondary),
                    ),
                )
                .child(button(format!("cancel-{}", item.id), "Cancel Send").on_click(move |_, _, cx| {
                    if let Some(store) = bridge::store(cx) {
                        let id = id.clone();
                        crate::rt::detach(async move { store.cancel_scheduled(&id).await });
                    }
                }))
        });

        let reply_banner = replying.map(|m| {
            let body = if m.text.is_empty() { "Attachment".to_owned() } else { m.text.clone() };
            self.banner("reply-banner", replying_to(&m), body, cx, |store, chat| store.set_replying_to(chat, None))
        });
        let edit_banner = editing.map(|m| {
            self.banner("edit-banner", "Editing".into(), m.text.clone(), cx, |store, chat| {
                store.set_editing(chat, None)
            })
        });

        // Messages' "+": everything that is not typing lives in one menu beside the field.
        let popover = match self.popover {
            Popover::None => None,
            Popover::Apps => Some(self.apps_popover(klipy, effects, can_schedule, cx)),
            Popover::Gif => Some(self.gif_popover(cx)),
            Popover::Effects => Some(self.effects_popover(cx)),
            Popover::Schedule => Some(self.schedule_popover(cx)),
            Popover::Path => Some(self.path_popover(cx)),
        };
        // The panels hang off a sliver beside the plus button: a popover needs an anchor with an
        // area to point at.
        let extras = div().id("composer-apps").w(px(1.)).h(px(COMPOSER_ROW)).children(popover);

        v_flex()
            .id("composer-area")
            .w_full()
            .flex_none()
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &EditLast, _, cx| {
                if !this.edit_last(cx) {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &PasteAttachments, _, cx| this.paste_attachments(cx)))
            .on_action(cx.listener(|this, _: &ChooseFiles, window, cx| this.choose_files(window, cx)))
            .on_action(cx.listener(|this, _: &ShowGifs, _, cx| this.open_panel(Popover::Gif, cx)))
            .on_action(cx.listener(|this, _: &ShowEffects, _, cx| this.open_panel(Popover::Effects, cx)))
            .on_action(cx.listener(|this, _: &ShowSendLater, _, cx| this.open_panel(Popover::Schedule, cx)))
            .on_action(cx.listener(|this, _: &AttachByPath, _, cx| this.open_panel(Popover::Path, cx)))
            .on_action(cx.listener(|this, _: &Dismiss, _, cx| {
                let (Some(store), Some(chat)) = (bridge::store(cx), this.chat.clone()) else {
                    cx.propagate();
                    return;
                };
                let (replying, editing) = {
                    let state = store.state();
                    (state.replying_to.contains_key(&chat), state.editing.contains_key(&chat))
                };
                if !replying && !editing {
                    cx.propagate();
                    return;
                }
                store.set_replying_to(&chat, None);
                store.set_editing(&chat, None);
            }))
            .children(scheduled_rows)
            .children(reply_banner)
            .children(edit_banner)
            .when(self.clipboard_notice, |col| {
                col.child(
                    text("Nothing to attach on the clipboard")
                        .text_size(theme.type_scale.caption)
                        .text_color(theme.text.tertiary)
                        .px_4(),
                )
            })
            .child(
                // Messages' bar, measured: the plus button 11 pt from the pane's edge and the
                // window's bottom (the menu's 1 pt anchor sits before it).
                h_flex().pl(px(10.)).pr(px(11.)).py(px(11.)).items_end().child(extras).child(
                    composer("composer", &self.draft)
                        .placeholder(placeholder)
                        .when(is_editing, |composer| composer.send_icon(Icon::CircleCheck, "Save Edit"))
                        .when_some(sms_fill, |composer, fill| composer.send_fill(fill))
                        .flex_1()
                        .on_event(cx.listener(|this, e: &ComposerEvent, window, cx| this.event(e, window, cx))),
                ),
            )
            .into_any_element()
    }
}
