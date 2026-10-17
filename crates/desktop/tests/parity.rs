//! Tier 3: the parity items that only a running window shows (docs/nd-parity.md): the password
//! eye, a bold subject, Attach by Path, the font override and a new row fading in. Captures
//! land in the test's target tmp dir.
//!
//! The connect screen runs without the demo: every launch here gets its own HOME, config and
//! cache directories under the target dir, so the user's real configuration is never read.
//!
//! It opens a real window, so it runs only under nextest or with `ND_REAL=1`.

use std::path::PathBuf;
use std::time::Duration;

use nd_test::{LaunchOptions, expect, launch_app};

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
        .filter(|w| w.1 > 40 && w.2 > 40)
        .collect();
    found.sort_by_key(|w| std::cmp::Reverse(w.1 * w.2));
    found
}

fn real_allowed() -> bool {
    let allowed = std::env::var_os("NEXTEST").is_some() || std::env::var_os("ND_REAL").is_some_and(|v| v == "1");
    if !allowed {
        println!("skipped: opens a real window; run with ND_REAL=1");
    }
    allowed
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A launch with its own home, so nothing of the user's is read or written.
fn isolated(name: &str) -> LaunchOptions {
    let home = tmp(name);
    LaunchOptions::new(env!("CARGO_BIN_EXE_messages"))
        .env("HOME", home.to_str().unwrap())
        .env("XDG_CONFIG_HOME", home.join("config").to_str().unwrap())
        .env("XDG_CACHE_HOME", home.join("cache").to_str().unwrap())
        .env("XDG_DATA_HOME", home.join("data").to_str().unwrap())
        .env("ND_AUTOMATION_CAPTURE", "region")
}

fn shot(app: &nd_test::AppHandle, name: &str) {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("parity-{name}.png"));
    if let Err(e) = app.screenshot(&path, None) {
        panic!("{e:?}\n{}", app.stderr_tail(200).join("\n"));
    }
    println!("screenshot {}", path.display());
}

#[test]
fn real_the_password_eye_shows_and_hides_the_password() {
    if !real_allowed() {
        return;
    }
    let app = launch_app(isolated("connect")).unwrap();
    let field = app.get_by_test_id("server-password");
    expect(&field).to_be_visible().unwrap();
    field.type_text("hunter2").unwrap();
    let value = |app: &nd_test::AppHandle| {
        let tree = app.tree(None).unwrap();
        tree.windows[0].root.iter().find(|n| n.test_id.as_deref() == Some("server-password")).and_then(|n| n.value.clone())
    };
    let hidden = value(&app);
    shot(&app, "password-hidden");
    let b = field.bounding_box().unwrap().expect("laid out");
    app.mouse().click(b.x + b.width - 12., b.y + b.height / 2.).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    let shown = value(&app);
    shot(&app, "password-shown");
    assert_ne!(hidden.as_deref(), Some("hunter2"), "a secure field keeps its value from automation: {hidden:?}");
    assert_eq!(shown.as_deref(), Some("hunter2"), "the eye shows the password");
}

/// Opens the conversation called `name`: scrolls the sidebar to its row and clicks it.
fn open_chat(app: &nd_test::AppHandle, name: &str) {
    let id = match name {
        "Chloe Martin" => "chat-chloe@martin.design",
        other => panic!("no row for {other}"),
    };
    let row = app.get_by_test_id(id);
    expect(&app.get_by_test_id("chats")).to_be_visible().unwrap();
    for _ in 0..10 {
        let inside = row.bounding_box().ok().flatten().is_some_and(|b| b.y > 100. && b.y + b.height < 700.);
        if inside {
            break;
        }
        app.get_by_test_id("chats").scroll(0., 200.).unwrap();
        std::thread::sleep(Duration::from_millis(200));
    }
    let b = row.bounding_box().unwrap().expect("laid out");
    app.mouse().click(b.x + b.width / 2., b.y + b.height / 2.).unwrap();
    expect(&app.get_by_test_id("thread")).to_be_visible().unwrap();
    std::thread::sleep(Duration::from_millis(600));
}

#[test]
fn real_a_subject_and_the_font_override_show() {
    if !real_allowed() {
        return;
    }
    let app = launch_app(isolated("subject").env("MESSAGES_DEMO", "1")).unwrap();
    open_chat(&app, "Chloe Martin");
    shot(&app, "subject");
    // One window at a time holds the machine's input lock.
    drop(app);
    let app = launch_app(isolated("font").env("MESSAGES_DEMO", "1").env("MESSAGES_FONT", "Courier New")).unwrap();
    open_chat(&app, "Chloe Martin");
    shot(&app, "font");
}

#[test]
fn real_attach_by_path_stages_the_file() {
    if !real_allowed() {
        return;
    }
    let file = tmp("attach").join("notes.txt");
    std::fs::write(&file, "hello").unwrap();
    let app = launch_app(isolated("attach").env("MESSAGES_DEMO", "1")).unwrap();
    open_chat(&app, "Chloe Martin");
    app.get_by_test_id("composer.attach").click().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let main = windows_of(app.pid());
    for (i, (id, _, _)) in main.iter().enumerate() {
        let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("parity-menu-{i}.png"));
        let _ = std::process::Command::new("screencapture").args(["-x", "-o", "-l", &id.to_string()]).arg(&path).status();
        println!("screenshot {}", path.display());
    }
    expect(&app.get_by_test_id("attach-path")).to_be_visible().unwrap();
    app.get_by_test_id("attach-path").click().unwrap();
    let field = app.get_by_test_id("attach-path-field");
    expect(&field).to_be_visible().unwrap();
    field.type_text(file.to_str().unwrap()).unwrap();
    field.press("enter").unwrap();
    std::thread::sleep(Duration::from_millis(500));
    shot(&app, "attach-path");
    let tree = app.tree(None).unwrap();
    let chips: Vec<String> = tree.windows[0]
        .root
        .iter()
        .filter_map(|n| n.test_id.clone())
        .filter(|id| id.starts_with("composer.chip."))
        .collect();
    assert_eq!(chips.len(), 1, "the typed path is staged: {chips:?}");
}

#[test]
fn real_a_new_row_fades_in() {
    if !real_allowed() {
        return;
    }
    let app = launch_app(isolated("fade").env("MESSAGES_DEMO", "1")).unwrap();
    open_chat(&app, "Chloe Martin");
    let field = app.get_by_test_id("composer.field");
    // A click on the field first: keys reach only the key window.
    let b = field.bounding_box().unwrap().expect("laid out");
    app.mouse().click(b.x + 20., b.y + b.height / 2.).unwrap();
    field.fill("fading in").unwrap();
    field.press("enter").unwrap();
    // The moment the row is in the tree, capture it a few times while it fades in.
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        let tree = app.tree(None).unwrap();
        if tree.windows[0].root.iter().any(|n| n.name.contains("fading in") && n.test_id.as_deref().is_some_and(|t| t.starts_with("bubble-"))) {
            break;
        }
    }
    println!("row seen after {:?}", start.elapsed());
    for i in 0..5 {
        shot(&app, &format!("fade-{i}"));
    }
}

#[test]
fn real_f12_cycles_the_frame_overlay() {
    if !real_allowed() {
        return;
    }
    let app = launch_app(isolated("hud").env("MESSAGES_DEMO", "1")).unwrap();
    open_chat(&app, "Chloe Martin");
    let field = app.get_by_test_id("composer.field");
    // A click on the field first: keys reach only the key window.
    let b = field.bounding_box().unwrap().expect("laid out");
    app.mouse().click(b.x + 20., b.y + b.height / 2.).unwrap();
    let lines = |app: &nd_test::AppHandle| -> Vec<String> {
        let tree = app.tree(None).unwrap();
        tree.windows[0].root.iter().map(|n| n.name.to_string()).filter(|n| n.ends_with(" fps") || n.ends_with(" dropped")).collect()
    };
    assert!(lines(&app).is_empty(), "the overlay starts hidden");
    field.press("f12").unwrap();
    std::thread::sleep(Duration::from_millis(400));
    let minimal = lines(&app);
    assert!(minimal.len() == 1 && minimal[0].ends_with(" fps"), "F12 shows the rate: {minimal:?}");
    shot(&app, "hud-minimal");
    field.press("f12").unwrap();
    std::thread::sleep(Duration::from_millis(400));
    let full = lines(&app);
    assert!(full.iter().any(|l| l.ends_with(" dropped")), "F12 again adds frame time and drops: {full:?}");
    shot(&app, "hud-full");
    field.press("f12").unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert!(lines(&app).is_empty(), "a third F12 hides it");
}
