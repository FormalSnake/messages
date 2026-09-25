//! The app icon, shared by the desktop notification and (eventually) the X11
//! window icon. Reuses `apps/desktop/assets/icon.svg` rather than shipping a
//! second copy; `packaging/linux/es.canarycoders.messages.desktop` and
//! `scripts/install-linux-desktop.sh` install it into the Linux icon theme.

use std::path::{Path, PathBuf};

/// Resolved relative to the compiled binary's source tree, which only holds
/// for a `cargo run`/`cargo build` checkout; a packaged build should bundle
/// this file instead and point here at the bundled copy.
pub fn icon_svg_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/assets/icon.svg")
}
