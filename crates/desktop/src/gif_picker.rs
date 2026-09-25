//! Port of `apps/desktop/src/ui/gif-picker.tsx`: the Klipy GIF picker
//! anchored off the composer's GIF button. Owned by D3; not mounted directly
//! by app.rs (`composer::Composer` opens it).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::config::Config;
use messages_core::gifs::{Gif, KlipyClient, download_gif, download_gif_preview, favorite_gifs};

use crate::bridge::{Bridge, StoreHandle, Topic};
use crate::icons::{Icon, IconName};
use crate::theme::{Palette, Theme, radius, spacing, type_scale};

const PANEL_WIDTH: Pixels = px(300.);
const PANEL_HEIGHT: Pixels = px(340.);
const CELL_HEIGHT: Pixels = px(84.);
const DEBOUNCE_MS: u64 = 300;

/// Runs `future` on the store's tokio runtime and delivers the result back to
/// the caller's own `cx.spawn`. `None` when the store is not connected yet.
fn spawn_on_store<T: Send + 'static>(cx: &App, future: impl std::future::Future<Output = T> + Send + 'static) -> Option<tokio::sync::oneshot::Receiver<T>> {
    let store = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone())?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    store.spawn(async move {
        let _ = tx.send(future.await);
    });
    Some(rx)
}

pub struct GifPicker {
    chat_guid: String,
    anchor: Point<Pixels>,
    on_close: std::rc::Rc<dyn Fn(&mut Window, &mut App)>,
    focus_handle: FocusHandle,
    query_input: Entity<InputState>,
    committed: String,
    debounce_generation: u64,
    klipy: Option<Arc<KlipyClient>>,
    items: Vec<Gif>,
    loading: bool,
    previews: HashMap<String, String>,
    favorite_previews: HashMap<String, String>,
}

impl GifPicker {
    pub fn new(chat_guid: String, anchor: Point<Pixels>, on_close: impl Fn(&mut Window, &mut App) + 'static, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade().into();
        Bridge::watch(cx, Topic::GifFavorites, weak);
        let focus_handle = cx.focus_handle();
        let query_input = crate::primitives::new_input_state(window, cx, "Search KLIPY", false);
        window.focus(&query_input.focus_handle(cx), cx);
        cx.subscribe(&query_input, |this: &mut Self, _state, event: &InputEvent, cx| match event {
            InputEvent::Change => this.on_query_changed(cx),
            InputEvent::PressEnter { .. } => this.commit_now(cx),
            _ => {}
        })
        .detach();
        let mut this = Self {
            chat_guid,
            anchor,
            on_close: std::rc::Rc::new(on_close),
            focus_handle,
            query_input,
            committed: String::new(),
            debounce_generation: 0,
            klipy: None,
            items: Vec::new(),
            loading: true,
            previews: HashMap::new(),
            favorite_previews: HashMap::new(),
        };
        this.load_client(cx);
        this
    }

    fn load_client(&mut self, cx: &mut Context<Self>) {
        let Some(rx) = spawn_on_store(cx, messages_core::config::load_config()) else { return };
        cx.spawn(async move |this, cx| {
            if let Ok(config) = rx.await {
                let _ = this.update(cx, |this, cx| this.apply_config(config, cx));
            }
        })
        .detach();
    }

    fn apply_config(&mut self, config: Config, cx: &mut Context<Self>) {
        if let Some(klipy) = config.klipy {
            self.klipy = Some(Arc::new(KlipyClient::new(klipy.api_key, reqwest::Client::new())));
            self.run_search(cx);
            self.load_favorite_previews(cx);
        } else {
            self.loading = false;
            cx.notify();
        }
    }

    fn on_query_changed(&mut self, cx: &mut Context<Self>) {
        self.debounce_generation += 1;
        let generation = self.debounce_generation;
        let query = self.query_input.read(cx).value().to_string();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(DEBOUNCE_MS)).await;
            let _ = this.update(cx, |this, cx| {
                if this.debounce_generation == generation {
                    this.committed = query;
                    this.run_search(cx);
                }
            });
        })
        .detach();
    }

    fn commit_now(&mut self, cx: &mut Context<Self>) {
        self.debounce_generation += 1;
        self.committed = self.query_input.read(cx).value().to_string();
        self.run_search(cx);
    }

    fn run_search(&mut self, cx: &mut Context<Self>) {
        let Some(klipy) = self.klipy.clone() else { return };
        self.loading = true;
        self.previews.clear();
        cx.notify();
        let term = self.committed.trim().to_owned();
        let Some(rx) = spawn_on_store(cx, async move { if term.is_empty() { klipy.trending(1).await } else { klipy.search(&term, 1).await } }) else { return };
        cx.spawn(async move |this, cx| {
            if let Ok(result) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.items = result.map(|page| page.items).unwrap_or_default();
                    this.loading = false;
                    cx.notify();
                    this.load_previews(cx);
                });
            }
        })
        .detach();
    }

    fn load_previews(&mut self, cx: &mut Context<Self>) {
        for item in self.items.clone() {
            let Some(rx) = spawn_on_store(cx, async move {
                let dir = messages_core::config::attachments_dir();
                download_gif_preview(&reqwest::Client::new(), &item, &dir).await.ok().map(|path| (item.id, path))
            }) else {
                continue;
            };
            cx.spawn(async move |this, cx| {
                if let Ok(Some((id, path))) = rx.await {
                    let _ = this.update(cx, |this, cx| {
                        this.previews.insert(id, path.to_string_lossy().into_owned());
                        cx.notify();
                    });
                }
            })
            .detach();
        }
    }

    fn load_favorite_previews(&mut self, cx: &mut Context<Self>) {
        let Some(store) = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone()) else { return };
        for item in favorite_gifs(&store.state().gif_favorites) {
            let Some(rx) = spawn_on_store(cx, async move {
                let dir = messages_core::config::attachments_dir();
                download_gif_preview(&reqwest::Client::new(), &item, &dir).await.ok().map(|path| (item.id, path))
            }) else {
                continue;
            };
            cx.spawn(async move |this, cx| {
                if let Ok(Some((id, path))) = rx.await {
                    let _ = this.update(cx, |this, cx| {
                        this.favorite_previews.insert(id, path.to_string_lossy().into_owned());
                        cx.notify();
                    });
                }
            })
            .detach();
        }
    }

    /// Downloads the full GIF and sends it as an attachment, then closes the picker.
    fn pick(&mut self, item: Gif, window: &mut Window, cx: &mut Context<Self>) {
        (self.on_close.clone())(window, cx);
        let chat_guid = self.chat_guid.clone();
        let Some(rx) = spawn_on_store(cx, async move {
            let dir = messages_core::config::attachments_dir();
            download_gif(&reqwest::Client::new(), &item, &dir).await
        }) else {
            return;
        };
        cx.spawn(async move |_this, cx| {
            if let Ok(Ok(path)) = rx.await {
                if let Some(store) = cx.update(|cx| cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone())) {
                    store.send_attachment(&chat_guid, &path);
                }
            }
        })
        .detach();
    }

    fn toggle_favorite(&self, item: &Gif, cx: &mut Context<Self>) {
        let Some(store) = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone()) else { return };
        store.toggle_gif_favorite(item);
    }
}

fn gif_cell(palette: &Palette, item: &Gif, preview: Option<String>, favorited: bool, cx: &Context<GifPicker>) -> impl IntoElement {
    let for_pick = item.clone();
    let for_fav = item.clone();
    div()
        .id(ElementId::Name(format!("gif-{}", item.id).into()))
        .relative()
        .w(px(84.))
        .h(CELL_HEIGHT)
        .rounded(radius::CONTROL)
        .overflow_hidden()
        .bg(palette.raised)
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .on_click(cx.listener(move |this, _, window, cx| this.pick(for_pick.clone(), window, cx)))
        .when_some(preview, |el, preview| el.child(img(preview).w_full().h_full().object_fit(ObjectFit::Contain)))
        .child(
            div()
                .id(ElementId::Name(format!("gif-favorite-{}", item.id).into()))
                .absolute()
                .top(spacing::X1)
                .right(spacing::X1)
                .w(px(20.))
                .h(px(20.))
                .rounded(radius::PILL)
                .bg(palette.overlay)
                .opacity(0.85)
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(move |this, _, _window, cx| this.toggle_favorite(&for_fav, cx)))
                .child(Icon::new(IconName::Heart).size(px(11.)).color(if favorited { palette.danger } else { palette.on_accent_soft })),
        )
}

impl Render for GifPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        let Some(store) = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone()) else {
            return div().into_any_element();
        };
        let favorites = favorite_gifs(&store.state().gif_favorites);
        let favorite_ids: HashSet<String> = favorites.iter().map(|item| item.id.clone()).collect();
        let show_favorites = self.committed.trim().is_empty();
        let on_close_outside = self.on_close.clone();
        let on_close_key = self.on_close.clone();


        anchored().position(self.anchor).anchor(Anchor::BottomLeft).snap_to_window_with_margin(spacing::X2).child(deferred(
            div()
                .id("gif-picker")
                .track_focus(&self.focus_handle)
                .flex()
                .flex_col()
                .w(PANEL_WIDTH)
                .h(PANEL_HEIGHT)
                .gap(spacing::X2)
                .p(spacing::X2)
                .rounded(radius::MENU)
                .bg(palette.overlay)
                .border_1()
                .border_color(palette.overlay_border)
                .on_mouse_down_out(move |_, window, cx| on_close_outside(window, cx))
                .on_key_down(move |event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        on_close_key(window, cx);
                    }
                })
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(spacing::X1)
                        .flex_shrink_0()
                        .h(px(28.))
                        .px(spacing::X2)
                        .rounded(radius::CONTROL)
                        .bg(palette.canvas)
                        .border_1()
                        .border_color(palette.separator)
                        .child(Icon::new(IconName::Search).size(px(13.)).color(palette.tertiary))
                        .child(div().flex_grow(1.).min_w(px(0.)).child(crate::primitives::text_field(&self.query_input))),
                )
                .child(
                    div()
                        .id("gif-grid")
                        .flex_grow(1.)
                        .min_h(px(0.))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap(spacing::X3)
                        .when(show_favorites && !favorites.is_empty(), |el| {
                            el.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(spacing::X1)
                                    .flex_shrink_0()
                                    .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).child("Favorites"))
                                    .child(div().flex().flex_row().flex_wrap().gap(spacing::X1).children(favorites.iter().map(|item| {
                                        let preview = self.favorite_previews.get(&item.id).cloned();
                                        gif_cell(&palette, item, preview, true, cx)
                                    }))),
                            )
                        })
                        .child(if self.loading {
                            div()
                                .flex_grow(1.)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).child("Loading…"))
                                .into_any_element()
                        } else if self.items.is_empty() {
                            div()
                                .flex_grow(1.)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .text_size(type_scale::CAPTION.font_size)
                                        .line_height(type_scale::CAPTION.line_height)
                                        .text_color(palette.secondary)
                                        .child(format!("No GIFs for \"{}\"", self.committed)),
                                )
                                .into_any_element()
                        } else {
                            div()
                                .flex()
                                .flex_col()
                                .gap(spacing::X1)
                                .flex_shrink_0()
                                .when(show_favorites, |el| {
                                    el.child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).child("Trending"))
                                })
                                .child(div().flex().flex_row().flex_wrap().gap(spacing::X1).children(self.items.iter().map(|item| {
                                    let preview = self.previews.get(&item.id).cloned();
                                    let favorited = favorite_ids.contains(&item.id);
                                    gif_cell(&palette, item, preview, favorited, cx)
                                })))
                                .into_any_element()
                        }),
                ),
        )).into_any_element()
    }
}
