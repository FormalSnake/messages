//! `MESSAGES_TRACE=1` startup timing on stderr: process start to the window's first render and
//! to the first render with conversation rows. nd paints in the frame that renders, so these are
//! within a frame of the paint. Every call is a no-op when the variable is unset.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, OnceLock};
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();
static ENABLED: LazyLock<bool> =
    LazyLock::new(|| std::env::var("MESSAGES_TRACE").is_ok_and(|value| !value.is_empty() && value != "0"));

pub static FIRST_RENDER: AtomicBool = AtomicBool::new(false);
pub static FIRST_ROWS: AtomicBool = AtomicBool::new(false);

/// First thing in `run`, so the times count from process start.
pub fn init() {
    START.get_or_init(Instant::now);
}

/// Logs `what` the first time `seen` is passed.
pub fn once(seen: &AtomicBool, what: &str) {
    if !*ENABLED || seen.swap(true, Ordering::Relaxed) {
        return;
    }
    let start = START.get().copied().unwrap_or_else(Instant::now);
    eprintln!("[trace +{:.1}ms] {what}", start.elapsed().as_secs_f64() * 1000.);
}
