//! The "Connect to your Mac" screen, shown when there is no store yet, or
//! overlaid when settings are open.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use messages_core::ServerInfo;

use crate::icons::{Icon, IconName};
use crate::primitives::{Button, ButtonKind, IconButton, divider};
use crate::theme::{Theme, radius, spacing, type_scale};

pub struct ConnectScreen {
    url: Entity<InputState>,
    password: Entity<InputState>,
    error: Option<SharedString>,
    connecting: bool,
    server: Option<ServerInfo>,
    had_url: bool,
    on_connect: std::rc::Rc<dyn Fn(String, String, &mut Window, &mut App)>,
    on_demo: std::rc::Rc<dyn Fn(&mut Window, &mut App)>,
    on_close: Option<std::rc::Rc<dyn Fn(&mut Window, &mut App)>>,
    _subscriptions: [Subscription; 2],
}

impl ConnectScreen {
    pub fn new(
        initial_url: String,
        initial_password: String,
        on_connect: impl Fn(String, String, &mut Window, &mut App) + 'static,
        on_demo: impl Fn(&mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let had_url = !initial_url.is_empty();
        let url = cx.new(|cx| InputState::new(window, cx).placeholder("http://192.168.1.20:1234"));
        let password = cx.new(|cx| InputState::new(window, cx).placeholder("Password").masked(true));
        url.update(cx, |state, cx| state.set_value(initial_url, window, cx));
        password.update(cx, |state, cx| state.set_value(initial_password, window, cx));
        window.focus(&if had_url { password.focus_handle(cx) } else { url.focus_handle(cx) }, cx);

        // gpui-component's `Input` emits `InputEvent::PressEnter` rather than
        // a DOM-style submit event; `subscribe_in` is the variant of
        // `subscribe` that also hands back the `Window` the event needs to
        // reach `submit`.
        let on_press_enter = |this: &mut Self, _: &Entity<InputState>, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.submit(window, cx);
            }
        };
        let subscriptions = [cx.subscribe_in(&url, window, on_press_enter), cx.subscribe_in(&password, window, on_press_enter)];

        Self {
            url,
            password,
            error: None,
            connecting: false,
            server: None,
            had_url,
            on_connect: std::rc::Rc::new(on_connect),
            on_demo: std::rc::Rc::new(on_demo),
            on_close: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn error(&mut self, error: Option<SharedString>) -> &mut Self {
        self.error = error;
        self
    }

    pub fn connecting(&mut self, connecting: bool) -> &mut Self {
        self.connecting = connecting;
        self
    }

    pub fn server(&mut self, server: Option<ServerInfo>) -> &mut Self {
        self.server = server;
        self
    }

    pub fn on_close(&mut self, handler: Option<impl Fn(&mut Window, &mut App) + 'static>) -> &mut Self {
        self.on_close = handler.map(|f| std::rc::Rc::new(f) as std::rc::Rc<dyn Fn(&mut Window, &mut App)>);
        self
    }

    /// Checks the URL against `/^https?:\/\/\S+$/`.
    fn valid(&self, cx: &App) -> bool {
        let url = self.url.read(cx).value();
        let url = url.trim();
        let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"));
        rest.is_some_and(|rest| !rest.is_empty() && !rest.chars().any(char::is_whitespace)) && !self.password.read(cx).value().is_empty()
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.valid(cx) || self.connecting {
            return;
        }
        let url = self.url.read(cx).value().trim().trim_end_matches('/').to_owned();
        let password = self.password.read(cx).value().to_string();
        (self.on_connect.clone())(url, password, window, cx);
    }
}

impl Render for ConnectScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        let valid = self.valid(cx);
        let private_api_on = self.server.as_ref().is_some_and(|server| server.private_api && server.helper_connected);

        div()
            .id("connect")
            .flex_grow(1.)
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .p(spacing::X6)
            .bg(palette.canvas)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::X4)
                    .w(px(420.))
                    .max_w_full()
                    .p(spacing::X6)
                    .rounded(radius::CARD)
                    .bg(palette.sidebar)
                    .border_1()
                    .border_color(palette.sidebar_border)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_start()
                            .gap(spacing::X2)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(spacing::X1)
                                    .flex_grow(1.)
                                    .min_w(px(0.))
                                    .child(div().text_size(type_scale::LARGE.font_size).line_height(type_scale::LARGE.line_height).font_weight(FontWeight::BOLD).text_color(palette.text).child("Connect to your Mac"))
                                    .child(
                                        div()
                                            .text_size(type_scale::BODY.font_size)
                                            .line_height(type_scale::BODY.line_height)
                                            .text_color(palette.secondary)
                                            .child("Messages talks to the BlueBubbles server running on your Mac. Copy the address and password from its settings."),
                                    ),
                            )
                            .when_some(self.on_close.clone(), |el, on_close| {
                                el.child(IconButton::new("close-settings", IconName::Close, "Close settings (Esc)").on_click(move |_, window, cx| on_close(window, cx)))
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(spacing::X3)
                            .child(field(cx, "Server address", Input::new(&self.url).bordered(true)))
                            .child(field(cx, "Server password", Input::new(&self.password).bordered(true).mask_toggle())),
                    )
                    .when_some(self.error.clone(), |el, error| {
                        el.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_start()
                                .gap(spacing::X2)
                                .p(spacing::X3)
                                .rounded(radius::CONTROL)
                                .bg(palette.danger_soft)
                                .child(Icon::new(IconName::Alert).size(px(14.)).color(palette.danger))
                                .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.text).flex_grow(1.).min_w(px(0.)).child(error)),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(spacing::X2)
                            .child(Button::new("connect-button", if self.connecting { "Connecting…" } else { "Connect" }).kind(ButtonKind::Primary).disabled(!valid || self.connecting).on_click({
                                cx.listener(|this, _, window, cx| this.submit(window, cx))
                            }))
                            .when(!self.had_url, |el| {
                                let on_demo = self.on_demo.clone();
                                el.child(Button::new("demo-button", "Use demo data").on_click(move |_, window, cx| on_demo(window, cx)))
                            }),
                    )
                    .child(divider(palette.separator))
                    .when_some(self.server.clone(), |el, server| {
                        el.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(spacing::X2)
                                .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).child("Connected server"))
                                .child(info_line("BlueBubbles", server.version.clone(), None, palette))
                                .child(info_line("macOS", server.macos_version.clone().unwrap_or_else(|| "unknown".into()), None, palette))
                                .child(info_line("iCloud", server.icloud_account.clone().unwrap_or_else(|| "not detected".into()), None, palette))
                                .child(info_line(
                                    "Private API",
                                    SharedString::from(if private_api_on { "On" } else if server.private_api { "On, helper not connected" } else { "Off" }),
                                    Some(private_api_on),
                                    palette,
                                )),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_start()
                            .gap(spacing::X2)
                            .child(Icon::new(if private_api_on { IconName::Unlock } else { IconName::Lock }).size(px(14.)).color(palette.tertiary))
                            .child(
                                div()
                                    .text_size(type_scale::CAPTION.font_size)
                                    .line_height(type_scale::CAPTION.line_height)
                                    .text_color(palette.tertiary)
                                    .flex_grow(1.)
                                    .min_w(px(0.))
                                    .child("Tapbacks, typing indicators, read receipts, replies, edits and effects need the Private API. Turn it on in BlueBubbles on a Mac with SIP disabled. Everything else works without it."),
                            ),
                    ),
            )
    }
}

fn field(cx: &App, label: &'static str, input: impl IntoElement) -> impl IntoElement {
    let palette = Theme::get(cx);
    div()
        .flex()
        .flex_col()
        .gap(spacing::X1)
        .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).child(label))
        .child(input)
}

fn info_line(label: &'static str, value: impl Into<SharedString>, ok: Option<bool>, palette: crate::theme::Palette) -> impl IntoElement {
    let value = value.into();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(spacing::X2)
        .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.secondary).w(px(88.)).flex_shrink_0().child(label))
        .when_some(ok, |el, ok| el.child(div().w(px(8.)).h(px(8.)).rounded(px(4.)).flex_shrink_0().bg(if ok { palette.online } else { palette.warning })))
        .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.text).flex_grow(1.).min_w(px(0.)).child(value))
}
