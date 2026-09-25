mod icons;
mod motion;
mod theme;

use gpui_kit::component::{Root, TitleBar};
use gpui_kit::*;

/// GTK/Wayland and X11 window manager application id (`WM_CLASS` on X11), used
/// to group and identify the window; matches the `.desktop` file's own name so
/// the shell can find the app's icon by id.
const APP_ID: &str = "es.canarycoders.messages";

struct Shell;

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::Theme::get(cx);
        component::v_flex()
            .size_full()
            .bg(palette.canvas)
            .child(TitleBar::new().child(div().text_color(palette.text).child("Messages")))
            .child(div().flex_1())
    }
}

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
    options
}

fn main() {
    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(|cx| {
        gpui_kit::init(cx);
        theme::Theme::install(cx);
        theme::watch_theme_file(cx);

        cx.spawn(async move |cx| {
            let options = cx.update(|cx| window_options(cx));
            cx.open_window(options, |window, cx| {
                let view = cx.new(|_| Shell);
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("open window");
        })
        .detach();
    });
}
