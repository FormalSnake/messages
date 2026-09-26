//! Cmd/Ctrl+K's "jump to a conversation" panel. `app.rs` mounts `Switcher`
//! when the action fires and drops it when `on_close` runs (Escape, a click
//! outside, or opening a chat).

use gpui_kit::component::box_shadow;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::*;
use messages_core::conversations::conversation_chats;
use messages_core::{Chat, chat_title, fuzzy::search_chats};

use crate::bridge::{Bridge, StoreHandle, Topic};
use crate::primitives::avatar;
use crate::sidebar_row::preview_text;
use crate::theme::{TITLEBAR_HEIGHT, Theme, radius, spacing, type_scale};

const PANEL_WIDTH: Pixels = px(480.);
const RESULT_LIMIT: usize = 8;
const ROW_HEIGHT: Pixels = px(52.);

pub struct Switcher {
    query: Entity<InputState>,
    highlighted: usize,
    on_close: std::rc::Rc<dyn Fn(&mut Window, &mut App)>,
    _subscriptions: Vec<Subscription>,
}

impl Switcher {
    pub fn new(window: &mut Window, on_close: impl Fn(&mut Window, &mut App) + 'static, cx: &mut Context<Self>) -> Self {
        let on_close = std::rc::Rc::new(on_close);
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Jump to a conversation"));
        window.focus(&query.focus_handle(cx), cx);
        let weak = cx.entity().downgrade();
        Bridge::watch(cx, Topic::ChatList, weak.into());

        let sub = cx.subscribe_in(&query, window, |this: &mut Self, _state, event: &InputEvent, window, cx| match event {
            InputEvent::Change => {
                this.highlighted = 0;
                cx.notify();
            }
            InputEvent::PressEnter { .. } => this.open_highlighted(window, cx),
            _ => {}
        });

        Self { query, highlighted: 0, on_close, _subscriptions: vec![sub] }
    }

    fn results(&self, cx: &App) -> Vec<std::sync::Arc<Chat>> {
        let Some(store) = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone()) else { return Vec::new() };
        let state = store.state();
        let chats = conversation_chats(&state);
        let query = self.query.read(cx).value();
        search_chats(&chats, &query, RESULT_LIMIT)
    }

    fn open(&self, chat: &Chat, window: &mut Window, cx: &mut App) {
        (self.on_close)(window, cx);
        let Some(store) = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone()) else { return };
        let guid = chat.guid.clone();
        let for_task = store.clone();
        store.spawn(async move { for_task.select_chat(Some(&guid)).await });
    }

    fn open_highlighted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let results = self.results(cx);
        if let Some(chat) = results.get(self.highlighted).cloned() {
            self.open(&chat, window, cx);
        }
    }
}

impl Render for Switcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        let width = window.viewport_size().width;
        let results = self.results(cx);
        let highlighted = self.highlighted.min(results.len().saturating_sub(1));
        let close_for_escape = self.on_close.clone();
        let close_for_outside = self.on_close.clone();

        anchored().position(point(width / 2., TITLEBAR_HEIGHT + spacing::X4)).anchor(Anchor::TopCenter).snap_to_window_with_margin(spacing::X2).child(deferred(
            div()
                .id("switcher")
                .flex()
                .flex_col()
                .w(PANEL_WIDTH)
                .rounded(radius::MENU)
                .bg(palette.overlay)
                .border_1()
                .border_color(palette.overlay_border)
                .shadow(vec![box_shadow(px(0.), px(10.), px(28.), px(0.), hsla(0., 0., 0., 0.65))])
                .on_mouse_down_out(move |_, window, cx| close_for_outside(window, cx))
                .on_key_down(move |event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        close_for_escape(window, cx);
                    }
                })
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(spacing::X2)
                        .h(px(40.))
                        .px(spacing::X3)
                        .border_b_1()
                        .border_color(palette.separator)
                        .child(crate::icons::Icon::new(crate::icons::IconName::Search).size(px(13.)).color(palette.tertiary))
                        .child(Input::new(&self.query)),
                )
                .child({
                    let list = div().flex().flex_col().p(spacing::X1);
                    if results.is_empty() {
                        list.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .h(ROW_HEIGHT)
                                .pl(spacing::X2)
                                .child(div().text_size(type_scale::BODY.font_size).line_height(type_scale::BODY.line_height).text_color(palette.secondary).child("No matching conversations.")),
                        )
                    } else {
                        list.children(results.iter().enumerate().map(|(index, chat)| {
                            let active = index == highlighted;
                            let fg = if active { palette.on_accent } else { palette.text };
                            let muted = if active { palette.on_accent_soft } else { palette.secondary };
                            let chat_click = chat.clone();
                            div()
                                .id(ElementId::Name(format!("switcher-row-{}", chat.identifier).into()))
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(spacing::X2)
                                .h(ROW_HEIGHT)
                                .px(spacing::X2)
                                .rounded(radius::MENU_ITEM)
                                .cursor_pointer()
                                .flex_shrink_0()
                                .bg(if active { palette.accent } else { palette.overlay })
                                .on_click(cx.listener(move |this, _, window, cx| this.open(&chat_click, window, cx)))
                                .on_hover(cx.listener(move |this, hovered, _window, cx| {
                                    if *hovered {
                                        this.highlighted = index;
                                        cx.notify();
                                    }
                                }))
                                .child(avatar(None, Some(chat), px(32.), cx))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .flex_grow(1.)
                                        .min_w(px(0.))
                                        .gap(px(1.))
                                        .child(
                                            div()
                                                .text_size(type_scale::BODY.font_size)
                                                .line_height(type_scale::BODY.line_height)
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(fg)
                                                .text_ellipsis()
                                                .child(chat_title(chat)),
                                        )
                                        .child(
                                            div()
                                                .text_size(type_scale::PREVIEW.font_size)
                                                .line_height(type_scale::PREVIEW.line_height)
                                                .text_color(muted)
                                                .text_ellipsis()
                                                .child(preview_text(chat.last_message.as_ref(), chat)),
                                        ),
                                )
                        }))
                    }
                }),
        ))
    }
}
