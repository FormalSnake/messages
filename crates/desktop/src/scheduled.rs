//! The list of scheduled sends for one chat, shown above the composer.
//! Owned by D3; not mounted directly by app.rs (`composer::Composer` renders it).

use gpui_kit::*;
use messages_core::ScheduledMessage;

use crate::bridge::{Bridge, StoreHandle, Topic};
use crate::icons::{Icon, IconName};
use crate::primitives::IconButton;
use crate::theme::{Theme, radius, spacing, type_scale};

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub struct ScheduledList {
    chat_guid: String,
}

impl ScheduledList {
    pub fn new(chat_guid: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade().into();
        Bridge::watch(cx, Topic::Scheduled, weak);
        let _ = window;
        Self { chat_guid }
    }

    /// `app.rs` mounts one composer per window, not per chat, so this is how
    /// the chat guid follows the selected conversation.
    pub fn set_chat(&mut self, chat_guid: String, cx: &mut Context<Self>) {
        if self.chat_guid != chat_guid {
            self.chat_guid = chat_guid;
            cx.notify();
        }
    }

    fn cancel(&self, id: &str, cx: &mut Context<Self>) {
        let Some(store) = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone()) else { return };
        let id = id.to_owned();
        let task_store = store.clone();
        store.spawn(async move { task_store.cancel_scheduled(&id).await });
    }
}

impl Render for ScheduledList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Theme::get(cx);
        let Some(store) = cx.try_global::<StoreHandle>().and_then(|handle| handle.0.clone()) else {
            return div().into_any_element();
        };
        let items: Vec<ScheduledMessage> = store.state().scheduled.iter().filter(|item| item.chat_guid == self.chat_guid).cloned().collect();
        if items.is_empty() {
            return div().into_any_element();
        }
        let now = now_ms();


        div().id("scheduled-list").flex().flex_col().children(items.into_iter().map(|item| {
            let id = item.id.clone();
            div()
                .id(ElementId::Name(format!("scheduled-{id}").into()))
                .flex()
                .flex_row()
                .items_center()
                .gap(spacing::X2)
                .mb(spacing::X2)
                .pl(spacing::X2)
                .pr(spacing::X1)
                .py(spacing::X1)
                .rounded(radius::CONTROL)
                .bg(palette.raised)
                .child(Icon::new(IconName::Schedule).size(px(14.)).color(palette.accent))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_grow(1.)
                        .min_w(px(0.))
                        .child(div().text_size(type_scale::CAPTION.font_size).line_height(type_scale::CAPTION.line_height).text_color(palette.text).child(item.text.clone()))
                        .child(
                            div()
                                .text_size(type_scale::MICRO.font_size)
                                .line_height(type_scale::MICRO.line_height)
                                .text_color(palette.secondary)
                                .child(format!("Sends at {}", messages_core::format::format_scheduled_for(item.send_at, now))),
                        ),
                )
                .child(IconButton::new(ElementId::Name(format!("cancel-{id}").into()), IconName::Close, "Cancel send").size(px(12.)).hit(px(24.)).color(palette.secondary).on_click(
                    cx.listener(move |this, _, _window, cx| this.cancel(&id, cx)),
                ))
        })).into_any_element()
    }
}
