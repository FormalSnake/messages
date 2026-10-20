//! Tier 3 on the GPUI backend (Linux, Windows): the real app on the demo data, driven with
//! pointer clicks. The composer's emoji button opens nd's picker at itself (Linux), a click beside
//! the composer ends editing, a click on the viewer's dim closes it, and a file attachment opens
//! in the system's app and saves through Save As.
//!
//! It opens a real window, so it runs only under nextest or with `ND_REAL=1`.
#![cfg(any(target_os = "linux", windows))]

use std::time::Duration;

mod common;

use nd_test::{LaunchOptions, expect};

const ALEX: &str = "chat-+14155550134";

fn real_allowed() -> bool {
    let allowed = std::env::var_os("NEXTEST").is_some() || std::env::var_os("ND_REAL").is_some_and(|v| v == "1");
    if !allowed {
        println!("skipped: opens a real window; run with ND_REAL=1");
    }
    allowed
}

fn open_alex() -> common::Running {
    let app = common::launch(LaunchOptions::new(env!("CARGO_BIN_EXE_messages")).env("MESSAGES_DEMO", "1"));
    let row = app.get_by_test_id(ALEX);
    expect(&row).to_be_visible().unwrap();
    let at = row.bounding_box().unwrap().expect("the row is laid out");
    app.mouse().click(at.x + at.width / 2., at.y + at.height / 2.).unwrap();
    expect(&app.get_by_test_id("thread")).to_be_visible().unwrap();
    app
}

#[test]
fn real_a_click_beside_the_composer_ends_editing() {
    if !real_allowed() {
        return;
    }
    let app = open_alex();
    let field = app.get_by_test_id("composer.field");
    let f = field.bounding_box().unwrap().expect("the field is laid out");
    app.mouse().click(f.x + f.width / 2., f.y + f.height / 2.).unwrap();
    expect(&field).to_be_focused().unwrap();
    let thread = app.get_by_test_id("thread").bounding_box().unwrap().expect("the thread is laid out");
    app.mouse().click(thread.x + thread.width / 2., thread.y + 12.).unwrap();
    expect(&field).not().to_be_focused().unwrap();
}

/// nd's picker (Linux) hangs from the button: a click just over the button lands in it, so the
/// picker stays open and the typed search never reaches the field.
#[cfg(target_os = "linux")]
#[test]
fn real_the_emoji_picker_opens_at_its_button() {
    if !real_allowed() {
        return;
    }
    let app = open_alex();
    let field = app.get_by_test_id("composer.field");
    let f = field.bounding_box().unwrap().expect("the field is laid out");
    app.mouse().click(f.x + 8., f.y + f.height / 2.).unwrap();
    let emoji = app.get_by_test_id("composer.emoji").bounding_box().unwrap().expect("the button is laid out");
    app.mouse().click(emoji.x + emoji.width / 2., emoji.y + emoji.height / 2.).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    app.mouse().click(emoji.x + emoji.width - 4., emoji.y - 8.).unwrap();
    app.keyboard().type_text("cat").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    expect(&field).to_have_value("").unwrap();
    app.keyboard().press("escape").unwrap();
}

#[test]
fn real_a_click_on_the_viewer_dim_closes_it() {
    if !real_allowed() {
        return;
    }
    let app = open_alex();
    let gif = app.get_by_test_id("att-demo-gif-1");
    gif.scroll_into_view().ok();
    std::thread::sleep(Duration::from_millis(800));
    let g = gif.bounding_box().unwrap().expect("the picture is laid out");
    app.mouse().double_click(g.x + g.width / 2., g.y + g.height / 2.).unwrap();
    let viewer = app.get_by_test_id("lightbox");
    expect(&viewer).to_be_visible().unwrap();
    std::thread::sleep(Duration::from_millis(800));
    let v = viewer.bounding_box().unwrap().expect("the viewer is laid out");
    // The bar under the picture holds nothing for a still picture: only the dim is there.
    app.mouse().click(v.x + v.width / 2., v.y + v.height - 20.).unwrap();
    expect(&viewer).not().to_be_attached().unwrap();
}

/// Nadia's booking PDF is not here yet: a click fetches it, showing that on the bubble, then
/// hands it to the system's opener (on Linux a stub `xdg-open` that records its argument, on
/// Windows the PDF app or Open With, found among the processes); Save As from the bubble's menu
/// copies it where the save dialog (scripted) says.
#[test]
fn real_a_file_attachment_opens_and_saves() {
    if !real_allowed() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("file-attachment");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let saved = dir.join("saved.pdf");
    let script = serde_json::json!({ "dialog.saveFile": [{ "path": saved }] }).to_string();
    let options = LaunchOptions::new(env!("CARGO_BIN_EXE_messages"))
        .env("MESSAGES_DEMO", "1")
        .env("ND_AUTOMATION_DIALOG_SCRIPT", script);
    #[cfg(target_os = "linux")]
    let (options, opened) = {
        let opened = dir.join("opened.txt");
        let stub = dir.join("xdg-open");
        std::fs::write(&stub, format!("#!/bin/sh\nprintf '%s' \"$1\" > '{}'\n", opened.display())).unwrap();
        std::fs::set_permissions(&stub, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default());
        (options.env("PATH", path), opened)
    };
    let app = common::launch(options);
    let row = app.get_by_test_id("chat-+34612345678");
    row.scroll_into_view().ok();
    expect(&row).to_be_visible().unwrap();
    std::thread::sleep(Duration::from_millis(600));
    let at = row.bounding_box().unwrap().expect("the row is laid out");
    app.mouse().click(at.x + at.width / 2., at.y + at.height / 2.).unwrap();
    expect(&app.get_by_test_id("chat-title")).to_contain_text("Nadia").unwrap();
    let file = app.get_by_test_id("att-demo-file-1");
    expect(&file).to_be_attached().unwrap();
    file.scroll_into_view().ok();
    std::thread::sleep(Duration::from_millis(600));
    let b = file.bounding_box().unwrap().expect("the file bubble is laid out");
    let (x, y) = (b.x + b.width / 2., b.y + b.height / 2.);
    app.mouse().click(x, y).unwrap();
    expect(&app.get_by_test_id("att-status-demo-file-1")).to_contain_text("Downloading").unwrap();
    #[cfg(target_os = "linux")]
    {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !opened.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        let target = std::fs::read_to_string(&opened).expect("the opener was called");
        assert!(target.ends_with("Booking confirmation.pdf"), "opened {target}");
    }
    #[cfg(windows)]
    {
        // The default PDF app, or Explorer's Open With when there is none (as in the g815 VM,
        // whose association points at a removed Edge).
        let find = "Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'OpenWith.exe' -or ($_.Name -ne 'powershell.exe' -and $_.CommandLine -like '*Booking confirmation.pdf*') } | ForEach-Object { \"$($_.ProcessId) $($_.Name)\" }";
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let mut opener = String::new();
        while opener.is_empty() && std::time::Instant::now() < deadline {
            opener = powershell(find);
        }
        assert!(!opener.is_empty(), "no process opened the PDF");
        println!("opened by {opener}");
        for pid in opener.lines().filter_map(|l| l.split(' ').next()) {
            powershell(&format!("Stop-Process -Id {pid} -ErrorAction SilentlyContinue"));
        }
    }
    expect(&app.get_by_test_id("att-status-demo-file-1")).not().to_be_attached().unwrap();

    // The menu opens at the press; Save As is its second row (rows are 8 px of padding around
    // a 20 px line, under the menu's 4 px inset and 1 px border).
    file.scroll_into_view().ok();
    std::thread::sleep(Duration::from_millis(600));
    let b = file.bounding_box().unwrap().expect("the file bubble is laid out");
    let (x, y) = (b.x + b.width / 2., b.y + b.height / 2.);
    file.right_click().unwrap();
    std::thread::sleep(Duration::from_millis(400));
    app.mouse().click(x + 40., y + 5. + 28. + 14.).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !saved.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    let bytes = std::fs::read(&saved).expect("Save As wrote the file");
    assert!(bytes.starts_with(b"%PDF"), "a PDF was saved");
}

#[cfg(windows)]
fn powershell(script: &str) -> String {
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}
