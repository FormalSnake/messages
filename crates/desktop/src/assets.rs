//! The app icon for desktop notifications, the same file
//! `scripts/install-linux.sh` installs into the icon theme.

use std::path::{Path, PathBuf};

/// A packaged build sets `MESSAGES_ICON_PATH` at compile time to where it
/// installs the icon; a build from a checkout reads it from the source tree.
pub fn icon_svg_path() -> PathBuf {
    match option_env!("MESSAGES_ICON_PATH") {
        Some(path) => PathBuf::from(path),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/linux/es.canarycoders.messages.svg"),
    }
}
