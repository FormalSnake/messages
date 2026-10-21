//! The cold start budget on Linux (docs/rust-architecture.md): process start to the first render
//! and to the first render with conversation rows, from `MESSAGES_TRACE=1`, over ten launches in
//! a private headless Hyprland on the demo data, and the app id the window carries.
//! `MESSAGES_BIN` picks the binary (a release build for the budget); the default is this build's.
//!
//! It opens a real window, so it runs only under nextest or with `ND_REAL=1`.
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::time::{Duration, Instant};

fn real_allowed() -> bool {
    std::env::var_os("NEXTEST").is_some() || std::env::var_os("ND_REAL").is_some_and(|v| v == "1")
}

/// The `+ms` of a `[trace +12.3ms] what` line.
fn ms(line: &str) -> Option<f64> {
    line.strip_prefix("[trace +")?.split_once("ms]")?.0.parse().ok()
}

#[test]
fn real_cold_start() {
    if !real_allowed() {
        println!("skipped: opens a real window; run with ND_REAL=1");
        return;
    }
    let bin = std::env::var("MESSAGES_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_messages").to_owned());
    let compositor =
        ndshell::hyprland::Hyprland::start(ndshell::hyprland::Options { width: 1400, height: 900, ..Default::default() })
            .unwrap();
    let (mut first, mut rows) = (Vec::new(), Vec::new());
    for run in 0..10 {
        let mut child = compositor
            .command(&bin)
            .env("MESSAGES_DEMO", "1")
            .env("MESSAGES_TRACE", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = BufReader::new(child.stderr.take().unwrap());
        let deadline = Instant::now() + Duration::from_secs(20);
        let (mut f, mut r) = (None, None);
        for line in stderr.lines().map_while(Result::ok) {
            if line.ends_with("] first render") {
                f = ms(&line);
            } else if line.ends_with("] conversation rows render") {
                r = ms(&line);
                break;
            }
            if Instant::now() > deadline {
                break;
            }
        }
        if run == 0 {
            // The window's Wayland app id, which the desktop entry's StartupWMClass names.
            let client = compositor.wait_for_client(child.id(), Duration::from_secs(10)).unwrap();
            assert_eq!(client.class, "dev.formalsnake.messages");
        }
        child.kill().ok();
        child.wait().ok();
        println!("run {run}: first render {f:?} ms, rows {r:?} ms");
        first.extend(f);
        rows.extend(r);
        std::thread::sleep(Duration::from_millis(500));
    }
    let range = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        format!("{:.0} to {:.0} ms, median {:.0}", v[0], v[v.len() - 1], v[v.len() / 2])
    };
    assert!(!first.is_empty() && !rows.is_empty(), "the trace printed its marks");
    println!("first render {}; conversation rows {}", range(&mut first), range(&mut rows));
}
