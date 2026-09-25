//! Port of `apps/desktop/src/ui/location.tsx`: the Find My tile card shown
//! under a participant in the details panel. Owned by D3; not mounted
//! directly by app.rs (it sits inside `details::InfoPanel`).

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::findmy::{FriendLocation, MapTile};

use crate::bridge::StoreHandle;
use crate::theme::{Theme, radius, spacing, type_scale};

const TILE_SIZE: f32 = 256.;
const TILE_ZOOM: u8 = 15;
const PIN_SIZE: f32 = 12.;

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

/// Millis since the epoch. Core keeps `now_ms` crate-private (`store.rs`), so
/// the UI computes its own for the "relative time" labels it renders.
fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn open_in_maps(location: &FriendLocation) {
    messages_core::open::open_external(&format!(
        "https://www.openstreetmap.org/?mlat={}&mlon={}#map=16/{}/{}",
        location.latitude, location.longitude, location.latitude, location.longitude
    ));
}

pub struct LocationCard {
    handle_address: String,
    location: FriendLocation,
    tile: Option<MapTile>,
}

impl LocationCard {
    pub fn new(handle_address: String, location: FriendLocation, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self { handle_address, location, tile: None };
        this.fetch_tile(cx);
        this
    }

    /// Refetches the tile only when the coordinates actually moved, so a
    /// details-panel re-render for an unrelated reason does not restart it.
    pub fn set_location(&mut self, location: FriendLocation, cx: &mut Context<Self>) {
        let moved = location.latitude != self.location.latitude || location.longitude != self.location.longitude;
        self.location = location;
        if moved {
            self.tile = None;
            self.fetch_tile(cx);
        }
        cx.notify();
    }

    fn fetch_tile(&mut self, cx: &mut Context<Self>) {
        let lat = self.location.latitude;
        let lon = self.location.longitude;
        let address = self.handle_address.clone();
        let Some(rx) = spawn_on_store(cx, async move {
            let http = reqwest::Client::new();
            messages_core::findmy::tile_for(&http, lat, lon, TILE_ZOOM, &messages_core::config::cache_dir()).await
        }) else {
            return;
        };
        cx.spawn(async move |this, cx| match rx.await {
            Ok(Ok(tile)) => {
                let _ = this.update(cx, |this, cx| {
                    this.tile = Some(tile);
                    cx.notify();
                });
            }
            Ok(Err(error)) => {
                eprintln!("findmy: tile fetch failed for {address}: {error}");
            }
            Err(_) => {}
        })
        .detach();
    }
}

impl Render for LocationCard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        let location = self.location.clone();
        let coordinate_label = location.label.clone().unwrap_or_else(|| format!("{:.4}, {:.4}", location.latitude, location.longitude));
        let now = now_ms();

        div()
            .id(ElementId::Name(format!("location-{}", self.handle_address).into()))
            .flex()
            .flex_col()
            .gap(spacing::X2)
            .px(spacing::X2)
            .pt(spacing::X1)
            .pb(spacing::X3)
            .flex_shrink_0()
            .child(
                div().relative().w(px(TILE_SIZE)).h(px(TILE_SIZE)).rounded(radius::BUBBLE).overflow_hidden().flex_shrink_0().bg(palette.raised).when_some(self.tile.as_ref(), |el, tile| {
                    el.child(img(crate::attachments::image_source(&tile.path)).w(px(TILE_SIZE)).h(px(TILE_SIZE)).object_fit(ObjectFit::Cover)).child(
                        div()
                            .absolute()
                            .left(px(tile.px as f32 - PIN_SIZE / 2.))
                            .top(px(tile.py as f32 - PIN_SIZE / 2.))
                            .w(px(PIN_SIZE))
                            .h(px(PIN_SIZE))
                            .rounded(px(PIN_SIZE / 2.))
                            .bg(palette.accent)
                            .border_2()
                            .border_color(white()),
                    )
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.text).child(coordinate_label))
                    .child(
                        div()
                            .text_size(type_scale::MICRO.font_size)
                            .line_height(type_scale::MICRO.line_height)
                            .text_color(palette.secondary)
                            .child(messages_core::format::relative_time(location.timestamp, now)),
                    ),
            )
            .child(
                div()
                    .id("open-in-maps")
                    .child(
                        div()
                            .text_size(type_scale::CAPTION.font_size)
                            .line_height(type_scale::CAPTION.line_height)
                            .text_color(palette.accent)
                            .border_b_1()
                            .border_color(palette.accent)
                            .child("Open in maps"),
                    )
                    .on_click(move |_, _window, _cx| open_in_maps(&location)),
            )
    }
}
