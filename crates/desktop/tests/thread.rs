//! Tier 3: the real app on the demo data, driven over its automation socket. Opens a
//! conversation, scrolls the thread to its first message and back, and checks that the last
//! message is fully on screen at the end: the bug class where rows that grow after they load
//! (tall screenshots, a GIF, a video poster) push the tail out of reach.
//!
//! It opens a real window, so it runs only under nextest or with `ND_REAL=1`.

use std::time::{Duration, Instant};

use nd_test::{LaunchOptions, SemanticNode, expect, launch_app};

const ALEX: &str = "chat-+14155550134";

fn real_allowed() -> bool {
    let allowed = std::env::var_os("NEXTEST").is_some() || std::env::var_os("ND_REAL").is_some_and(|v| v == "1");
    if !allowed {
        println!("skipped: opens a real window; run with ND_REAL=1");
    }
    allowed
}

fn find<'a>(node: &'a SemanticNode, test_id: &str) -> Option<&'a SemanticNode> {
    if node.test_id.as_deref() == Some(test_id) {
        return Some(node);
    }
    node.children.iter().find_map(|child| find(child, test_id))
}

/// The thread's message rows on screen, in order, with their bounds. Rows the table stopped
/// showing keep their last frame and still read as visible (docs/nd-parity.md), so a row only
/// counts when it starts below the one before it.
fn rows(node: &SemanticNode, out: &mut Vec<(String, f32, f32)>) {
    let mut all = Vec::new();
    collect(node, &mut all);
    let mut bottom = f32::MIN;
    for (id, top, end) in all {
        if top >= bottom - 1. {
            bottom = end;
            out.push((id, top, end));
        }
    }
}

fn collect(node: &SemanticNode, out: &mut Vec<(String, f32, f32)>) {
    if let (Some(id), Some(bounds)) = (node.test_id.as_deref(), node.bounds) {
        if id.starts_with("row-") && node.visible {
            out.push((id.to_owned(), bounds.origin.y, bounds.origin.y + bounds.size.height));
        }
    }
    for child in &node.children {
        collect(child, out);
    }
}

#[test]
fn opening_a_thread_scrolling_to_the_top_and_back_keeps_the_last_row_visible() {
    if !real_allowed() {
        return;
    }
    let app = launch_app(
        LaunchOptions::new(env!("CARGO_BIN_EXE_messages"))
            .env("MESSAGES_DEMO", "1")
            .env("ND_AUTOMATION_CAPTURE", "region"),
    )
    .unwrap();
    expect(&app.get_by_test_id(ALEX)).to_be_visible().unwrap();
    let shot = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("launch.png");
    app.screenshot(&shot, None).unwrap();
    println!("screenshot {}\n{}", shot.display(), app.snapshot().unwrap());
    // A list row is not a button, so the semantic click does not take it: the pointer does.
    let row = app.get_by_test_id(ALEX).bounding_box().unwrap().expect("the row is laid out");
    app.mouse().click(row.x + row.width / 2., row.y + row.height / 2.).unwrap();
    expect(&app.get_by_test_id("chat-title")).to_contain_text("Alex").unwrap();
    expect(&app.get_by_test_id("thread")).to_be_visible().unwrap();

    let snapshot = || {
        let tree = app.tree(None).unwrap();
        let root = &tree.windows[0].root;
        let list = find(root, "thread").and_then(|n| n.bounds).expect("the thread is laid out");
        let mut found = Vec::new();
        rows(find(root, "thread").unwrap(), &mut found);
        (list.origin.y, list.origin.y + list.size.height, found)
    };
    // The tail settles once the pictures in it have their size.
    let settle = |what: &str, check: &dyn Fn(f32, f32, &[(String, f32, f32)]) -> bool| {
        let started = Instant::now();
        loop {
            let (top, bottom, found) = snapshot();
            if check(top, bottom, &found) {
                return found;
            }
            assert!(started.elapsed() < Duration::from_secs(10), "{what}: list {top}..{bottom}, rows {found:#?}");
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    let last_visible = |_: f32, bottom: f32, found: &[(String, f32, f32)]| {
        found.last().is_some_and(|(_, row_top, row_bottom)| *row_bottom <= bottom + 1. && *row_bottom > *row_top)
    };
    let opened = settle("the thread opens at its end", &last_visible);
    let last = opened.last().unwrap().0.clone();
    assert_eq!(last, "row-demo-msg-0014", "the newest message is the one at the bottom");
    println!("opened {:?}", snapshot());
    let shot = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("thread-open.png");
    app.screenshot(&shot, None).unwrap();
    println!("screenshot {}", shot.display());

    // Up to the first message: the first row sits at or below the list's top edge.
    let thread = app.get_by_test_id("thread");
    let at_top = |top: f32, _: f32, found: &[(String, f32, f32)]| {
        found.first().is_some_and(|(id, row_top, _)| id == "row-demo-msg-0001" && *row_top >= top - 1.)
    };
    let started = Instant::now();
    loop {
        let (top, bottom, found) = snapshot();
        if at_top(top, bottom, &found) {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(15), "never reached the top: {found:#?}");
        thread.scroll(0., -600.).unwrap();
    }
    let (_, bottom, found) = snapshot();
    println!("at top {:?}", snapshot());
    assert!(found.iter().all(|(id, ..)| id != &last), "the last row is still on screen at the top: {found:?} {bottom}");

    // And back down: the last row is whole again, the same one as before.
    let started = Instant::now();
    loop {
        let (top, bottom, found) = snapshot();
        if last_visible(top, bottom, &found) && found.last().is_some_and(|(id, ..)| *id == last) {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(15), "the tail is out of reach: {found:#?}");
        thread.scroll(0., 600.).unwrap();
    }
    let shot = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("thread-back.png");
    app.screenshot(&shot, None).unwrap();
    println!("screenshot {}", shot.display());
}

/// Opens Alex's conversation in a fresh demo launch.
fn open_alex() -> nd_test::AppHandle {
    let app = launch_app(LaunchOptions::new(env!("CARGO_BIN_EXE_messages")).env("MESSAGES_DEMO", "1")).unwrap();
    expect(&app.get_by_test_id(ALEX)).to_be_visible().unwrap();
    let row = app.get_by_test_id(ALEX).bounding_box().unwrap().expect("the row is laid out");
    app.mouse().click(row.x + row.width / 2., row.y + row.height / 2.).unwrap();
    expect(&app.get_by_test_id("chat-title")).to_contain_text("Alex").unwrap();
    app
}

#[test]
fn sending_shows_the_bubble_and_the_demo_answers() {
    if !real_allowed() {
        return;
    }
    let app = open_alex();
    let field = app.get_by_test_id("composer.field");
    // Keys reach only the key window, which a window launched right after another test's may
    // not be yet: a click in the field makes it so.
    let b = field.bounding_box().unwrap().expect("laid out");
    app.mouse().click(b.x + 40., b.y + b.height / 2.).unwrap();
    field.fill("hello from the port").unwrap();
    field.press("enter").unwrap();
    expect(&app.get_by_text("hello from the port").first()).to_be_visible().unwrap();
    expect(&field).to_have_value("").unwrap();
}

#[test]
fn the_switcher_jumps_to_a_conversation() {
    if !real_allowed() {
        return;
    }
    let app = open_alex();
    app.keyboard().press("cmd-k").unwrap();
    let search = app.get_by_test_id("switcher.search");
    expect(&search).to_be_visible().unwrap();
    app.keyboard().type_text("family").unwrap();
    expect(&search).to_have_value("family").unwrap();
    // The palette's rows are native table cells outside the semantic tree; give the query's
    // results a moment to replace the unfiltered ones before Return takes the top row.
    std::thread::sleep(Duration::from_millis(300));
    app.keyboard().press("enter").unwrap();
    expect(&app.get_by_test_id("chat-title")).to_contain_text("Family").unwrap();
}

#[test]
fn details_open_and_close_from_the_toolbar() {
    if !real_allowed() {
        return;
    }
    let app = open_alex();
    let width = || app.get_by_test_id("thread-view").bounding_box().unwrap().map_or(0., |b| b.width);
    let wide = width();
    app.get_by_test_id("chat-title").click().unwrap();
    expect(&app.get_by_text("Photos and Files").first()).to_be_visible().unwrap();
    let started = Instant::now();
    while width() > wide - 200. {
        assert!(started.elapsed() < Duration::from_secs(5), "the inspector never took its room");
        std::thread::sleep(Duration::from_millis(50));
    }
    // A collapsed inspector keeps its nodes, still reported visible (docs/nd-parity.md), so the
    // thread getting its width back is what says the panel closed.
    app.get_by_test_id("chat-title").click().unwrap();
    let started = Instant::now();
    while width() < wide - 1. {
        assert!(started.elapsed() < Duration::from_secs(5), "the inspector never gave its room back");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_new_conversation_starts_from_a_typed_address() {
    if !real_allowed() {
        return;
    }
    let app = open_alex();
    app.get_by_test_id("new-chat").click().unwrap();
    let to = app.get_by_test_id("new-chat-to");
    expect(&to).to_be_visible().unwrap();
    to.fill("+1 415 555 0123").unwrap();
    to.press("enter").unwrap();
    expect(&app.get_by_test_id("recipient-0")).to_be_visible().unwrap();
    let field = app.get_by_test_id("new-chat-composer.field");
    field.fill("hi there").unwrap();
    field.press("enter").unwrap();
    expect(&app.get_by_test_id("new-chat-view")).to_be_hidden().unwrap();
}
