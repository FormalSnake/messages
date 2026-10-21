//! Tier 3 on the GPUI backend (Linux, Windows): the bugs reported on g815 on 2026-10-18, each
//! driven the way the user hit it, in the real app on the demo data, with a screenshot of what
//! it shows in `$CARGO_TARGET_TMPDIR/reported/`.
//!
//! It opens a real window, so it runs only under nextest or with `ND_REAL=1`.
#![cfg(any(target_os = "linux", windows))]

use std::path::PathBuf;
use std::time::{Duration, Instant};

mod common;

use base64::Engine as _;
use nd_test::{LaunchOptions, expect};

const ALEX: &str = "chat-+14155550134";

fn real_allowed() -> bool {
    let allowed = std::env::var_os("NEXTEST").is_some() || std::env::var_os("ND_REAL").is_some_and(|v| v == "1");
    if !allowed {
        println!("skipped: opens a real window; run with ND_REAL=1");
    }
    allowed
}

fn shot(app: &common::Running, name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("reported");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.png"));
    #[cfg(target_os = "linux")]
    app.grab(&path);
    #[cfg(windows)]
    app.screenshot(&path, None).unwrap();
    println!("screenshot {}", path.display());
    path
}

fn open_alex(options: LaunchOptions) -> common::Running {
    let app = common::launch(options.env("MESSAGES_DEMO", "1"));
    let row = app.get_by_test_id(ALEX);
    expect(&row).to_be_visible().unwrap();
    let at = row.bounding_box().unwrap().expect("the row is laid out");
    app.mouse().click(at.x + at.width / 2., at.y + at.height / 2.).unwrap();
    expect(&app.get_by_test_id("thread")).to_be_visible().unwrap();
    app
}

fn alex() -> common::Running {
    open_alex(LaunchOptions::new(env!("CARGO_BIN_EXE_messages")))
}

/// React in a message's menu opens a submenu of the tapbacks beside it, and picking one puts it
/// on the bubble.
#[test]
fn real_the_message_menu_reacts_through_its_submenu() {
    if !real_allowed() {
        return;
    }
    let app = alex();
    let bubble = app.get_by_test_id("bubble-demo-msg-0012-0");
    expect(&bubble).to_be_visible().unwrap();
    let b = bubble.bounding_box().unwrap().expect("the bubble is laid out");
    // The menu opens at the press, the bubble's centre.
    let (x, y) = (b.x + b.width / 2., b.y + b.height / 2.);
    bubble.right_click().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    // React is the first row: 5 px of inset and border, then a 28 px row.
    let react = (x + 40., y + 5. + 14.);
    app.mouse().move_to(react.0, react.1).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    shot(&app, "menu-react-submenu");
    // The submenu hangs off the menu's trailing edge, its first row (Love) level with React.
    // Across the React row to the right, past the menu's 180 px minimum.
    app.mouse().move_to(react.0 + 150., react.1).unwrap();
    app.mouse().click(react.0 + 200., react.1).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    let value = bubble.value().unwrap().unwrap_or_default();
    assert!(value.contains('\u{2764}'), "the bubble shows the Love tapback: {value}");
}

/// nd's emoji picker, captured at the window's scale for the blur check.
#[cfg(target_os = "linux")]
#[test]
fn real_the_emoji_picker_is_captured() {
    if !real_allowed() {
        return;
    }
    let app = alex();
    let field = app.get_by_test_id("composer.field");
    let f = field.bounding_box().unwrap().expect("the field is laid out");
    app.mouse().click(f.x + 8., f.y + f.height / 2.).unwrap();
    let emoji = app.get_by_test_id("composer.emoji").bounding_box().unwrap().expect("the button is laid out");
    app.mouse().click(emoji.x + emoji.width / 2., emoji.y + emoji.height / 2.).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    shot(&app, "emoji-picker");
    app.keyboard().press("escape").unwrap();
}

/// The `nd-video` decoder threads of the app.
#[cfg(target_os = "linux")]
fn players(app: &common::Running) -> usize {
    let tasks = PathBuf::from(format!("/proc/{}/task", app.app.pid()));
    std::fs::read_dir(tasks)
        .map(|dir| {
            dir.filter_map(Result::ok)
                .filter(|t| std::fs::read_to_string(t.path().join("comm")).is_ok_and(|c| c.trim() == "nd-video"))
                .count()
        })
        .unwrap_or(0)
}

/// A video in the viewer stops when a click on the dim closes it: no player is left behind, and
/// opening it again runs one player, not one more each time.
#[cfg(target_os = "linux")]
#[test]
fn real_closing_the_viewer_stops_its_video() {
    if !real_allowed() {
        return;
    }
    // A silent audio sink: the headless session has no sound server, and autoaudiosink probing
    // ALSA there holds playbin's locks for seconds, so a teardown in that window waits it out.
    let app = open_alex(
        LaunchOptions::new(env!("CARGO_BIN_EXE_messages")).env("GST_PLUGIN_FEATURE_RANK", "fakeaudiosink:MAX"),
    );
    let clip = app.get_by_test_id("att-demo-video-1");
    for round in 0..3 {
        clip.scroll_into_view().ok();
        std::thread::sleep(Duration::from_millis(800));
        let c = clip.bounding_box().unwrap().expect("the video is laid out");
        app.mouse().double_click(c.x + c.width / 2., c.y + c.height / 2.).unwrap();
        let viewer = app.get_by_test_id("lightbox");
        expect(&viewer).to_be_visible().unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        assert_eq!(players(&app), 1, "one player while the viewer shows (round {round})");
        if round == 0 {
            shot(&app, "video-playing");
        }
        let v = viewer.bounding_box().unwrap().expect("the viewer is laid out");
        app.mouse().click(v.x + 20., v.y + v.height / 2.).unwrap();
        expect(&viewer).not().to_be_attached().unwrap();
        let closed = Instant::now();
        while players(&app) > 0 && closed.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(players(&app), 0, "the player stopped with the viewer (round {round})");
    }
    shot(&app, "video-closed");
}

/// A click on the conversation's name in the toolbar, as the user opens the details.
fn open_details(app: &common::Running) {
    let b = app.get_by_test_id("thread-identity").bounding_box().unwrap().expect("the name is laid out");
    app.mouse().click(b.x + b.width / 2., b.y + b.height / 2.).unwrap();
}

/// The details show Alex's Find My location with the pin at the map's centre.
#[test]
fn real_the_find_my_map_has_its_pin() {
    if !real_allowed() {
        return;
    }
    let app = alex();
    open_details(&app);
    let pin = app.get_by_test_id("map-pin-+14155550134");
    expect(&pin).to_be_visible().unwrap();
    std::thread::sleep(Duration::from_millis(600));
    shot(&app, "find-my-pin");
}

/// A GIF picker with more favourites than fit: the grid scrolls to the last.
#[test]
fn real_the_gif_favourites_scroll() {
    if !real_allowed() {
        return;
    }
    let config = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("gif-config");
    let _ = std::fs::remove_dir_all(&config);
    std::fs::create_dir_all(config.join("messages")).unwrap();
    let favorites: serde_json::Map<String, serde_json::Value> = (0..24)
        .map(|i| {
            let hue = i * 15;
            let svg = format!(
                "<svg xmlns='http://www.w3.org/2000/svg' width='84' height='84'><rect width='84' height='84' fill='hsl({hue},70%,55%)'/><text x='42' y='52' font-size='28' text-anchor='middle' fill='white'>{i}</text></svg>"
            );
            let url = format!("data:image/svg+xml;base64,{}", base64::engine::general_purpose::STANDARD.encode(svg));
            let id = format!("fav{i}");
            let gif = serde_json::json!({ "id": id, "previewUrl": url, "gifUrl": url, "width": 84, "height": 84 });
            (id, serde_json::json!({ "gif": gif, "updatedAt": 1000 - i }))
        })
        .collect();
    let json = serde_json::json!({ "server": null, "klipy": { "apiKey": "test" }, "gifFavorites": favorites });
    std::fs::write(config.join("messages/config.json"), json.to_string()).unwrap();
    let app = open_alex(LaunchOptions::new(env!("CARGO_BIN_EXE_messages")).env("XDG_CONFIG_HOME", config.display().to_string()));
    app.get_by_test_id("composer.attach").click().unwrap();
    app.get_by_test_id("gif").click().unwrap();
    let results = app.get_by_test_id("gif-results");
    expect(&results).to_be_visible().unwrap();
    std::thread::sleep(Duration::from_millis(600));
    shot(&app, "gif-favourites-top");
    let r = results.bounding_box().unwrap().expect("the grid is laid out");
    let last = app.get_by_test_id("gif-fav23");
    let before = last.bounding_box().unwrap().expect("the last favourite is laid out");
    assert!(before.y > r.y + r.height, "the last favourite starts below the grid's edge");
    for _ in 0..12 {
        app.mouse().move_to(r.x + r.width / 2., r.y + r.height / 2.).unwrap();
        results.scroll(0., 120.).unwrap();
        std::thread::sleep(Duration::from_millis(60));
    }
    std::thread::sleep(Duration::from_millis(600));
    shot(&app, "gif-favourites-scrolled");
    let after = last.bounding_box().unwrap().expect("the last favourite is laid out");

    assert!(after.y + after.height <= r.y + r.height + 1., "the last favourite scrolled into the grid: {after:?} in {r:?}");
}

/// The details inspector slides in: part way through, it shows a part of its width.
#[test]
fn real_the_details_slide_open() {
    if !real_allowed() {
        return;
    }
    let app = alex();
    let thread = app.get_by_test_id("thread");
    let start = thread.bounding_box().unwrap().expect("the thread is laid out").width;
    open_details(&app);
    let mut widths = Vec::new();
    let began = Instant::now();
    let mut captured = false;
    while began.elapsed() < Duration::from_millis(900) {
        if let Ok(Some(b)) = thread.bounding_box() {
            widths.push(b.width);
            if !captured && b.width < start - 20. && b.width > start - 200. {
                shot(&app, "details-sliding");
                captured = true;
            }
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    let end = *widths.last().expect("measured");
    shot(&app, "details-open");
    assert!(end < start - 100., "the details opened: {start} to {end}");
    let between = widths.iter().filter(|w| **w < start - 2. && **w > end + 2.).count();
    assert!(between >= 2, "the thread narrowed over frames, not at once: {widths:?}");
}

/// The tray item in a real StatusNotifierItem host (waybar, from `WAYBAR`) shows the app's icon,
/// not a missing-icon placeholder: Messages names its tray icon after SF Symbols, which no Linux
/// icon theme has. Run under `dbus-run-session`, so the item and the host share a private bus.
#[cfg(target_os = "linux")]
#[test]
fn real_the_tray_shows_the_app_icon() {
    if !real_allowed() {
        return;
    }
    let Ok(waybar) = std::env::var("WAYBAR") else {
        println!("skipped: set WAYBAR to a waybar binary");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("waybar");
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("config.json");
    std::fs::write(
        &config,
        r#"{"layer":"top","position":"top","height":40,"modules-right":["tray"],"tray":{"icon-size":32}}"#,
    )
    .unwrap();
    let style = dir.join("style.css");
    std::fs::write(&style, "* { background: #303030; }").unwrap();
    // The icon where the package installs it, under a data directory of the app's own.
    let data = dir.join("data");
    let icon = data.join("icons/hicolor/512x512/apps/dev.formalsnake.messages.png");
    std::fs::create_dir_all(icon.parent().unwrap()).unwrap();
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packaging/linux/dev.formalsnake.messages.png"), &icon).unwrap();
    let app = open_alex(
        LaunchOptions::new(env!("CARGO_BIN_EXE_messages")).env("XDG_DATA_HOME", data.display().to_string()),
    );
    let mut bar = app.client(&waybar).arg("-c").arg(&config).arg("-s").arg(&style).spawn().unwrap();
    std::thread::sleep(Duration::from_secs(4));
    let names = std::process::Command::new("busctl").args(["--user", "list", "--no-legend"]).output().unwrap();
    let names = String::from_utf8_lossy(&names.stdout).into_owned();
    let item = names
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .find(|n| n.starts_with("org.kde.StatusNotifierItem-"))
        .expect("an item on the bus")
        .to_owned();
    let get = |property: &str| {
        let out = std::process::Command::new("busctl")
            .args(["--user", "get-property", &item, "/StatusNotifierItem", "org.kde.StatusNotifierItem", property])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().chars().take(120).collect::<String>()
    };
    let (name, theme, pixmap) = (get("IconName"), get("IconThemePath"), get("IconPixmap"));
    println!("IconName {name}\nIconThemePath {theme}\nIconPixmap {pixmap}");
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("reported/tray.png");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    let status = app.client("grim").arg("-g").arg("0,0 1400x40").arg(&out).status().unwrap();
    assert!(status.success());
    println!("screenshot {}", out.display());
    bar.kill().ok();
    bar.wait().ok();
    assert_eq!(name, "s \"dev.formalsnake.messages\"");
    assert!(!pixmap.starts_with("a(iiay) 0"), "the app icon goes out as a pixmap too: {pixmap}");
}
