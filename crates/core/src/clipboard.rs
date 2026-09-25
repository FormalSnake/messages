//! Port of packages/core/src/clipboard.ts: files and images through the
//! clipboard. Plain text goes through GPUI's own clipboard in the desktop crate.
//! Linux: wl-copy / wl-paste on Wayland, xclip on X11. macOS: osascript.
//! Windows: PowerShell.

use std::path::{Path, PathBuf};

/// `text/uri-list` to existing local paths; comments and non-file URIs dropped.
pub fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    let _ = text;
    unimplemented!()
}

/// Files or an image on the clipboard, saved into the attachment cache as `paste-<ms>.<ext>`. Empty for text only.
pub async fn clipboard_attachments() -> Vec<PathBuf> {
    unimplemented!()
}

/// Used where GPUI's clipboard is not reachable (the Windows toast path, tests).
pub async fn copy_text(text: &str) {
    let _ = text;
    unimplemented!()
}

/// A JPEG re-encoded once to `clip-<name>.png` beside the cache; PNG passes through; anything else is None.
pub async fn png_for(source: &Path, mime: &str) -> Option<PathBuf> {
    let _ = (source, mime);
    unimplemented!()
}

/// An image goes on as the bitmap (plus the file on macOS and Windows), anything else as a file.
pub async fn copy_file(source: &Path, mime: &str) {
    let _ = (source, mime);
    unimplemented!()
}
