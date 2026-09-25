//! The behaviours `apps/desktop/app.test.tsx` checked, driven through GPUI's
//! test platform against the demo transport. The store runs on a real tokio
//! runtime (the demo replies on real timers), so waits poll with a deadline.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
use messages_core::agent::ChatPrefs;
use messages_core::config::Config;
use messages_core::conversations::conversation_messages;

use super::*;

const ALEX: &str = "iMessage;-;+14155550134";
const FAMILY: &str = "iMessage;+;chat240119384759";
const NADIA: &str = "iMessage;-;+34612345678";

fn runtime() -> tokio::runtime::Handle {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("tokio runtime")).handle().clone()
}

/// Keeps config writes (pins, GIF favorites) and the attachment cache out of
/// the real `~/.config/messages`.
fn isolate_dirs() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let home = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-home");
        // SAFETY: set once, before any test thread reads these variables.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", home.join("config"));
            std::env::set_var("XDG_CACHE_HOME", home.join("cache"));
        }
    });
}

fn demo_config(pinned: &[&str]) -> Config {
    let chats: HashMap<String, ChatPrefs> = pinned.iter().map(|guid| (guid.to_string(), ChatPrefs { pinned: Some(true), ..Default::default() })).collect();
    Config { demo: true, notifications: false, chats, ..Default::default() }
}

fn boot(cx: &mut TestAppContext, config: Config) -> (Entity<AppRoot>, &mut VisualTestContext) {
    isolate_dirs();
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
        crate::theme::Theme::install(cx);
        crate::bridge::Bridge::install(cx);
        cx.set_reduce_motion(true);
    });
    let runtime = runtime();
    let (root, cx) = cx.add_window_view(|window, cx| AppRoot::with_config(runtime, Some(config), window, cx));
    wait_until(cx, "the composer", |cx| cx.debug_bounds("composer").is_some());
    (root, cx)
}

/// Pumps GPUI (and its fake clock) until `check` holds, for up to 20 s of real time.
fn wait_until(cx: &mut VisualTestContext, what: &str, mut check: impl FnMut(&mut VisualTestContext) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        cx.run_until_parked();
        let _ = cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        if check(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
        cx.executor().advance_clock(Duration::from_millis(20));
    }
}

fn store(cx: &mut VisualTestContext) -> MessagesStore {
    cx.update(|_, cx| crate::bridge::store(cx)).expect("store")
}

fn texts(cx: &mut VisualTestContext, chat: &str) -> Vec<String> {
    let store = store(cx);
    let state = store.state();
    conversation_messages(&state, chat).iter().map(|message| message.text.clone()).collect()
}

fn selected_title(root: &Entity<AppRoot>, cx: &mut VisualTestContext) -> Option<String> {
    let header = cx.update(|_, cx| root.read(cx).header.clone());
    cx.update(|_, cx| header.read(cx).title(cx))
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is not painted"));
    cx.simulate_click(bounds.center(), Modifiers::none());
}

#[::core::prelude::v1::test]
fn opens_on_the_most_recent_conversation() {
    let mut app = TestAppContext::single();
    let cx = &mut app;
    let (root, cx) = boot(cx, demo_config(&[]));
    wait_until(cx, "Alex selected", |cx| selected_title(&root, cx).as_deref() == Some("Alex Rivera"));
    assert!(texts(cx, ALEX).iter().any(|text| text == "coffee at 4? the place on valencia"));
}

#[::core::prelude::v1::test]
fn send_shows_the_canned_reply() {
    let mut app = TestAppContext::single();
    let cx = &mut app;
    let (_root, cx) = boot(cx, demo_config(&[]));
    cx.simulate_input("see you there");
    click(cx, "send");
    wait_until(cx, "my message", |cx| texts(cx, ALEX).iter().any(|text| text == "see you there"));
    wait_until(cx, "the canned reply", |cx| texts(cx, ALEX).iter().any(|text| text == "ha, deal"));
}

#[::core::prelude::v1::test]
fn switching_chats_updates_title_and_thread() {
    let mut app = TestAppContext::single();
    let cx = &mut app;
    let (root, cx) = boot(cx, demo_config(&[]));
    wait_until(cx, "the Family row", |cx| cx.debug_bounds("chat-iMessage;+;chat240119384759").is_some());
    click(cx, "chat-iMessage;+;chat240119384759");
    wait_until(cx, "Family in the header", |cx| selected_title(&root, cx).as_deref() == Some("Family"));
    wait_until(cx, "Family's thread", |cx| texts(cx, FAMILY).iter().any(|text| text == "Sunday lunch is at ours, 1pm. Bring the good bread."));
}

#[::core::prelude::v1::test]
fn pinned_avatar_click_opens_its_conversation() {
    let mut app = TestAppContext::single();
    let cx = &mut app;
    let (root, cx) = boot(cx, demo_config(&[FAMILY, "SMS;-;+14155550188", "iMessage;+;chat881204957120"]));
    wait_until(cx, "the pinned strip", |cx| cx.debug_bounds("pinned-iMessage;+;chat240119384759").is_some());
    click(cx, "pinned-iMessage;+;chat240119384759");
    wait_until(cx, "Family", |cx| selected_title(&root, cx).as_deref() == Some("Family"));
    click(cx, "pinned-SMS;-;+14155550188");
    wait_until(cx, "Jordan", |cx| selected_title(&root, cx).as_deref() == Some("Jordan Lee"));
    click(cx, "pinned-iMessage;+;chat881204957120");
    wait_until(cx, "Design crit", |cx| selected_title(&root, cx).as_deref() == Some("Design crit"));
}

#[::core::prelude::v1::test]
fn new_chat_flow_opens_the_new_thread() {
    let mut app = TestAppContext::single();
    let cx = &mut app;
    let (root, cx) = boot(cx, demo_config(&[]));
    click(cx, "new-message");
    wait_until(cx, "the new chat screen", |cx| cx.update(|_, cx| root.read(cx).new_chat_view.is_some()));
    cx.simulate_input("ben");
    cx.simulate_keystrokes("enter");
    let view = cx.update(|_, cx| root.read(cx).new_chat_view.clone()).expect("new chat");
    cx.update(|window, cx| view.update(cx, |view, cx| view.focus_draft(window, cx)));
    cx.simulate_input("PR looks good");
    cx.simulate_keystrokes("enter");
    wait_until(cx, "the new thread", |cx| !cx.update(|_, cx| root.read(cx).new_chat) && selected_title(&root, cx).as_deref() == Some("Ben Okafor"));
    let chat = store(cx).state().selected_chat.clone().expect("selected");
    wait_until(cx, "the first message", |cx| texts(cx, &chat).iter().any(|text| text == "PR looks good"));
}

#[::core::prelude::v1::test]
fn info_panel_animates_out_before_unmounting() {
    let mut app = TestAppContext::single();
    let cx = &mut app;
    let (root, cx) = boot(cx, demo_config(&[]));
    click(cx, "info");
    wait_until(cx, "the details panel", |cx| cx.debug_bounds("info-panel").is_some());
    click(cx, "close-details");
    cx.run_until_parked();
    assert!(cx.update(|_, cx| root.read(cx).info_shown.is_mounted()), "the panel unmounted before sliding out");
    cx.executor().advance_clock(motion::DURATION_PANEL + Duration::from_millis(20));
    wait_until(cx, "the panel to leave", |cx| cx.debug_bounds("info-panel").is_none());
}

#[::core::prelude::v1::test]
fn gallery_shows_photos_and_files() {
    let mut app = TestAppContext::single();
    let cx = &mut app;
    let (root, cx) = boot(cx, demo_config(&[]));
    wait_until(cx, "Nadia's row", |cx| cx.debug_bounds("chat-iMessage;-;+34612345678").is_some());
    click(cx, "chat-iMessage;-;+34612345678");
    wait_until(cx, "Nadia", |cx| selected_title(&root, cx).as_deref() == Some("Nadia Haddad"));
    cx.dispatch_action(ToggleInfo);
    wait_until(cx, "the gallery photo", |cx| cx.debug_bounds("gallery-photo-demo-att-3").is_some());
    assert!(conversation_messages(&store(cx).state(), NADIA).iter().any(|message| message.attachments.iter().any(|item| item.guid == "demo-att-3")));
}

#[::core::prelude::v1::test]
fn reply_shows_the_banner_and_sending_clears_it() {
    let mut app = TestAppContext::single();
    let cx = &mut app;
    let (_root, cx) = boot(cx, demo_config(&[]));
    let target = {
        let store = store(cx);
        let state = store.state();
        conversation_messages(&state, ALEX).iter().find(|message| message.text == "coffee at 4? the place on valencia").map(|message| message.guid.clone()).expect("fixture")
    };
    store(cx).set_replying_to(ALEX, Some(&target));
    wait_until(cx, "the reply banner", |cx| cx.debug_bounds("reply-banner").is_some());
    cx.simulate_input("on my way");
    click(cx, "send");
    wait_until(cx, "the reply", |cx| texts(cx, ALEX).iter().any(|text| text == "on my way"));
    cx.executor().advance_clock(motion::DURATION_BASE + Duration::from_millis(20));
    wait_until(cx, "the banner to leave", |cx| cx.debug_bounds("reply-banner").is_none());
    let store = store(cx);
    let state = store.state();
    let sent = conversation_messages(&state, ALEX).into_iter().find(|message| message.text == "on my way").expect("sent");
    assert_eq!(sent.reply_to.as_deref(), Some(target.as_str()));
}

#[::core::prelude::v1::test]
fn escape_closes_the_topmost_overlay_first() {
    let mut app = TestAppContext::single();
    let cx = &mut app;
    let (root, cx) = boot(cx, demo_config(&[]));
    cx.dispatch_action(ToggleInfo);
    cx.dispatch_action(OpenSwitcher);
    cx.run_until_parked();
    assert!(cx.update(|_, cx| root.read(cx).switcher.is_some()));
    cx.simulate_keystrokes("escape");
    assert!(cx.update(|_, cx| root.read(cx).switcher.is_none() && root.read(cx).info_open));
    cx.simulate_keystrokes("escape");
    assert!(cx.update(|_, cx| !root.read(cx).info_open));
}

#[::core::prelude::v1::test]
fn empty_sidebar_space_offers_new_message() {
    let mut app = TestAppContext::single();
    let cx = &mut app;
    let (root, cx) = boot(cx, demo_config(&[]));
    let area = cx.debug_bounds("sidebar-list-area").expect("list area");
    let below_rows = gpui_kit::point(area.center().x, area.bottom() - gpui_kit::px(4.));
    cx.simulate_event(gpui_kit::MouseDownEvent { position: below_rows, button: gpui_kit::MouseButton::Right, modifiers: Modifiers::none(), click_count: 1, first_mouse: false });
    cx.simulate_event(gpui_kit::MouseUpEvent { position: below_rows, button: gpui_kit::MouseButton::Right, modifiers: Modifiers::none(), click_count: 1 });
    let menu_len = |cx: &mut VisualTestContext| cx.update(|_, cx| root.read(cx).menu.as_ref().map(|menu| menu.read(cx).len()));
    assert_eq!(menu_len(cx), Some(1), "the empty list area offers only New message");

    // A row keeps its own menu: the list area must not replace it.
    let row = cx.debug_bounds("chat-iMessage;+;chat240119384759").expect("row").center();
    cx.simulate_event(gpui_kit::MouseDownEvent { position: row, button: gpui_kit::MouseButton::Right, modifiers: Modifiers::none(), click_count: 1, first_mouse: false });
    cx.simulate_event(gpui_kit::MouseUpEvent { position: row, button: gpui_kit::MouseButton::Right, modifiers: Modifiers::none(), click_count: 1 });
    assert!(menu_len(cx).is_some_and(|len| len > 1), "the row's own menu was replaced");
}

#[::core::prelude::v1::test]
fn escape_in_the_composer_drops_the_reply_before_the_panel() {
    let mut app = TestAppContext::single();
    let cx = &mut app;
    let (root, cx) = boot(cx, demo_config(&[]));
    let target = conversation_messages(&store(cx).state(), ALEX).first().map(|message| message.guid.clone()).expect("fixture");
    store(cx).set_replying_to(ALEX, Some(&target));
    cx.dispatch_action(ToggleInfo);
    wait_until(cx, "the reply banner", |cx| cx.debug_bounds("reply-banner").is_some());
    let composer = cx.update(|_, cx| root.read(cx).composer.clone());
    cx.update(|window, cx| composer.update(cx, |composer, cx| composer.focus(window, cx)));
    cx.simulate_keystrokes("escape");
    assert!(!store(cx).state().replying_to.contains_key(ALEX), "Escape left the reply up");
    assert!(cx.update(|_, cx| root.read(cx).info_open), "Escape closed the panel before the reply");
    cx.simulate_keystrokes("escape");
    assert!(cx.update(|_, cx| !root.read(cx).info_open));
}

