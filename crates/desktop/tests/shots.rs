//! Screenshots of the real app on the demo data, one per conversation, for the parity checklist
//! (docs/nd-parity.md, "shot"). `SHOT_CHATS` names the sidebar test ids to open (comma
//! separated); without it every demo conversation is captured. The pictures land in the test's
//! target tmp dir and the path is printed.
//!
//! It opens a real window, so it runs only under nextest or with `ND_REAL=1`.

use std::time::Duration;

use nd_test::{LaunchOptions, expect, launch_app};

const CHATS: &[&str] = &[
    "chat-+14155550134",
    "chat-chat240119384759",
    "chat-priya.natarajan@icloud.com",
    "chat-+14155550188",
    "chat-chat881204957120",
    "chat-+14155550199",
    "chat-+34612345678",
    "chat-+14155550170",
    "chat-chloe@martin.design",
];

#[test]
fn real_every_demo_conversation_is_captured() {
    if !real_allowed() {
        println!("skipped: opens a real window; run with ND_REAL=1");
        return;
    }
    let wanted = std::env::var("SHOT_CHATS").ok().filter(|w| !w.trim().is_empty());
    let chats: Vec<&str> = match wanted.as_deref() {
        Some(list) => list.split(',').map(str::trim).filter(|s| !s.is_empty()).collect(),
        None => CHATS.to_vec(),
    };
    #[cfg(target_os = "linux")]
    let compositor = ndshell::hyprland::Hyprland::start(ndshell::hyprland::Options {
        width: 1400,
        height: 900,
        ..Default::default()
    })
    .unwrap();
    // `SHOT_VARIANTS`: `<preset>-<scheme>` pairs, comma separated (`fluent-dark`); `native` is
    // the platform's own look, `system` its own scheme.
    let variants = std::env::var("SHOT_VARIANTS").unwrap_or_default();
    let variants: Vec<&str> = variants.split(',').map(str::trim).filter(|v| !v.is_empty()).collect();
    for variant in if variants.is_empty() { vec!["native-system"] } else { variants } {
        let (preset, scheme) = variant.split_once('-').unwrap_or((variant, "system"));
        let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("shots").join(variant);
        std::fs::create_dir_all(&dir).unwrap();
        let mut options = LaunchOptions::new(env!("CARGO_BIN_EXE_messages")).env("MESSAGES_DEMO", "1");
        if preset != "native" {
            options = options.env("ND_PRESET", preset);
        }
        if scheme != "system" {
            let toml = dir.join("theme.toml");
            std::fs::write(&toml, format!("appearance = \"{scheme}\"\n")).unwrap();
            options = options.env("ND_THEME_TOML", toml.display().to_string());
        }
        #[cfg(target_os = "linux")]
        for (key, value) in compositor.client_env() {
            options = options.env(key, value);
        }
        let app = launch_app(options).unwrap();
        for chat in &chats {
            let row = app.get_by_test_id(chat);
            row.scroll_into_view().ok();
            expect(&row).to_be_visible().unwrap();
            let bounds = row.bounding_box().unwrap().expect("the row is laid out");
            app.mouse().click(bounds.x + bounds.width / 2., bounds.y + bounds.height / 2.).unwrap();
            expect(&app.get_by_test_id("thread")).to_be_visible().unwrap();
            // Pictures decode off the main thread; give them a moment before the capture.
            std::thread::sleep(Duration::from_millis(1500));
            let shot = dir.join(format!("shot-{}.png", chat.trim_start_matches("chat-").replace(['+', '@', '.'], "_")));
            #[cfg(target_os = "linux")]
            grab(&compositor, app.pid(), &shot);
            #[cfg(target_os = "macos")]
            grab_macos(app.pid(), &shot);
            println!("screenshot {}", shot.display());
        }
        // A draft in the field, for the send button inside it.
        app.get_by_test_id("composer.field").fill("Running late, see you at 8").unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let shot = dir.join("shot-draft.png");
        #[cfg(target_os = "linux")]
        grab(&compositor, app.pid(), &shot);
        #[cfg(target_os = "macos")]
        grab_macos(app.pid(), &shot);
        println!("screenshot {}", shot.display());
    }
}

/// The app's window from the headless output: GPUI on Linux reads no frame back from a real
/// window, so the compositor's screencopy (grim) takes it.
#[cfg(target_os = "linux")]
fn grab(compositor: &ndshell::hyprland::Hyprland, pid: u32, out: &std::path::Path) {
    let client = compositor.wait_for_client(pid, Duration::from_secs(10)).unwrap();
    let geometry = format!("{},{} {}x{}", client.at.0, client.at.1, client.size.0, client.size.1);
    let status = compositor.command("grim").arg("-g").arg(&geometry).arg(out).status().unwrap();
    assert!(status.success(), "grim -g {geometry} failed");
}

/// The whole window, toolbar and sidebar included, as the system draws it.
#[cfg(target_os = "macos")]
fn grab_macos(pid: u32, out: &std::path::Path) {
    let main = windows_of(pid).first().map(|w| w.0).expect("the app has a window");
    let _ = std::process::Command::new("screencapture").args(["-x", "-o", "-l", &main.to_string()]).arg(out).status();
}

/// The on-screen windows of `pid`, largest first: (id, width, height).
fn windows_of(pid: u32) -> Vec<(u32, u32, u32)> {
    let script = format!(
        "import CoreGraphics\nlet l = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]\nfor w in l where (w[\"kCGWindowOwnerPID\"] as? Int) == {pid} {{ let b = w[\"kCGWindowBounds\"] as! [String: Any]; print(w[\"kCGWindowNumber\"]!, b[\"Width\"]!, b[\"Height\"]!) }}"
    );
    let out = std::process::Command::new("swift").args(["-e", &script]).output().expect("swift runs");
    let mut found: Vec<(u32, u32, u32)> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let n: Vec<u32> = l.split_whitespace().filter_map(|p| p.parse::<f64>().ok().map(|f| f as u32)).collect();
            (n.len() == 3).then(|| (n[0], n[1], n[2]))
        })
        .collect();
    found.retain(|w| w.1 > 100 && w.2 > 100);
    found.sort_by_key(|w| std::cmp::Reverse(w.1 * w.2));
    found
}

fn real_allowed() -> bool {
    std::env::var_os("NEXTEST").is_some() || std::env::var_os("ND_REAL").is_some_and(|v| v == "1")
}

/// Alex's conversation open, the pointer over its newest message, then the video in the
/// lightbox: captured for the checklist (hover time, picture tails, in-app playback).
#[test]
fn real_hover_and_lightbox_video_are_captured() {
    if !real_allowed() {
        println!("skipped: opens a real window; run with ND_REAL=1");
        return;
    }
    let app = launch_app(
        LaunchOptions::new(env!("CARGO_BIN_EXE_messages"))
            .env("MESSAGES_DEMO", "1")
            .env("ND_AUTOMATION_CAPTURE", "region"),
    )
    .unwrap();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let row = app.get_by_test_id("chat-+14155550134");
    expect(&row).to_be_visible().unwrap();
    let bounds = row.bounding_box().unwrap().expect("the row is laid out");
    app.mouse().click(bounds.x + bounds.width / 2., bounds.y + bounds.height / 2.).unwrap();
    expect(&app.get_by_test_id("thread")).to_be_visible().unwrap();
    std::thread::sleep(Duration::from_millis(1500));

    let last = app.get_by_test_id("row-demo-msg-0014").bounding_box().unwrap().expect("the last row is laid out");
    app.mouse().move_to(last.x + last.width / 2., last.y + last.height / 2.).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let shot = dir.join("shot-hover.png");
    app.screenshot(&shot, None).unwrap();
    println!("screenshot {}", shot.display());

    let video = app.get_by_test_id("att-demo-video-1");
    video.scroll_into_view().ok();
    std::thread::sleep(Duration::from_millis(500));
    let at = video.bounding_box().unwrap().expect("the video block is laid out");
    app.mouse().click(at.x + at.width / 2., at.y + at.height / 2.).unwrap();
    // Quick Look opens in its own panel: find it among the app's windows and capture it.
    std::thread::sleep(Duration::from_millis(2000));
    let windows = windows_of(app.pid());
    println!("windows {windows:?}");
    assert!(windows.len() >= 2, "Quick Look opened a panel beside the main window: {windows:?}");
    for (i, (id, _, _)) in windows.iter().enumerate() {
        let shot = dir.join(format!("shot-quicklook-{i}.png"));
        let _ =
            std::process::Command::new("screencapture").args(["-x", "-o", "-l", &id.to_string()]).arg(&shot).status();
        println!("screenshot {}", shot.display());
    }
    println!("{}", app.snapshot().unwrap());
}

/// Alex's video double-clicked: the viewer covers the window and plays it, a click on the
/// picture pauses it where it is and another plays on. `ND_PRESET` picks the look.
#[cfg(any(target_os = "linux", windows))]
#[test]
fn real_a_double_clicked_video_plays_in_the_viewer() {
    if !real_allowed() {
        println!("skipped: opens a real window; run with ND_REAL=1");
        return;
    }
    #[cfg(target_os = "linux")]
    let compositor = ndshell::hyprland::Hyprland::start(ndshell::hyprland::Options {
        width: 1400,
        height: 900,
        ..Default::default()
    })
    .unwrap();
    let mut options = LaunchOptions::new(env!("CARGO_BIN_EXE_messages")).env("MESSAGES_DEMO", "1");
    if let Ok(preset) = std::env::var("ND_PRESET") {
        options = options.env("ND_PRESET", preset);
    }
    #[cfg(target_os = "linux")]
    for (key, value) in compositor.client_env() {
        options = options.env(key, value);
    }
    let app = launch_app(options).unwrap();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("viewer");
    std::fs::create_dir_all(&dir).unwrap();
    let shoot = |name: &str| {
        let shot = dir.join(name);
        #[cfg(target_os = "linux")]
        grab(&compositor, app.pid(), &shot);
        #[cfg(windows)]
        grab_windows(app.pid(), &shot);
        println!("screenshot {}", shot.display());
        shot
    };
    let row = app.get_by_test_id("chat-+14155550134");
    expect(&row).to_be_visible().unwrap();
    let at = row.bounding_box().unwrap().expect("the row is laid out");
    app.mouse().click(at.x + at.width / 2., at.y + at.height / 2.).unwrap();
    expect(&app.get_by_test_id("thread")).to_be_visible().unwrap();
    let video = app.get_by_test_id("att-demo-video-1");
    video.scroll_into_view().ok();
    std::thread::sleep(Duration::from_millis(800));
    let at = video.bounding_box().unwrap().expect("the video block is laid out");
    let (x, y) = (at.x + at.width / 2., at.y + at.height / 2.);
    app.mouse().click(x, y).unwrap();
    std::thread::sleep(Duration::from_millis(120));
    app.mouse().click(x, y).unwrap();
    expect(&app.get_by_test_id("lightbox-video-demo-video-1")).to_be_visible().unwrap();
    let position = |app: &nd_test::AppHandle| {
        let snapshot = app.snapshot().unwrap();
        assert!(snapshot.contains("lightbox-video-demo-video-1"), "the viewer stayed open, so it plays:\n{snapshot}");
        assert!(!snapshot.contains("[testid=lightbox-play] [hidden]"), "the transport is on screen:\n{snapshot}");
        let line = snapshot.lines().find(|l| l.contains("[testid=lightbox-seek]")).unwrap_or_default();
        line.split("[value=").nth(1).unwrap_or_default().to_owned()
    };
    std::thread::sleep(Duration::from_millis(900));
    let first = shoot("viewer-1.png");
    assert!(!position(&app).starts_with("\"0:00"), "the double click left it playing");

    // A click on the picture pauses it, and the position holds; another plays on.
    let picture = app.get_by_test_id("lightbox-picture").bounding_box().unwrap().expect("the picture is laid out");
    let (px, py) = (picture.x + picture.width / 2., picture.y + picture.height / 2.);
    app.mouse().click(px, py).unwrap();
    std::thread::sleep(Duration::from_millis(600));
    let paused = position(&app);
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(position(&app), paused, "paused playback holds its position");
    app.mouse().click(px, py).unwrap();
    std::thread::sleep(Duration::from_millis(900));
    let second = shoot("viewer-2.png");
    assert_ne!(std::fs::read(&first).unwrap(), std::fs::read(&second).unwrap(), "the picture moves");
}

/// The app's main window from the screen: a release build has no GPUI capture, so the
/// desktop is copied at the window's rectangle.
#[cfg(windows)]
fn grab_windows(pid: u32, out: &std::path::Path) {
    let script = format!(
        "Add-Type -AssemblyName System.Drawing; Add-Type -Namespace W -Name U -MemberDefinition '[DllImport(\"user32.dll\")] public static extern bool GetWindowRect(System.IntPtr h, out RECT r); [DllImport(\"user32.dll\")] public static extern bool SetProcessDPIAware(); public struct RECT {{ public int L, T, R, B; }}'; \
         [W.U]::SetProcessDPIAware() | Out-Null; $h = (Get-Process -Id {pid}).MainWindowHandle; $r = New-Object W.U+RECT; [W.U]::GetWindowRect($h, [ref]$r) | Out-Null; \
         $b = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T); $g = [System.Drawing.Graphics]::FromImage($b); \
         $g.CopyFromScreen($r.L, $r.T, 0, 0, $b.Size); $b.Save('{}')",
        out.display()
    );
    let status = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &script]).status().unwrap();
    assert!(status.success(), "the window capture failed");
}

/// `GPUIX_BACKGROUND=1` opens the window without making it key.
#[test]
fn real_a_background_launch_does_not_take_focus() {
    if !real_allowed() {
        println!("skipped: opens a real window; run with ND_REAL=1");
        return;
    }
    let app = launch_app(
        LaunchOptions::new(env!("CARGO_BIN_EXE_messages")).env("MESSAGES_DEMO", "1").env("GPUIX_BACKGROUND", "1"),
    )
    .unwrap();
    expect(&app.get_by_test_id("chat-+14155550134")).to_be_visible().unwrap();
    let tree = app.tree(None).unwrap();
    let window = &tree.windows[0];
    assert!(window.visible, "the window is on screen");
    assert!(!window.key, "the window did not take focus");
}

fn bounds_of(app: &nd_test::AppHandle, id: &str) -> Option<(f32, f32, f32, f32)> {
    app.get_by_test_id(id).bounding_box().ok().flatten().map(|b| (b.x, b.y, b.width, b.height))
}

/// The composer empty, with a word, and grown to several lines: captured, and the buttons'
/// centres checked against the field's single line, then their bottoms against its last.
#[test]
fn real_composer_buttons_line_up_with_the_field() {
    if !real_allowed() {
        println!("skipped: opens a real window; run with ND_REAL=1");
        return;
    }
    let app = launch_app(
        LaunchOptions::new(env!("CARGO_BIN_EXE_messages"))
            .env("MESSAGES_DEMO", "1")
            .env("ND_AUTOMATION_CAPTURE", "region"),
    )
    .unwrap();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let row = app.get_by_test_id("chat-chloe@martin.design");
    expect(&row).to_be_visible().unwrap();
    let at = row.bounding_box().unwrap().expect("the row is laid out");
    app.mouse().click(at.x + at.width / 2., at.y + at.height / 2.).unwrap();
    expect(&app.get_by_test_id("composer.field")).to_be_visible().unwrap();
    std::thread::sleep(Duration::from_millis(800));
    let centre = |b: (f32, f32, f32, f32)| b.1 + b.3 / 2.;
    let report = |what: &str| {
        let field = bounds_of(&app, "composer.field").expect("field");
        println!("{what}: field {field:?}");
        for id in ["composer.attach", "composer.emoji", "composer.send"] {
            if let Some(b) = bounds_of(&app, id) {
                println!(
                    "{what}: {id} {b:?} centre off {:.1} bottom off {:.1}",
                    centre(b) - centre(field),
                    (b.1 + b.3) - (field.1 + field.3)
                );
            }
        }
        let shot = dir.join(format!("shot-composer-{what}.png"));
        app.screenshot(&shot, None).unwrap();
        println!("screenshot {}", shot.display());
        field
    };
    let empty = report("empty");
    for id in ["composer.attach", "composer.emoji"] {
        let b = bounds_of(&app, id).expect(id);
        assert!((centre(b) - centre(empty)).abs() <= 1., "{id} centres on the field: {b:?} against {empty:?}");
    }
    app.get_by_test_id("composer.field").type_text("Hello there").unwrap();
    std::thread::sleep(Duration::from_millis(400));
    let one = report("word");
    let send = bounds_of(&app, "composer.send").expect("the send button shows with text");
    assert!((centre(send) - centre(one)).abs() <= 1., "send centres on the field: {send:?} against {one:?}");
    // Keys reach only the key window: a click in the field makes it so.
    let f = bounds_of(&app, "composer.field").expect("field");
    app.mouse().click(f.0 + 200., f.1 + f.3 / 2.).unwrap();
    app.get_by_test_id("composer.field").press("shift-enter").unwrap();
    app.get_by_test_id("composer.field").type_text("second line").unwrap();
    app.get_by_test_id("composer.field").press("shift-enter").unwrap();
    app.get_by_test_id("composer.field").type_text("third line").unwrap();
    std::thread::sleep(Duration::from_millis(400));
    let tall = report("lines");
    assert!(tall.3 > one.3 + 20., "the field grew: {tall:?}");
    for id in ["composer.attach", "composer.emoji", "composer.send"] {
        let b = bounds_of(&app, id).expect(id);
        assert!(((b.1 + b.3) - (tall.1 + tall.3)).abs() <= 1., "{id} stays on the last line: {b:?} against {tall:?}");
    }
}

/// At launch the message field has the keyboard, as in Messages, not the sidebar's search.
#[test]
fn real_launch_focuses_the_message_field() {
    if !real_allowed() {
        println!("skipped: opens a real window; run with ND_REAL=1");
        return;
    }
    let app = launch_app(LaunchOptions::new(env!("CARGO_BIN_EXE_messages")).env("MESSAGES_DEMO", "1")).unwrap();
    expect(&app.get_by_test_id("composer.field")).to_be_visible().unwrap();
    std::thread::sleep(Duration::from_millis(800));
    let tree = app.tree(None).unwrap();
    let focused: Vec<String> =
        tree.windows[0].root.iter().filter(|n| n.focused).filter_map(|n| n.test_id.clone()).collect();
    assert!(focused.iter().any(|id| id == "composer.field"), "the message field has focus, not {focused:?}");
}

/// A real two-finger swipe right over a received bubble (posted at the HID tap) opens Reply; a
/// click elsewhere closes it, with frames across the close so the arrow can be checked for
/// clipping. A long swipe then runs Reply.
#[cfg(target_os = "macos")]
#[test]
fn real_swipe_to_reply_and_back_is_captured() {
    use ndshell::input::{GesturePhase, scroll_event};
    if !real_allowed() {
        println!("skipped: opens a real window; run with ND_REAL=1");
        return;
    }
    let app = launch_app(
        LaunchOptions::new(env!("CARGO_BIN_EXE_messages"))
            .env("MESSAGES_DEMO", "1")
            .env("ND_AUTOMATION_CAPTURE", "region"),
    )
    .unwrap();
    expect(&app.get_by_test_id("row-demo-msg-0049")).to_be_visible().unwrap();
    std::thread::sleep(Duration::from_millis(800));
    let tree = app.tree(None).unwrap();
    let frame = tree.windows[0].frame.expect("the window's frame");
    let row = app.get_by_test_id("row-demo-msg-0049").bounding_box().unwrap().expect("laid out");
    let at = (f64::from(frame.origin.x + row.x + row.width / 3.), f64::from(frame.origin.y + row.y + row.height / 2.));
    let main = windows_of(app.pid()).first().map(|w| w.0).expect("a window");
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let shoot = |name: &str| {
        let shot = dir.join(format!("shot-swipe-{name}.png"));
        let _ =
            std::process::Command::new("screencapture").args(["-x", "-o", "-l", &main.to_string()]).arg(&shot).status();
        println!("screenshot {}", shot.display());
    };
    let post = |dx: i32, phase: GesturePhase| {
        ndshell::input::post(scroll_event(at, dx, 0, phase));
        std::thread::sleep(Duration::from_millis(16));
    };
    ndshell::input::move_to(at);
    std::thread::sleep(Duration::from_millis(200));
    // A click on the bubble leaves its row unhighlighted: the transcript has no selection.
    ndshell::input::click(at);
    std::thread::sleep(Duration::from_millis(300));
    shoot("clicked");
    // Fingers to the right: the row slides right and shows Reply, which stays open once the
    // fingers lift.
    post(0, GesturePhase::Began);
    for _ in 0..15 {
        post(20, GesturePhase::Changed);
    }
    // The fingers rest before they lift, so the release carries no fling into a full swipe.
    std::thread::sleep(Duration::from_millis(200));
    post(0, GesturePhase::Changed);
    post(0, GesturePhase::Ended);
    std::thread::sleep(Duration::from_millis(700));
    shoot("open");
    // A click elsewhere closes it: the row slides back with the arrow whole, recorded as it goes
    // (a screenshot takes longer than the slide).
    let video = dir.join("shot-swipe-back.mov");
    let _ = std::fs::remove_file(&video);
    let rect = format!(
        "-R{},{},{},{}",
        frame.origin.x + row.x,
        frame.origin.y + row.y - 20.,
        row.width,
        row.height + 40.
    );
    let mut recording =
        std::process::Command::new("screencapture").args(["-x", "-v", "-V2", &rect]).arg(&video).spawn().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    ndshell::input::click((at.0 + 400., at.1));
    let _ = recording.wait();
    println!("recording {}", video.display());
    shoot("closed");
    // A long swipe runs Reply and the row slides back on its own.
    std::thread::sleep(Duration::from_millis(300));
    post(0, GesturePhase::Began);
    for _ in 0..40 {
        post(20, GesturePhase::Changed);
    }
    shoot("full");
    post(0, GesturePhase::Ended);
    std::thread::sleep(Duration::from_millis(700));
    shoot("full-after");
}
