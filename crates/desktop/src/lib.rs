//! The Messages window on NativeDesktop. `root.rs` lays out sidebar, conversation and details;
//! `bridge.rs` is the only place store events become view notifies; everything that is not
//! pixels lives in `messages-core`.

pub mod actions;
pub mod bridge;
pub mod composer;
pub mod connect;
pub mod details;
pub mod header;
pub mod lightbox;
pub mod message;
pub mod migrate;
pub mod new_chat;
pub mod notify;
pub mod root;
pub mod rows;
pub mod rt;
pub mod settings;
pub mod sidebar;
pub mod switcher;
pub mod thread;
pub mod ui;

use nd::prelude::*;

pub const DEFAULT_SIZE: (f32, f32) = (1120., 760.);
pub const MIN_SIZE: (f32, f32) = (720., 480.);

pub fn app() -> Application {
    Application::new(nd::manifest!())
}

/// Everything the window needs before it opens: settings, key bindings, menus, the bridge.
pub fn init(cx: &mut App) {
    settings::install(cx);
    actions::init(cx);
    composer::init(cx);
    bridge::Bridge::install(cx);
    notify::install(cx);
}

pub fn open(cx: &mut App) -> WindowHandle<root::Root> {
    let options = WindowOptions::main("main")
        .title("Messages")
        .size(size(px(DEFAULT_SIZE.0), px(DEFAULT_SIZE.1)))
        .min_size(size(px(MIN_SIZE.0), px(MIN_SIZE.1)));
    cx.open_window(options, |window, cx| cx.new(|cx| root::Root::new(window, cx)))
}

/// The window on the demo conversations with Alex's thread open, the store started before the
/// window opens. For the appearance audit and screenshots, which render one settled frame and
/// run none of the app's async work.
pub fn open_demo(cx: &mut App) -> WindowHandle<root::Root> {
    init(cx);
    let config = messages_core::config::Config { demo: true, ..Default::default() };
    let store = {
        let runtime = rt::handle();
        let _guard = runtime.enter();
        root::build_store(&config, runtime.clone())
    };
    rt::handle().block_on(async {
        store.start().await;
        store.select_chat(Some("iMessage;-;+14155550134")).await;
    });
    cx.set_global(bridge::ConfigHandle(config.clone()));
    let options = WindowOptions::main("main").title("Messages").size(size(px(DEFAULT_SIZE.0), px(DEFAULT_SIZE.1)));
    cx.open_window(options, move |window, cx| cx.new(|cx| root::Root::with_store(config, store, window, cx)))
}

pub fn run() -> nd::Result<()> {
    migrate::run();
    // A second launch hands over to the first and exits; demo and test runs skip the lock.
    let instance = if settings::demo_requested() { None } else { rt::claim_instance() };
    if matches!(instance, Some(rt::Claim::Forwarded)) {
        return Ok(());
    }
    app().run(move |cx| {
        init(cx);
        open(cx);
        if let Some(rt::Claim::Primary(primary)) = instance {
            rt::raise_on_launch(primary, cx);
        }
    })
}
