//! The Find My map shown under a participant in the details panel: an Apple
//! Maps snapshot the Mac agent renders with MapKit, centred on the person, with
//! their avatar over the centre. Mounted by `details::InfoPanel`.

use std::path::PathBuf;

use gpui_kit::component::box_shadow;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::Handle;
use messages_core::findmy::{FriendLocation, MapSnapshotRequest, maps_url};

use crate::bridge::StoreHandle;
use crate::primitives::avatar;
use crate::theme::{INFO_WIDTH, Theme, radius, spacing, type_scale};

const MAP_HEIGHT: f32 = 150.;
const AVATAR: f32 = 36.;
/// Metres across the map's width: a few streets either side.
const SPAN: u32 = 1500;

/// The panel is a fixed width; the map fills it inside both column insets.
fn map_width() -> f32 {
    f32::from(INFO_WIDTH) - 4. * f32::from(spacing::X2)
}

/// Millis since the epoch. Core keeps `now_ms` crate-private (`store.rs`), so
/// the UI computes its own for the "relative time" labels it renders.
fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn maps_app() -> &'static str {
    if cfg!(target_os = "macos") { "Apple Maps" } else { "Google Maps" }
}

enum Snapshot {
    Loading,
    Ready(PathBuf),
    /// No agent, an agent too old to render maps, or a render that failed.
    Unavailable,
}

pub struct LocationCard {
    handle: Handle,
    location: FriendLocation,
    request: Option<MapSnapshotRequest>,
    snapshot: Snapshot,
}

impl LocationCard {
    pub fn new(handle: Handle, location: FriendLocation, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self { handle, location, request: None, snapshot: Snapshot::Loading }
    }

    pub fn set_location(&mut self, location: FriendLocation, cx: &mut Context<Self>) {
        if location != self.location {
            self.location = location;
            cx.notify();
        }
    }

    fn name(&self) -> String {
        self.location.name.clone().or_else(|| self.handle.name.clone()).unwrap_or_else(|| self.handle.address.clone())
    }

    fn open_in_maps(&self) {
        messages_core::open::open_external(&maps_url(self.location.latitude, self.location.longitude, Some(&self.name())));
    }

    /// Asks again only when the rounded request changes: a move, the theme flipping, or a new display scale.
    fn want_snapshot(&mut self, request: MapSnapshotRequest, cx: &mut Context<Self>) {
        if self.request.as_ref().is_some_and(|current| current.file_name() == request.file_name()) {
            return;
        }
        self.request = Some(request.clone());
        let store = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone());
        let Some((store, agent)) = store.and_then(|store| store.agent().cloned().map(|agent| (store, agent))) else {
            self.snapshot = Snapshot::Unavailable;
            return;
        };
        if !matches!(self.snapshot, Snapshot::Ready(_)) {
            self.snapshot = Snapshot::Loading;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        let asked = request.clone();
        store.spawn(async move {
            let _ = tx.send(agent.find_my_snapshot(&asked, &messages_core::config::cache_dir().join("maps")).await);
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                if this.request.as_ref() != Some(&request) {
                    return;
                }
                this.snapshot = match result {
                    Ok(path) => Snapshot::Ready(path),
                    Err(error) => {
                        eprintln!("findmy: map snapshot: {error}");
                        Snapshot::Unavailable
                    }
                };
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for LocationCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        let width = map_width();
        let scale = window.scale_factor().ceil().clamp(1., 3.) as u32;
        self.want_snapshot(
            MapSnapshotRequest {
                latitude: self.location.latitude,
                longitude: self.location.longitude,
                width: width.round() as u32,
                height: MAP_HEIGHT as u32,
                scale,
                dark: palette.is_dark(),
                span: SPAN,
            },
            cx,
        );

        let location = &self.location;
        let coordinates = format!("{:.5}, {:.5}", location.latitude, location.longitude);
        let place = location.label.clone().unwrap_or_else(|| coordinates.clone());
        let mut detail = messages_core::format::relative_time(location.timestamp, now_ms());
        if let Some(accuracy) = location.accuracy.filter(|accuracy| *accuracy > 0.) {
            detail.push_str(&format!(" · within {} m", accuracy.round() as i64));
        }
        // Metres per point along the width, which is how the agent frames the span.
        let accuracy_radius = location.accuracy.map(|metres| (metres * f64::from(width) / f64::from(SPAN)) as f32).filter(|radius| *radius > AVATAR / 2. + 4. && *radius < width / 2.);
        let open_label = format!("Open in {}", maps_app());

        let map = div()
            .id("map")
            .relative()
            .w(px(width))
            .h(px(MAP_HEIGHT))
            .flex_shrink_0()
            .rounded(radius::CARD)
            .overflow_hidden()
            .bg(palette.raised)
            .border_1()
            .border_color(crate::primitives::image_outline(&palette))
            .cursor_pointer()
            .tab_index(0)
            .hover(|style| style.opacity(0.92))
            .focus_visible(move |style| style.border_color(palette.focus_ring))
            .map(|el| match &self.snapshot {
                Snapshot::Ready(path) => el.child(img(crate::attachments::image_source(path)).absolute().top_0().left_0().w(px(width)).h(px(MAP_HEIGHT)).object_fit(ObjectFit::Cover)),
                Snapshot::Loading => el,
                Snapshot::Unavailable => el.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(spacing::X2)
                        .flex()
                        .justify_center()
                        .text_size(type_scale::MICRO.font_size)
                        .line_height(type_scale::MICRO.line_height)
                        .text_color(palette.tertiary)
                        .child(coordinates.clone()),
                ),
            })
            .when_some(accuracy_radius, |el, radius| {
                el.child(
                    div()
                        .absolute()
                        .left(px(width / 2. - radius))
                        .top(px(MAP_HEIGHT / 2. - radius))
                        .size(px(radius * 2.))
                        .rounded(px(radius))
                        .bg(palette.accent.opacity(0.18))
                        .border_1()
                        .border_color(palette.accent.opacity(0.45)),
                )
            })
            .child(
                div()
                    .absolute()
                    .left(px((width - AVATAR) / 2.))
                    .top(px((MAP_HEIGHT - AVATAR) / 2.))
                    .size(px(AVATAR))
                    .rounded(px(AVATAR / 2.))
                    .border_2()
                    .border_color(white())
                    .shadow(vec![box_shadow(px(0.), px(2.), px(6.), px(0.), hsla(0., 0., 0., 0.35))])
                    .overflow_hidden()
                    .child(avatar(Some(&self.handle), None, px(AVATAR - 4.), cx)),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, _cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.open_in_maps();
                }
            }))
            .on_click(cx.listener(|this, _, _window, _cx| this.open_in_maps()));

        div()
            .id(ElementId::Name(format!("location-{}", self.handle.address).into()))
            .flex()
            .flex_col()
            .gap(spacing::X2)
            .px(spacing::X2)
            .pt(spacing::X1)
            .pb(spacing::X3)
            .flex_shrink_0()
            .child(map)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).font_weight(FontWeight::SEMIBOLD).text_color(palette.text).text_ellipsis().child(place))
                    .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).child(detail)),
            )
            .child(
                div()
                    .id("open-in-maps")
                    .self_start()
                    .cursor_pointer()
                    .tab_index(0)
                    .rounded(radius::CONTROL)
                    .hover(|style| style.opacity(0.8))
                    .active(|style| style.opacity(0.6))
                    .focus_visible(move |style| style.bg(palette.selected_soft))
                    .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).text_color(palette.accent).child(open_label))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, _cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.open_in_maps();
                        }
                    }))
                    .on_click(cx.listener(|this, _, _window, _cx| this.open_in_maps())),
            )
    }
}
