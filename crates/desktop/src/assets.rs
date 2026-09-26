//! The app icon for desktop notifications, the same file
//! `scripts/install-linux.sh` installs into the icon theme.

use std::path::{Path, PathBuf};

/// Resolved against the source tree, which only holds for a binary built from
/// a checkout; a packaged build has to ship this file and point here at it.
pub fn icon_svg_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/linux/es.canarycoders.messages.svg")
}
