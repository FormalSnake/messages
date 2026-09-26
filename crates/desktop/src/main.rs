mod app;
mod assets;
mod bridge;
mod chrome;
mod confirm;
mod emoji_font;
mod connect;
mod icons;
mod live_theme;
mod menus;
mod motion;
mod primitives;
mod theme;
mod toast;
mod trace;

// D1 sidebar
mod new_chat;
mod search_results;
mod sidebar;
mod sidebar_row;
mod switcher;

// D2 thread
mod attachments;
mod bubble;
mod gif;
mod lightbox;
mod reply_thread;
mod stills;
mod thread;
mod thread_rows;

// D3 header and composer
mod composer;
mod details;
mod facetime;
mod gif_picker;
mod header;
mod location;
mod scheduled;

use gpui_kit::component::{Root, TitleBar};
use gpui_kit::*;

use app::AppRoot;

/// GTK/Wayland and X11 window manager application id (`WM_CLASS` on X11), used
/// to group and identify the window; matches the `.desktop` file's own name so
/// the shell can find the app's icon by id.
const APP_ID: &str = "es.canarycoders.messages";

fn window_options(cx: &App) -> WindowOptions {
    let mut options = TitleBar::window_options();
    options.titlebar = Some(TitlebarOptions {
        title: Some("Messages".into()),
        // Matches `apps/desktop/app.tsx`'s `trafficLightX`/`trafficLightY` (16, 18).
        traffic_light_position: Some(point(px(16.), px(18.))),
        ..TitleBar::title_bar_options()
    });
    options.window_bounds = Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1120.), px(760.)), cx)));
    options.window_min_size = Some(size(px(720.), px(480.)));
    options.app_id = Some(APP_ID.into());
    // Same name as the TS app's `GPUIX_BACKGROUND` so existing screenshot/test
    // scripts keep working unchanged.
    options.focus = std::env::var("GPUIX_BACKGROUND").ok().as_deref() != Some("1");
    options
}

fn main() {
    trace::init();
    // Every network call, timer, file read or write, JSON parse, image decode
    // and ffmpeg run happens here, never on the GPUI foreground thread.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("messages-rt")
        .enable_all()
        .build()
        .expect("build tokio runtime");
    // Leaked so its Handle stays valid for the process; the runtime itself is
    // never meant to shut down before the process does.
    let runtime_handle = Box::leak(Box::new(runtime)).handle().clone();

    gpui_kit::application().with_assets(icons::IconAssets).run(move |cx| {
        trace::log_if_enabled("platform up");
        // gpui-component resolves ".SystemUIFont" and the platform monospace
        // default by listing every installed font (about 200 ms through
        // CoreText) unless the theme already names families. The app never
        // shows monospace text and names its own sans, so both are set first
        // and the default light and dark configs are put back afterwards.
        {
            use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};
            let mut named = Theme::default();
            named.font_family = theme::font_sans();
            named.mono_font_family = theme::font_sans();
            cx.set_global(named);
            gpui_kit::init(cx);
            let registry = ThemeRegistry::global(cx);
            let (light, dark) = (registry.default_light_theme().clone(), registry.default_dark_theme().clone());
            let component = Theme::global_mut(cx);
            component.light_theme = light;
            component.dark_theme = dark;
            Theme::change(ThemeMode::Light, None, cx);
        }
        app::init(cx);
        theme::Theme::install(cx);
        live_theme::watch(cx);
        emoji_font::install(cx);
        bridge::Bridge::install(cx);
        trace::watch_keys(cx);
        trace::log_if_enabled("app initialised");

        let runtime_handle = runtime_handle.clone();
        cx.spawn(async move |cx| {
            let options = cx.update(|cx| window_options(cx));
            cx.open_window(options, |window, cx| {
                trace::log_if_enabled("window created");
                let view = cx.new(|cx| AppRoot::new(runtime_handle, window, cx));
                trace::log_if_enabled("views built");
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("open window");
        })
        .detach();
    });
}
